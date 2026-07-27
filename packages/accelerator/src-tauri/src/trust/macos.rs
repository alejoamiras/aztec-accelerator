//! macOS trust backend — the login Keychain via the `security` CLI (absolute path). Moved verbatim
//! (behavior-preserving) from `certs.rs`, re-shaped to take an explicit CA cert path and to report
//! through [`TrustReport`]. `security add-trusted-cert` raises the native password dialog — that is
//! the consent ceremony for enabling HTTPS on macOS.

use super::{AnchorRef, StoreStatus, TrustReport};
use std::path::{Path, PathBuf};
use std::process::Command;

const STORE: &str = "macOS Keychain";

/// Absolute path to `security` — never a bare-name PATH lookup (a planted `security` earlier on PATH
/// can't win; same defense as the absolute System32 tools elsewhere).
const SECURITY_BIN: &str = "/usr/bin/security";

fn login_keychain() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library/Keychains/login.keychain-db")
}

/// Add a cert as a trusted root in the login Keychain (prompts the user). Trust is keyed to the
/// cert's content, so a later atomic rename of the file does not invalidate it.
fn add_trusted_cert(cert_path: &Path) -> Result<(), String> {
    let output = Command::new(SECURITY_BIN)
        .args(["add-trusted-cert", "-r", "trustRoot", "-k"])
        .arg(login_keychain())
        .arg(cert_path)
        .output()
        .map_err(|e| format!("could not run security: {e}"))?;
    if output.status.success() {
        tracing::info!("CA certificate installed in macOS login Keychain");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        tracing::error!(%stderr, "Failed to install CA trust");
        Err(format!("security add-trusted-cert failed: {stderr}"))
    }
}

/// Whether the given cert verifies as trusted.
///
/// `-l` is REQUIRED: our anchor is a CA certificate (BasicConstraints CA=true), and `verify-cert`
/// verifies its `-c` argument AS A LEAF — which by default rejects a leaf that is itself a CA. Without
/// `-l` ("the leaf may be a CA"), `verify-cert -c ca.pem` returns non-zero even for a fully-trusted
/// anchor, so `is_ca_trusted` would false-negative and the launch gate would never bring HTTPS up on
/// macOS (post-impl codex Critical). `-L` keeps the check offline (no OCSP/CRL network fetch that
/// could hang the launch path). Trust is content-keyed, so a later atomic file rename doesn't matter.
fn verify_cert_trusted(cert_path: &Path) -> bool {
    cert_path.exists()
        && Command::new(SECURITY_BIN)
            .args(["verify-cert", "-l", "-L", "-c"])
            .arg(cert_path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

/// SHA-1 of the currently-installed "Aztec Accelerator Local CA" anchor (if any) — captured before
/// rotation so the OLD anchor can be removed after the NEW one is installed. Returns the first match.
fn keychain_sha1() -> Option<String> {
    let output = Command::new(SECURITY_BIN)
        .args(["find-certificate", "-Z", "-c", "Aztec Accelerator Local CA"])
        .arg(login_keychain())
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|l| l.trim().strip_prefix("SHA-1 hash:"))
        .map(|h| h.trim().to_string())
}

/// Best-effort removal of a trusted cert by SHA-1. Returns whether the delete reported success.
///
/// `-t` ALSO removes the cert's user TRUST SETTINGS, not just the keychain item. Without it, the item
/// is deleted but its "always trust" setting lingers as an orphaned entry that `find-certificate` can
/// no longer see (so the remove loop stops early leaving trust behind) — post-impl codex High.
fn delete_by_sha1(sha1: &str) -> bool {
    match Command::new(SECURITY_BIN)
        .args(["delete-certificate", "-t", "-Z", sha1])
        .arg(login_keychain())
        .output()
    {
        Ok(o) if o.status.success() => {
            tracing::info!(sha1, "Removed CA anchor + trust settings");
            true
        }
        Ok(o) => {
            tracing::warn!(stderr = %String::from_utf8_lossy(&o.stderr), "Could not remove CA anchor (left in keychain)");
            false
        }
        Err(e) => {
            tracing::warn!(error = %e, "Could not run delete-certificate");
            false
        }
    }
}

pub fn install(ca_cert: &Path) -> TrustReport {
    let status = match add_trusted_cert(ca_cert) {
        Ok(()) if verify_cert_trusted(ca_cert) => StoreStatus::ok(STORE),
        Ok(()) => StoreStatus::fail(STORE, "installed but verify-cert did not confirm trust"),
        Err(e) => StoreStatus::fail(STORE, e),
    };
    TrustReport {
        stores: vec![status],
    }
}

pub fn status(ca_cert: &Path) -> TrustReport {
    let installed = verify_cert_trusted(ca_cert);
    TrustReport {
        stores: vec![StoreStatus {
            store: STORE.into(),
            installed,
            detail: None,
        }],
    }
}

pub fn remove(_ca_cert: &Path) -> TrustReport {
    // Remove ALL our anchors by CN — the live one AND any left by prior rotations (post-impl review).
    // Loop deleting the first CN match until none remain (bounded). `find` returns the first; each
    // delete removes exactly it. Stop on a failed delete so an undeletable anchor doesn't spin.
    let mut delete_failed = false;
    for _ in 0..64 {
        match keychain_sha1() {
            Some(sha1) => {
                if !delete_by_sha1(&sha1) {
                    delete_failed = true;
                    break;
                }
            }
            None => break,
        }
    }
    // `installed` = whether ANY of our CN anchors is still present (catches stale rotation anchors,
    // not just the live cert — codex Low: the old live-cert-only check reported success while a
    // same-CN anchor remained). The caller fails the Settings/CLI action when this stays true.
    let remaining = keychain_sha1().is_some();
    let detail = if remaining {
        Some("a CA anchor is still trusted after the removal attempt".to_string())
    } else if delete_failed {
        Some("delete-certificate reported a failure".to_string())
    } else {
        None
    };
    TrustReport {
        stores: vec![StoreStatus {
            store: STORE.into(),
            installed: remaining || delete_failed,
            detail,
        }],
    }
}

pub fn current_anchor(_live_ca: &Path) -> AnchorRef {
    AnchorRef(keychain_sha1())
}

pub fn trust_new_anchor(staged_ca: &Path) -> Result<(), String> {
    add_trusted_cert(staged_ca)?;
    if verify_cert_trusted(staged_ca) {
        Ok(())
    } else {
        Err("verify-cert failed for the newly-staged anchor".into())
    }
}

pub fn remove_anchor(old: AnchorRef) {
    if let Some(sha1) = old.0 {
        let _ = delete_by_sha1(&sha1);
    }
}
