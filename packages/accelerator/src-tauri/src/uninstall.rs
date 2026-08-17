//! B5: `--prepare-uninstall` — the ownership-checked teardown the NSIS uninstaller (and the
//! manual/scripted wrappers on other OSes) run BEFORE the app is deleted, while the exe is still present.
//!
//! The hazard this guards against is a SECOND install — a copied instance (#429) — that shares this
//! user's `~/.aztec-accelerator` state. Uninstalling THIS copy must not strip the OTHER copy's autostart
//! entry, kill its crash-recovery, or delete the CA trust + certs it still relies on. The stored autostart
//! entry is the ownership oracle: if it canonicalizes to a DIFFERENT binary (`points_elsewhere`), or we
//! cannot read it, another install is present (or ownership is uncertain) and we LEAVE all shared state,
//! reporting why. Only when the entry is OURS, BROKEN (was ours, target gone), or ABSENT do we remove
//! autostart + crash-recovery + trust + certs.
//!
//! The low-level per-artifact match (Windows Run-value token / scheduled-task `<Command>`) lives in the
//! NSIS `POSTUNINSTALL` belt, which runs AFTER the exe is gone and so cannot call back into this code; this
//! CLI path is the primary and reuses the `#429` canonicalized autostart probe as its single oracle.
//!
//! Every primitive is idempotent, so a re-run — the belt firing after a successful primary, or a retried
//! uninstall — is safe.

use crate::autostart::StoredTarget;

/// Whether this install owns the shared state, decided from the stored autostart entry. Mirrors
/// `autostart::implicit_arm_allowed`'s taxonomy (D10): a healthy entry pointing elsewhere, or one we
/// cannot read, is NOT ours — fail-closed, so uncertainty LEAVES the other install's state intact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ownership {
    /// Safe to remove shared state: the entry is ours, was ours (broken target), or absent.
    Ours,
    /// Another install owns the autostart entry, or we cannot read it — leave everything shared.
    Foreign,
}

fn classify_ownership(stored: &StoredTarget) -> Ownership {
    match stored {
        // Resolves to a DIFFERENT binary ⇒ a second install still wants it.
        StoredTarget::Healthy {
            points_elsewhere: true,
            ..
        } => Ownership::Foreign,
        // Cannot parse/read the entry ⇒ cannot PROVE it is ours ⇒ fail-closed.
        StoredTarget::Unreadable { .. } => Ownership::Foreign,
        // Ours, a broken entry that was ours, or nothing at all.
        StoredTarget::Healthy {
            points_elsewhere: false,
            ..
        }
        | StoredTarget::Broken { .. }
        | StoredTarget::Absent => Ownership::Ours,
    }
}

/// The disposition of one artifact after [`prepare_uninstall`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Removed, or already absent — removal is idempotent.
    Removed,
    /// Deliberately LEFT because another install owns it / relies on it.
    LeftForeign,
    /// Removal was attempted and did NOT complete — the caller must surface this (non-zero exit).
    Failed(String),
}

impl Step {
    fn failed(&self) -> bool {
        matches!(self, Step::Failed(_))
    }

    fn label(&self) -> String {
        match self {
            Step::Removed => "removed / absent".into(),
            Step::LeftForeign => "left (another install owns it)".into(),
            Step::Failed(why) => format!("FAILED — {why}"),
        }
    }
}

/// The result of [`prepare_uninstall`]: the ownership verdict plus one [`Step`] per artifact.
#[derive(Debug, Clone)]
pub struct UninstallOutcome {
    pub foreign_detected: bool,
    pub autostart: Step,
    pub crash_recovery: Step,
    pub trust: Step,
}

impl UninstallOutcome {
    /// Non-zero-exit trigger: any ATTEMPTED removal that failed. A deliberate `LeftForeign` is NOT a
    /// failure — it is the correct outcome when a second install is present, so it must exit 0.
    pub fn incomplete(&self) -> bool {
        self.autostart.failed() || self.crash_recovery.failed() || self.trust.failed()
    }

    /// Human-readable per-artifact lines for the CLI (mirrors `--remove-ca-trust`'s per-store output).
    pub fn report_lines(&self) -> Vec<String> {
        vec![
            format!("autostart entry:  {}", self.autostart.label()),
            format!("crash recovery:   {}", self.crash_recovery.label()),
            format!("CA trust + certs: {}", self.trust.label()),
        ]
    }
}

/// Ownership-checked teardown. See the module docs. Idempotent.
pub fn prepare_uninstall() -> UninstallOutcome {
    let reference = crate::autostart::owned_reference_path().ok();
    let stored = crate::autostart::read_stored_target(reference.as_deref());
    match classify_ownership(&stored) {
        Ownership::Foreign => UninstallOutcome {
            foreign_detected: true,
            autostart: Step::LeftForeign,
            crash_recovery: Step::LeftForeign,
            trust: Step::LeftForeign,
        },
        Ownership::Ours => {
            // Remove the autostart entry and disarm crash recovery. Kept as SEPARATE calls (rather than
            // `set_enabled_at(None, false)`, which bundles both under one lock) so each reports its own
            // outcome — uninstall is terminal and single-threaded, so the lock is uncontended either way.
            let autostart = match crate::autostart::remove_entry() {
                Ok(()) => Step::Removed,
                Err(e) => Step::Failed(e),
            };
            // Crash recovery MUST die here: its task/unit relaunches the app ~1 min after it quits, which
            // would resurrect the app in the middle of an uninstall (recon collision #2).
            let crash_recovery = if crate::crash_recovery::disable_crash_recovery() {
                Step::Removed
            } else {
                Step::Failed("could not confirm crash recovery was disabled".into())
            };
            let trust = remove_trust_and_certs();
            UninstallOutcome {
                foreign_detected: false,
                autostart,
                crash_recovery,
                trust,
            }
        }
    }
}

/// Remove the CA from every browser trust store AND delete the generated cert material. Trust removal is
/// the authoritative signal — it ASKS the stores again rather than trusting a delete's own word (see
/// [`crate::trust::remove_ca_trust`]); the certs dir is best-effort cleanup once trust is confirmed gone.
fn remove_trust_and_certs() -> Step {
    let report = crate::trust::remove_ca_trust(&crate::certs::live_ca_cert_path());
    if report.removal_incomplete() {
        let detail = report
            .removal_failure_detail()
            .unwrap_or_else(|| "a trust store still trusts the local CA".into());
        return Step::Failed(detail);
    }
    let certs = crate::certs::certs_dir();
    match std::fs::remove_dir_all(&certs) {
        Ok(()) => Step::Removed,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Step::Removed,
        Err(e) => Step::Failed(format!("could not remove {}: {e}", certs.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // The ownership decision table (D10 taxonomy). Mutation proof: flip any arm below and a case fails.
    #[test]
    fn foreign_or_unreadable_is_not_ours_everything_else_is() {
        // FOREIGN: entry resolves to a different binary → a second install still wants it.
        assert_eq!(
            classify_ownership(&StoredTarget::Healthy {
                program: PathBuf::from("/opt/other/AztecAccelerator"),
                points_elsewhere: true,
            }),
            Ownership::Foreign,
        );
        // FOREIGN: we cannot read/parse the entry → fail-closed (never nuke shared state on a guess).
        assert_eq!(
            classify_ownership(&StoredTarget::Unreadable {
                reason: "io error".into(),
            }),
            Ownership::Foreign,
        );
        // OURS: entry resolves to us.
        assert_eq!(
            classify_ownership(&StoredTarget::Healthy {
                program: PathBuf::from("/opt/us/AztecAccelerator"),
                points_elsewhere: false,
            }),
            Ownership::Ours,
        );
        // OURS: a broken entry was ours (its target is gone) — safe to clear.
        assert_eq!(
            classify_ownership(&StoredTarget::Broken {
                program: "/gone/AztecAccelerator".into(),
            }),
            Ownership::Ours,
        );
        // OURS: nothing stored at all.
        assert_eq!(classify_ownership(&StoredTarget::Absent), Ownership::Ours);
    }

    // A deliberate foreign skip must NOT be reported as a failure (it must exit 0).
    #[test]
    fn foreign_skips_are_not_failures_but_real_failures_are() {
        let foreign = UninstallOutcome {
            foreign_detected: true,
            autostart: Step::LeftForeign,
            crash_recovery: Step::LeftForeign,
            trust: Step::LeftForeign,
        };
        assert!(!foreign.incomplete(), "a foreign skip must exit 0");

        let failed = UninstallOutcome {
            foreign_detected: false,
            autostart: Step::Removed,
            crash_recovery: Step::Removed,
            trust: Step::Failed("still trusted".into()),
        };
        assert!(failed.incomplete(), "a failed removal must exit non-zero");
    }
}
