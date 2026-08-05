//! Cross-platform browser-trust-store management for the local CA anchor.
//!
//! `certs.rs` owns cert generation, paths, and the rustls config; this module owns ONLY the OS trust
//! mechanism. It never generates or reads a private key — every entry point takes an already-written
//! CA *cert* path that `certs.rs` derives.
//!
//! Backends:
//! - **macOS** ([`macos`]): the login Keychain via the absolute-path `security` CLI.
//! - **Linux** ([`linux`]): user-level NSS databases (`~/.pki/nssdb` + each Firefox profile) via
//!   `certutil` — NO root, honest per-store reporting when `certutil` is absent.
//! - **Windows** ([`windows`]): the CurrentUser `Root` store via the absolute-path `certutil.exe`;
//!   live-cert identity by serial (CN reserved for delete-all).
//!
//! Security posture (see plan §8): all shell-outs use `Command` + fixed argv (no shell string);
//! external binaries are resolved to safe absolute paths (a planted `certutil` in a writable PATH
//! dir is rejected). Name constraints on the anchor are defense-in-depth; the load-bearing control
//! is the keyless CA (it can sign nothing), so a trusted anchor in any store is harmless.

use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as imp;

// Any other target (none shipped) falls back to the not-supported stub.
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod stub;
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
use stub as imp;

/// One trust store's install/query result. Surfaced honestly in the UI — a store we could not write
/// (e.g. `certutil` missing, a sandboxed browser we can't reach) is *reported*, not hidden.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StoreStatus {
    /// Human-facing store name (e.g. "macOS Keychain", "System NSS", "Firefox (default)").
    pub store: String,
    /// Whether the anchor is installed / trusted in this store.
    pub installed: bool,
    /// Optional explanation when not installed (e.g. "certutil not found — install libnss3-tools").
    pub detail: Option<String>,
}

impl StoreStatus {
    pub(crate) fn ok(store: impl Into<String>) -> Self {
        Self {
            store: store.into(),
            installed: true,
            detail: None,
        }
    }
    pub(crate) fn fail(store: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            store: store.into(),
            installed: false,
            detail: Some(detail.into()),
        }
    }
}

/// The per-store outcome of an install / removal / status query across all discoverable stores.
#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct TrustReport {
    pub stores: Vec<StoreStatus>,
}

impl TrustReport {
    /// True iff the anchor is installed in at least one store — the "HTTPS is usable in some browser"
    /// signal the wizard keys on (a partially-trusted loopback listener is harmless; see plan R3).
    pub fn any_installed(&self) -> bool {
        self.stores.iter().any(|s| s.installed)
    }

    /// For a REMOVE report: whether any store still reports the anchor trusted OR removal couldn't be
    /// confirmed (`installed: true` in a remove report means "still there / unconfirmed"). Callers use
    /// it to FAIL the Settings/CLI removal instead of reporting a clean removal that didn't happen
    /// (post-impl codex High — the old paths always returned success).
    pub fn removal_incomplete(&self) -> bool {
        self.stores.iter().any(|s| s.installed)
    }

    /// A human-readable reason a removal was incomplete (the first still-trusted store's detail), for
    /// surfacing to the Settings UI / CLI. `None` when removal is complete.
    pub fn removal_failure_detail(&self) -> Option<String> {
        self.stores.iter().find(|s| s.installed).map(|s| {
            s.detail
                .clone()
                .unwrap_or_else(|| format!("{} still trusts the certificate", s.store))
        })
    }
}

/// Opaque reference to a previously-installed anchor, captured BEFORE a rotation swap so the OLD
/// anchor can be removed AFTER the NEW one is trusted (D4). Per-OS payload: macOS SHA-1 / Linux
/// nickname. `None` = nothing to remove.
pub struct AnchorRef(pub(crate) Option<String>);

/// What a "is our anchor in this store?" query could actually establish.
///
/// F-05 (audit 2026-07-31): both the macOS and Windows backends collapsed this into a bool, and both
/// collapsed it the UNSAFE way — `security find-certificate`'s spawn error became `None` via `.ok()?`,
/// and `certutil -store`'s became `false` via `.unwrap_or(false)`. On the REMOVAL path that reads as
/// "the anchor is gone", so Settings reported a clean removal, `--remove-ca-trust` exited 0, and the
/// root stayed trusted — on exactly the machines where it matters (a policy-blocked `security`, a
/// certutil.exe blocked by EDR/AppLocker, a locked or non-default login keychain). `Unknown` is what
/// makes [`TrustReport::removal_incomplete`] reachable in those cases.
///
/// Lives here rather than in either backend so it is compiled and unit-tested on every platform —
/// the macOS and Windows sources are `cfg`-gated to their own OS, so bugs in them are invisible to
/// the Linux CI leg that runs the unit tests.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Probe {
    Present,
    Absent,
    Unknown,
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
impl Probe {
    /// Classify a store query from its `Command::output()` result.
    ///
    /// `absent_marker` is how the tool says "no such certificate" when that is NOT an error:
    /// - `None` — a non-zero exit means absent (macOS `security find-certificate`, which has no
    ///   other documented failure mode we could tell apart, and for which non-zero is the normal
    ///   post-delete result).
    /// - `Some(marker)` — a non-zero exit is absent only if the output carries `marker`; anything
    ///   else is `Unknown` (Windows `certutil`, whose `CRYPT_E_NOT_FOUND` HRESULT prints
    ///   locale-independently and so distinguishes "nothing to delete" from "could not read").
    pub(crate) fn from_query(
        out: &std::io::Result<std::process::Output>,
        absent_marker: Option<&str>,
    ) -> Self {
        match out {
            // Could not run the tool at all: the store was never inspected.
            Err(_) => Self::Unknown,
            Ok(o) if o.status.success() => Self::Present,
            Ok(o) => match absent_marker {
                None => Self::Absent,
                Some(marker) => {
                    let text = format!(
                        "{}{}",
                        String::from_utf8_lossy(&o.stdout),
                        String::from_utf8_lossy(&o.stderr)
                    );
                    if text.contains(marker) {
                        Self::Absent
                    } else {
                        Self::Unknown
                    }
                }
            },
        }
    }
}

// ── Public dispatch API (certs.rs + commands.rs) ──

/// Install `ca_cert` as a trusted root in every discoverable browser store. Best-effort per store;
/// the report says which succeeded (Linux/wizard treat ≥1 installed as success).
pub fn install_ca_trust(ca_cert: &Path) -> TrustReport {
    imp::install(ca_cert)
}

/// Remove the anchor from every store (Settings "Remove certificate trust" + the uninstall CLI).
pub fn remove_ca_trust(ca_cert: &Path) -> TrustReport {
    imp::remove(ca_cert)
}

/// Per-store trust status for `ca_cert` (drives the honest Settings/wizard status list).
pub fn trust_status(ca_cert: &Path) -> TrustReport {
    imp::status(ca_cert)
}

/// Whether the anchor is trusted in at least one store (launch gate on macOS/Windows; UI on Linux).
pub fn is_ca_trusted(ca_cert: &Path) -> bool {
    imp::status(ca_cert).any_installed()
}

// ── Rotation hooks (certs::rotate) ──

/// Capture the currently-installed anchor's identity (from `live_ca`) for post-swap removal.
pub fn current_anchor(live_ca: &Path) -> AnchorRef {
    imp::current_anchor(live_ca)
}

/// Trust + verify a freshly-staged anchor BEFORE it replaces the live one. `Err` ⇒ the caller keeps
/// the old set (fail-closed — no outage, never an untrusted cert served).
pub fn trust_new_anchor(staged_ca: &Path) -> Result<(), String> {
    imp::trust_new_anchor(staged_ca)
}

/// Remove a previously-captured old anchor after the swap (best-effort).
pub fn remove_anchor(old: AnchorRef) {
    imp::remove_anchor(old)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{ExitStatus, Output};

    /// The Windows backend's marker; also used here to exercise the `Some(marker)` mode generically.
    const NOT_FOUND: &str = "0x80092004";

    fn exit(code: i32) -> ExitStatus {
        #[cfg(unix)]
        return {
            use std::os::unix::process::ExitStatusExt;
            // A unix wait-status: exit code N occupies the high byte, so 0 is success.
            ExitStatus::from_raw(code << 8)
        };
        #[cfg(windows)]
        return {
            use std::os::windows::process::ExitStatusExt;
            ExitStatus::from_raw(code as u32)
        };
    }

    fn ran(code: i32, text: &str) -> std::io::Result<Output> {
        Ok(Output {
            status: exit(code),
            stdout: text.as_bytes().to_vec(),
            stderr: Vec::new(),
        })
    }

    fn could_not_run() -> std::io::Result<Output> {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "blocked",
        ))
    }

    // ── F-05 regression (audit 2026-07-31) ────────────────────────────────────────────────────────
    // The defect was one collapsed state, in both OS backends, in the same unsafe direction: a tool
    // that could not RUN was classified as "the anchor is not there". On the removal path that is a
    // clean-removal report for a removal that never happened. These fail against the pre-fix code
    // (`.ok()?` on macOS, `.unwrap_or(false)` on Windows).

    /// THE bug. Both backends' spawn-failure paths, in both marker modes.
    #[test]
    fn a_tool_that_cannot_run_is_unknown_never_absent() {
        assert_eq!(Probe::from_query(&could_not_run(), None), Probe::Unknown);
        assert_eq!(
            Probe::from_query(&could_not_run(), Some(NOT_FOUND)),
            Probe::Unknown
        );
    }

    /// …and `Unknown` has to reach the caller as "still trusted", or nothing above changes: the CLI
    /// exits 0 and Settings claims success. This is the contract `main.rs`/`commands.rs` act on.
    #[test]
    fn an_unverifiable_removal_is_reported_incomplete() {
        let unknown = TrustReport {
            stores: vec![StoreStatus {
                store: "macOS Keychain".into(),
                installed: true,
                detail: Some("could not verify removal — `security` could not query it".into()),
            }],
        };
        assert!(unknown.removal_incomplete());
        assert!(unknown
            .removal_failure_detail()
            .unwrap()
            .contains("could not verify"));

        let gone = TrustReport {
            stores: vec![StoreStatus {
                store: "macOS Keychain".into(),
                installed: false,
                detail: None,
            }],
        };
        assert!(!gone.removal_incomplete());
        assert!(gone.removal_failure_detail().is_none());
    }

    /// macOS mode: `security find-certificate` has no distinguishable not-found signal, and non-zero
    /// is its NORMAL post-delete result — so non-zero must be `Absent`, or every successful removal
    /// would report failure.
    #[test]
    fn without_a_marker_a_non_zero_exit_means_absent() {
        assert_eq!(Probe::from_query(&ran(1, ""), None), Probe::Absent);
        assert_eq!(
            Probe::from_query(&ran(0, "SHA-1 hash: AB"), None),
            Probe::Present
        );
    }

    /// Windows mode: `CRYPT_E_NOT_FOUND` is the honest "nothing to delete"; any OTHER failure means
    /// the store could not be read and must not pass as removed.
    #[test]
    fn with_a_marker_only_the_marker_means_absent() {
        assert_eq!(
            Probe::from_query(
                &ran(
                    1,
                    "CertUtil: -store command FAILED: 0x80092004 (CRYPT_E_NOT_FOUND)"
                ),
                Some(NOT_FOUND)
            ),
            Probe::Absent
        );
        assert_eq!(
            Probe::from_query(&ran(1, "CertUtil: access denied"), Some(NOT_FOUND)),
            Probe::Unknown
        );
        assert_eq!(
            Probe::from_query(&ran(0, "Aztec Accelerator Local CA"), Some(NOT_FOUND)),
            Probe::Present
        );
    }

    /// The marker is read from stderr too — certutil is not consistent about which stream it uses.
    #[test]
    fn the_marker_is_recognised_on_stderr() {
        let on_stderr = Ok(Output {
            status: exit(1),
            stdout: Vec::new(),
            stderr: b"FAILED: 0x80092004 (CRYPT_E_NOT_FOUND)".to_vec(),
        });
        assert_eq!(
            Probe::from_query(&on_stderr, Some(NOT_FOUND)),
            Probe::Absent
        );
    }
}
