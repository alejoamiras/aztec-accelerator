//! B5: `--prepare-uninstall` — the ownership-checked teardown the NSIS uninstaller (and the
//! manual/scripted wrappers on other OSes) run BEFORE the app is deleted, while the exe is still present.
//!
//! The hazard this guards against is a SECOND install — a copied instance (#429) — that shares this
//! user's `~/.aztec-accelerator` state. Uninstalling THIS copy must not strip the OTHER copy's autostart
//! entry, kill its crash-recovery, or delete the CA trust + certs it still relies on.
//!
//! The stored autostart entry is the ownership oracle, read STRICTLY (codex): destructive removal of
//! SHARED state (trust/certs) has a higher proof burden than the arm path, so only a healthy entry that
//! resolves to THIS binary ([`Ownership::ConfirmedOurs`]) — or the total absence of any entry
//! ([`Ownership::NoEntry`]) — permits it. A healthy entry pointing elsewhere, a broken one (which may be
//! another copy's entry on a removable volume), or an unreadable one is [`Ownership::ForeignOrUncertain`]:
//! we LEAVE everything shared and report why. If we cannot even resolve our OWN identity we fail closed the
//! same way — never delete shared state on a guess.
//!
//! Documented residual (codex): a second install that never armed autostart is invisible to this oracle, so
//! an `Absent`-entry uninstall still removes the shared CA trust that copy relied on. A per-install registry
//! would be the complete fix; until then such a copy must re-enable HTTPS.
//!
//! Every primitive is idempotent, so the NSIS `POSTUNINSTALL` belt re-running after this (or a retried
//! uninstall) is safe.

use crate::autostart::StoredTarget;

/// Ownership of the shared state, decided STRICTLY from the stored autostart entry (codex: three states,
/// not a two-way mirror of the arm-path policy — arming and destructive cleanup have different proof
/// burdens).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ownership {
    /// A healthy entry that resolves to THIS binary — proven ours: remove the entry AND shared state.
    ConfirmedOurs,
    /// No stored entry at all: nothing to remove, and no *evidence* of another install — shared state is
    /// ours to clear (subject to the documented never-armed residual).
    NoEntry,
    /// A healthy entry pointing elsewhere, an unresolved/broken entry (possibly another copy on a removable
    /// volume), or an unreadable one — cannot prove ownership, so LEAVE the entry and ALL shared state.
    ForeignOrUncertain,
}

fn classify(stored: &StoredTarget) -> Ownership {
    match stored {
        StoredTarget::Healthy {
            points_elsewhere: false,
            ..
        } => Ownership::ConfirmedOurs,
        StoredTarget::Absent => Ownership::NoEntry,
        // Elsewhere ⇒ another install. Broken ⇒ maybe another copy's stale/removable-volume entry — NOT a
        // proof it was ours. Unreadable ⇒ fail-closed. All three: leave shared state intact.
        StoredTarget::Healthy {
            points_elsewhere: true,
            ..
        }
        | StoredTarget::Broken { .. }
        | StoredTarget::Unreadable { .. } => Ownership::ForeignOrUncertain,
    }
}

/// The disposition of one artifact after [`prepare_uninstall`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Removed successfully.
    Removed,
    /// Nothing to remove (no such artifact) — not a failure.
    NotPresent,
    /// Deliberately LEFT because another install owns it / relies on it — not a failure.
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
            Step::Removed => "removed".into(),
            Step::NotPresent => "absent (nothing to remove)".into(),
            Step::LeftForeign => "left (another install owns it)".into(),
            Step::Failed(why) => format!("FAILED — {why}"),
        }
    }
}

/// The result of [`prepare_uninstall`]: the ownership verdict plus one [`Step`] per artifact.
#[derive(Debug, Clone)]
pub struct UninstallOutcome {
    /// True when another install (or an unreadable/uncertain entry, or an unresolvable self-identity) was
    /// detected and ALL shared state was deliberately left.
    pub foreign_detected: bool,
    /// One-line reason, present when `foreign_detected`.
    pub reason: Option<String>,
    pub autostart: Step,
    pub crash_recovery: Step,
    pub trust: Step,
}

impl UninstallOutcome {
    /// Leave EVERYTHING — the fail-safe outcome when another install is present or ownership can't be
    /// proven. Never `incomplete()` (a deliberate leave is success, exit 0).
    fn left_all(reason: String) -> Self {
        UninstallOutcome {
            foreign_detected: true,
            reason: Some(reason),
            autostart: Step::LeftForeign,
            crash_recovery: Step::LeftForeign,
            trust: Step::LeftForeign,
        }
    }

    /// Non-zero-exit trigger: any ATTEMPTED removal that failed. A deliberate `LeftForeign`/`NotPresent` is
    /// NOT a failure — a foreign skip must exit 0 so the NSIS/scripted caller does not treat it as an error.
    pub fn incomplete(&self) -> bool {
        self.autostart.failed() || self.crash_recovery.failed() || self.trust.failed()
    }

    /// Human-readable per-artifact lines for the CLI (mirrors `--remove-ca-trust`'s per-store output).
    pub fn report_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(reason) = &self.reason {
            lines.push(format!("ownership: LEFT shared state — {reason}"));
        }
        lines.push(format!("autostart entry:  {}", self.autostart.label()));
        lines.push(format!("crash recovery:   {}", self.crash_recovery.label()));
        lines.push(format!("CA trust + certs: {}", self.trust.label()));
        lines
    }
}

/// Ownership-checked teardown. See the module docs. Idempotent.
pub fn prepare_uninstall() -> UninstallOutcome {
    // Fail CLOSED if we cannot compute our own identity: `read_stored_target(None)` would read every
    // healthy entry as `points_elsewhere:false` (ours) and we could delete another install's shared state.
    let reference = match crate::autostart::owned_reference_path() {
        Ok(r) => r,
        Err(e) => {
            return UninstallOutcome::left_all(format!(
                "cannot resolve this install's identity: {e}"
            ))
        }
    };
    let stored = crate::autostart::read_stored_target(Some(&reference));
    match classify(&stored) {
        Ownership::ForeignOrUncertain => UninstallOutcome::left_all(
            "the autostart entry belongs to another install, or is unreadable".into(),
        ),
        Ownership::ConfirmedOurs => remove_ours(true),
        Ownership::NoEntry => remove_ours(false),
    }
}

/// Remove the artifacts THIS install owns. `remove_autostart_entry` is false for [`Ownership::NoEntry`]
/// (there is no entry). Crash-recovery is gated on the same ownership verdict: `set_enabled` always arms
/// the autostart entry and the recovery task/unit together, so a ConfirmedOurs/NoEntry verdict means the
/// recovery task is ours too. (The exe-gone NSIS belt independently re-checks the task's own `<Command>`.)
fn remove_ours(remove_autostart_entry: bool) -> UninstallOutcome {
    let autostart = if remove_autostart_entry {
        match crate::autostart::remove_entry() {
            Ok(()) => Step::Removed,
            Err(e) => Step::Failed(e),
        }
    } else {
        Step::NotPresent
    };
    // Crash recovery MUST die: its task/unit relaunches the app ~1 min after it quits, which would
    // resurrect the app in the middle of an uninstall (recon collision #2).
    let crash_recovery = if crate::crash_recovery::disable_crash_recovery() {
        Step::Removed
    } else {
        Step::Failed("could not confirm crash recovery was disabled".into())
    };
    let trust = remove_trust_and_certs();
    UninstallOutcome {
        foreign_detected: false,
        reason: None,
        autostart,
        crash_recovery,
        trust,
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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Step::NotPresent,
        Err(e) => Step::Failed(format!("could not remove {}: {e}", certs.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // The three-state ownership table (codex). Mutation proof: collapse any FOREIGN/UNCERTAIN arm into an
    // "Ours"-shaped verdict and a case below fails.
    #[test]
    fn only_a_healthy_self_entry_or_no_entry_permits_shared_removal() {
        assert_eq!(
            classify(&StoredTarget::Healthy {
                program: PathBuf::from("/opt/us/AztecAccelerator"),
                points_elsewhere: false,
            }),
            Ownership::ConfirmedOurs,
        );
        assert_eq!(classify(&StoredTarget::Absent), Ownership::NoEntry);
        // A healthy entry elsewhere is another install.
        assert_eq!(
            classify(&StoredTarget::Healthy {
                program: PathBuf::from("/opt/other/AztecAccelerator"),
                points_elsewhere: true,
            }),
            Ownership::ForeignOrUncertain,
        );
        // Broken does NOT prove it was ours (could be another copy on an unplugged volume).
        assert_eq!(
            classify(&StoredTarget::Broken {
                program: "/mnt/usb/AztecAccelerator".into(),
            }),
            Ownership::ForeignOrUncertain,
        );
        // Unreadable ⇒ fail-closed.
        assert_eq!(
            classify(&StoredTarget::Unreadable {
                reason: "io error".into(),
            }),
            Ownership::ForeignOrUncertain,
        );
    }

    #[test]
    fn foreign_and_notpresent_skips_are_not_failures_but_real_failures_are() {
        assert!(!UninstallOutcome::left_all("another install".into()).incomplete());

        let no_entry_ok = UninstallOutcome {
            foreign_detected: false,
            reason: None,
            autostart: Step::NotPresent,
            crash_recovery: Step::Removed,
            trust: Step::Removed,
        };
        assert!(!no_entry_ok.incomplete(), "NotPresent is not a failure");

        let failed = UninstallOutcome {
            foreign_detected: false,
            reason: None,
            autostart: Step::Removed,
            crash_recovery: Step::Removed,
            trust: Step::Failed("still trusted".into()),
        };
        assert!(failed.incomplete(), "a failed removal must exit non-zero");
    }
}
