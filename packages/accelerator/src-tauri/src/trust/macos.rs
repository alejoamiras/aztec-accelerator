//! macOS trust backend — the login Keychain via the `security` CLI (absolute path). Moved verbatim
//! (behavior-preserving) from `certs.rs`, re-shaped to take an explicit CA cert path and to report
//! through [`TrustReport`]. `security add-trusted-cert` raises the native password dialog — that is
//! the consent ceremony for enabling HTTPS on macOS.

use super::{AnchorRef, Probe, StoreStatus, TrustReport};
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
///
/// **No `-p` policy scoping, deliberately** (F-02, audit 2026-07-31). Narrowing the grant to
/// `-p ssl` would also require `verify-cert -p ssl`, and the SSL policy matches a hostname against
/// the certificate's SANs — our anchor is a CA with none, so the launch gate would false-negative and
/// HTTPS would never come up on macOS. That is exactly the class of break the `-l` flag above
/// documents (post-impl codex Critical), and NOTHING in CI could catch a repeat: installing a root
/// needs interactive auth, so `tests/trust_macos.rs` covers only the query path.
///
/// The trade is acceptable because the grant's blast radius is set by the certificate, not the
/// policy list: this anchor is KEYLESS (its private key was generated per-signature and never
/// written anywhere) and name-constrained to loopback, so a broader policy grant on it still vouches
/// for nothing. `certs::validate_ca_profile` is what keeps both of those properties true — it refuses
/// to let any other certificate reach this function.
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

/// The anchor currently in the login keychain — a [`Probe`] plus the SHA-1 needed to delete it. See
/// [`Probe`] for why "could not ask" must not collapse into "absent" (F-05).
enum Anchor {
    Found(String),
    Absent,
    Unknown,
}

/// SHA-1 of the currently-installed "Aztec Accelerator Local CA" anchor — captured before rotation so
/// the OLD anchor can be removed after the NEW one is installed. Returns the first match.
///
/// A spawn failure — `security` blocked by policy, a locked or non-default login keychain, a different
/// `HOME` under `sudo` — is `Unknown`, never `Absent`; pre-F-05 the `.ok()?` here turned every one of
/// those into "no anchor to delete", which the removal path then reported as a clean removal. A
/// A NON-ZERO exit is `Absent` *provisionally* (`absent_marker: None`): for `find-certificate` that
/// overwhelmingly means "no such item", and it is the normal post-delete result, so treating it as
/// `Unknown` outright would make every successful removal report failure instead. The control question
/// below then separates that from an unreadable keychain, which exits nonzero for the same reason.
fn keychain_anchor() -> Anchor {
    let out = Command::new(SECURITY_BIN)
        .args(["find-certificate", "-Z", "-c", "Aztec Accelerator Local CA"])
        .arg(login_keychain())
        .output();
    // Bound the borrow before the match: a `&out` temporary in a match SCRUTINEE lives until the end
    // of the match, which would forbid reading `out` in an arm.
    let mut probe = Probe::from_query(&out, None);
    if probe == Probe::Unknown {
        tracing::warn!("could not run security find-certificate; keychain state unknown");
    }
    // Round 2 (codex pass over F-05): `absent_marker: None` maps EVERY nonzero exit to `Absent`, and
    // `security` exits nonzero both for "no such item" (normal, and the expected post-delete result)
    // and for "could not read this keychain" (locked, permission-denied, wrong `HOME` under `sudo`) —
    // so the second case still reported a clean removal. There is no locale-independent exit code
    // that separates them, so ask a CONTROL question instead: can we read the keychain AT ALL? The
    // same shape as the NSIS hook's re-query, and it costs one extra process only on the miss path.
    if probe == Probe::Absent && !keychain_is_readable() {
        tracing::warn!("the login keychain could not be read; its contents are unknown");
        probe = Probe::Unknown;
    }
    let stdout = match out {
        Ok(o) => o.stdout,
        Err(_) => Vec::new(),
    };
    match probe {
        Probe::Absent => Anchor::Absent,
        Probe::Unknown => Anchor::Unknown,
        // It found something; if we cannot read the hash back we cannot delete it either.
        Probe::Present => match String::from_utf8_lossy(&stdout)
            .lines()
            .find_map(|l| l.trim().strip_prefix("SHA-1 hash:"))
            .map(|h| h.trim().to_string())
        {
            Some(sha1) => Anchor::Found(sha1),
            None => {
                tracing::warn!("security find-certificate succeeded but no SHA-1 could be parsed");
                Anchor::Unknown
            }
        },
    }
}

/// Can `security` actually QUERY the login keychain? Both signals must agree.
///
/// Neither alone is sufficient, and I shipped each alone in turn:
/// - An unfiltered `find-certificate` also exits nonzero on a keychain holding NO certificates, so
///   deleting our anchor when it was the last one reported a successful removal as unverifiable.
/// - `File::open` succeeds on a keychain that is filesystem-readable but LOCKED, corrupt, or denied
///   by the keychain service — precisely the states where `security` runs, exits nonzero, and the
///   removal would again be reported as complete without proof (codex rounds 2 and 4).
///
/// Requiring both means the ambiguity that remains is one-directional: a genuinely EMPTY keychain
/// reports "could not verify" for a removal that had nothing to remove. That is the fail-closed
/// direction and it costs a user a manual re-check; the alternative costs them a root CA they were
/// told was gone. It cannot be narrowed further from code we can test — `security`'s exit codes do
/// not separate "no such item" from "cannot read this store", its stderr strings are localisable,
/// and installing/locking a keychain in CI needs interactive auth (`tests/trust_macos.rs` covers
/// only the query path).
///
/// Lead worth following on a real Mac, not taken here: `security show-keychain-info <keychain>`
/// reads keychain SETTINGS (`SecKeychainCopySettings`) independently of item count, and is reported
/// to fail when keychain authentication is broken. If it can be shown, across supported macOS
/// versions, to succeed on an empty UNLOCKED keychain and fail WITHOUT prompting on
/// locked/corrupt/service-denied ones, it would replace both checks here and remove the empty-
/// keychain false positive. It needs that matrix first — settings readability does not obviously
/// imply certificate-search readability, and a check that prompts would be worse than the ambiguity
/// it replaces. (codex round 5.)
fn keychain_is_readable() -> bool {
    if std::fs::File::open(login_keychain()).is_err() {
        return false;
    }
    Command::new(SECURITY_BIN)
        .arg("find-certificate")
        .arg(login_keychain())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Convenience for the callers that only need "the current anchor, if we can name it".
fn keychain_sha1() -> Option<String> {
    match keychain_anchor() {
        Anchor::Found(s) => Some(s),
        _ => None,
    }
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
    let mut unknown = false;
    for _ in 0..64 {
        match keychain_anchor() {
            Anchor::Found(sha1) => {
                if !delete_by_sha1(&sha1) {
                    delete_failed = true;
                    break;
                }
            }
            Anchor::Absent => break,
            // F-05: we could not even ask. Previously this broke out of the loop with
            // `delete_failed == false` and the post-check then read "absent", so the caller reported a
            // clean removal without a single delete having been attempted.
            Anchor::Unknown => {
                unknown = true;
                break;
            }
        }
    }
    // `installed` = still present OR not provably gone. The caller fails the Settings/CLI action on
    // either, so "could not determine" can never render as "removed".
    let post = keychain_anchor();
    let remaining = matches!(post, Anchor::Found(_));
    let post_unknown = unknown || matches!(post, Anchor::Unknown);
    let detail = if remaining {
        Some("a CA anchor is still trusted after the removal attempt".to_string())
    } else if post_unknown {
        Some(
            "could not verify removal — `security` could not query the login keychain, so the CA may \
             still be trusted"
                .to_string(),
        )
    } else if delete_failed {
        Some("delete-certificate reported a failure".to_string())
    } else {
        None
    };
    TrustReport {
        stores: vec![StoreStatus {
            store: STORE.into(),
            installed: remaining || post_unknown || delete_failed,
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
