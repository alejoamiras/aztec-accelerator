//! Windows trust backend — the CurrentUser `Root` store via `certutil.exe` (absolute System32 path).
//!
//! Design (plan D3/D4, codex/audit): `certutil.exe` is a system binary (zero new Rust deps; the
//! established shell-out-by-absolute-path pattern, cf. `crash_recovery::schtasks_exe`). Everything is
//! exit-code-driven (locale-independent), never stdout-scraping.
//!
//! Live-cert IDENTITY (`install` verify, `status`, `is_ca_trusted`, `trust_new_anchor`, rotation's
//! delete-old) is matched by **serial** (`-store`/`-delstore Root <serial>`, parsed from the PEM via
//! `x509-parser`) — CN alone can't tell the CURRENT leaf's anchor from a stale rotation anchor that
//! shares the CN, so a CN check could report "trusted" while the live leaf chains to an absent root
//! (post-impl codex High, both hunts). **CN** (`-delstore Root "Aztec Accelerator Local CA"`) is
//! reserved for the ONE place we want "match all of ours": remove/uninstall, which deletes the live
//! anchor plus every rotation leftover. The serial paths are exercised by the manual release runbook.
//!
//! Consent: `certutil -user -addstore Root` raises the Windows root-CA trust dialog (that IS the
//! user's consent), so the CI integration test seeds non-interactively via PowerShell
//! `Import-Certificate` and exercises verify/remove instead (P4 spike I3; see `tests/trust_windows.rs`).

use super::{AnchorRef, Probe, StoreStatus, TrustReport};
use std::path::{Path, PathBuf};
use std::process::Command;

const STORE: &str = "Windows CurrentUser Root";
const CA_CN: &str = "Aztec Accelerator Local CA";
/// certutil store names. `Disallowed` (the "Untrusted Certificates" store) OVERRIDES `Root` in Windows
/// chain building, so trust means "in Root AND not in Disallowed" (see [`live_present`]).
const ROOT_STORE: &str = "Root";
const DISALLOWED_STORE: &str = "Disallowed";

/// Absolute path to `certutil.exe` — never a bare-name PATH lookup (a planted `certutil` earlier on
/// PATH must not win). **Prefers the hardcoded `C:\Windows\System32\certutil.exe`** when it exists, so
/// a tainted `SystemRoot`/`windir` environment can't redirect this privileged trust operation on a
/// standard install (post-impl codex High). Only falls back to the env-derived path for the rare
/// non-standard Windows root where `C:\Windows` isn't it. (`GetSystemDirectoryW` would be the fully
/// robust API but needs a `windows-sys`/FFI dep — deliberately avoided per D3's zero-new-dep choice.)
fn certutil_exe() -> PathBuf {
    let hardcoded = PathBuf::from("C:\\Windows\\System32\\certutil.exe");
    if hardcoded.is_file() {
        return hardcoded;
    }
    let system_root = std::env::var("SystemRoot")
        .or_else(|_| std::env::var("windir"))
        .unwrap_or_else(|_| "C:\\Windows".to_string());
    Path::new(&system_root)
        .join("System32")
        .join("certutil.exe")
}

/// The cert's serial number as the hex string `certutil` uses to identify it (lowercase, no
/// separators). Parsed from the PEM via `x509-parser` (already a dep) — locale-proof, no stdout scrape.
fn cert_serial(ca_pem: &Path) -> Option<String> {
    let bytes = std::fs::read(ca_pem).ok()?;
    let (_, pem) = x509_parser::pem::parse_x509_pem(&bytes).ok()?;
    let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents).ok()?;
    // raw_serial_as_string() is colon-separated hex (e.g. "1a:2b:…"); certutil matches the compact form.
    Some(cert.raw_serial_as_string().replace(':', "").to_lowercase())
}

fn add_store(ca_cert: &Path) -> bool {
    Command::new(certutil_exe())
        .args(["-user", "-addstore", "Root"])
        .arg(ca_cert)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// certutil's HRESULT for "the requested object was not found". Kept as a FAST PATH only — see
/// [`presence_by_cn`] for why it cannot be the sole discriminator.
pub(super) const CRYPT_E_NOT_FOUND: &str = "0x80092004";

/// Is ANY anchor with our CN present? Used ONLY by the remove/uninstall path, which deletes every
/// same-CN anchor (live + rotation leftovers) — the one place CN's "match all of ours" is what we want.
///
/// Pre-F-05 this was `.map(|o| o.status.success()).unwrap_or(false)`: a `certutil.exe` that could not
/// execute at all (a commonly restricted LOLBIN — EDR/AppLocker) read as "not present", which the
/// removal path then reported as a clean removal.
///
/// The first F-05 fix then over-corrected by keying "absent" on `CRYPT_E_NOT_FOUND` appearing in the
/// output. **That assumption is false on a real runner** — a filtered `-store` miss exits non-zero
/// WITHOUT printing that code, so every ordinary removal (nothing of ours installed, or our anchor
/// just successfully deleted) classified as `Unknown` and reported "could not verify removal". A
/// control that fires on the happy path is worse than the fail-open it replaced, because it teaches
/// the user to ignore it. Caught by `tests/trust_windows.rs`, which only a Windows runner can run.
///
/// So the verdict no longer depends on a string certutil may not print. A filtered miss asks a
/// CONTROL question instead — can certutil read the store AT ALL? — exactly as the macOS backend and
/// the NSIS hook do. The marker stays as a fast path that skips the second process when it IS there.
fn presence_by_cn() -> Probe {
    let out = Command::new(certutil_exe())
        .args(["-user", "-store", "Root", CA_CN])
        .output();
    let probe = match Probe::from_query(&out, Some(CRYPT_E_NOT_FOUND)) {
        // Found it, or certutil said "no such object" in so many words.
        p @ (Probe::Present | Probe::Absent) => p,
        // Non-zero for some other reason, or it never ran: ask whether the store is readable.
        Probe::Unknown => {
            if root_store_is_readable() {
                Probe::Absent
            } else {
                Probe::Unknown
            }
        }
    };
    if probe == Probe::Unknown {
        tracing::warn!("certutil could not read the CurrentUser Root store; its state is unknown");
    }
    probe
}

/// Can certutil enumerate the CurrentUser `Root` store at all? An UNFILTERED `-store Root` succeeds
/// whenever the store is readable, which separates "our certificate is not in it" from "we could not
/// look" without depending on any particular error string or HRESULT.
///
/// Residual, same shape as the macOS control and stated for the same reason: if the CurrentUser Root
/// store were genuinely EMPTY this may exit non-zero and we would report "could not verify" for a
/// removal that had nothing to remove. That is the fail-closed direction, and it is far rarer than
/// the case this replaces — a filtered miss is universal, an empty user Root store is not — but it is
/// the same trade, not a closed one.
fn root_store_is_readable() -> bool {
    Command::new(certutil_exe())
        .args(["-user", "-store", "Root"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Is a cert with THIS exact serial present in `store`? Exit-code driven (locale-independent).
/// The LIVE-cert identity check uses the serial, not the CN: every stale rotation anchor shares the
/// CN, so a CN match could report "trusted" while the CURRENT leaf chains to an anchor that's actually
/// gone (post-impl codex High, both hunts). Uses the compact hex serial the rotation delete-by-serial
/// path already depends on.
fn is_present_by_serial(store: &str, serial: &str) -> bool {
    Command::new(certutil_exe())
        .args(["-user", "-store", store, serial])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Whether the cert AT `ca_cert` is EFFECTIVELY trusted for chain building on Windows.
///
/// Presence in `Root` is not sufficient: the `Disallowed` (Untrusted Certificates) store takes
/// PRECEDENCE, so a cert in both is rejected by the OS while a naive Root-only check would call it
/// trusted — the same "reports trusted when it isn't" class as the CN-vs-serial bug, and it would make
/// the launch gate serve a leaf browsers refuse (post-impl codex Medium). So: present in Root AND not
/// in Disallowed.
///
/// If the serial can't be parsed we return `false` (can't identify → treat as not-trusted): the launch
/// gate then skips HTTPS and the app serves plain HTTP, the safe-fail direction — never presenting a
/// cert we can't verify is trusted.
fn live_present(ca_cert: &Path) -> bool {
    let Some(serial) = cert_serial(ca_cert) else {
        tracing::warn!("could not parse CA serial; treating Windows trust as not-present");
        return false;
    };
    if is_present_by_serial(DISALLOWED_STORE, &serial) {
        tracing::warn!(
            "CA is in the CurrentUser Disallowed store — explicitly distrusted, treating as not-trusted"
        );
        return false;
    }
    is_present_by_serial(ROOT_STORE, &serial)
}

/// Delete the OLD anchor SPECIFICALLY, by serial — used only during rotation, where old + new share
/// the CN so delete-by-CN would nuke the new one too (D4). This is the one place the serial-string
/// format matters; it's exercised by the manual release runbook, not headless CI.
fn delete_by_serial(serial: &str) {
    let _ = Command::new(certutil_exe())
        .args(["-user", "-delstore", "Root", serial])
        .output();
}

/// Delete every anchor named `CA_CN` (uninstall / Settings "Remove trust"). No dialog on delete.
///
/// Returns whether the delete is known to have run cleanly. F-05: the result used to be discarded
/// entirely (`let _ = …`), so a certutil that could not execute was indistinguishable from a
/// successful deletion. "Nothing to delete" (`CRYPT_E_NOT_FOUND`) counts as clean.
fn delete_by_cn() -> bool {
    // Only "did certutil execute?" is decided here. Whether the anchor is GONE is decided by the
    // re-query in `remove`, which is the same reasoning the NSIS hook uses: a delete's own exit code
    // conflates "nothing to delete" with "could not delete", and on this tool it does so without a
    // reliable marker to tell them apart (see `presence_by_cn`).
    match Command::new(certutil_exe())
        .args(["-user", "-delstore", "Root", CA_CN])
        .output()
    {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!(error = %e, "could not run certutil -delstore");
            false
        }
    }
}

pub fn install(ca_cert: &Path) -> TrustReport {
    // Verify by the INSTALLED cert's serial — a stale same-CN anchor must not read as "the new one
    // landed" (post-impl codex High).
    let ok = add_store(ca_cert) && live_present(ca_cert);
    let status = if ok {
        StoreStatus::ok(STORE)
    } else {
        StoreStatus::fail(
            STORE,
            "certutil could not add the certificate to CurrentUser Root",
        )
    };
    TrustReport {
        stores: vec![status],
    }
}

pub fn status(ca_cert: &Path) -> TrustReport {
    TrustReport {
        stores: vec![StoreStatus {
            store: STORE.into(),
            // The LIVE cert's identity (by serial), so a stale same-CN anchor can't report it trusted.
            installed: live_present(ca_cert),
            detail: None,
        }],
    }
}

pub fn remove(_ca_cert: &Path) -> TrustReport {
    // Uninstall: delete ALL our anchors by CN (covers rotation leftovers too).
    let deleted_cleanly = delete_by_cn();
    // `installed` reports "still trusted OR not provably gone" — the caller fails the Settings/CLI
    // removal on either, so a certutil that could not run can no longer render as a clean removal.
    let (installed, detail) = match presence_by_cn() {
        Probe::Present => (
            true,
            Some("a CA anchor is still trusted after the removal attempt".to_string()),
        ),
        Probe::Unknown => (
            true,
            Some(
                "could not verify removal — certutil could not read the CurrentUser Root store, so \
                 the CA may still be trusted"
                    .to_string(),
            ),
        ),
        Probe::Absent if !deleted_cleanly => (
            true,
            Some("certutil -delstore could not be run".to_string()),
        ),
        Probe::Absent => (false, None),
    };
    TrustReport {
        stores: vec![StoreStatus {
            store: STORE.into(),
            installed,
            detail,
        }],
    }
}

pub fn current_anchor(live_ca: &Path) -> AnchorRef {
    // Capture the OLD serial before rotation so we can delete THIS anchor specifically after the swap
    // (delete-by-CN would also nuke the freshly-installed new anchor — D4).
    AnchorRef(cert_serial(live_ca))
}

pub fn trust_new_anchor(staged_ca: &Path) -> Result<(), String> {
    if !add_store(staged_ca) {
        return Err("certutil -addstore failed for the new anchor".into());
    }
    // Verify the STAGED cert specifically, by its serial (CN would match the still-present old anchor).
    if live_present(staged_ca) {
        Ok(())
    } else {
        Err("new anchor not present in CurrentUser Root after add (by serial)".into())
    }
}

pub fn remove_anchor(old: AnchorRef) {
    if let Some(serial) = old.0 {
        delete_by_serial(&serial);
    }
}
