//! Windows trust backend — the CurrentUser `Root` store via `certutil.exe` (absolute System32 path).
//!
//! Design (plan D3/D4, codex/audit): `certutil.exe` is a system binary (zero new Rust deps; the
//! established shell-out-by-absolute-path pattern, cf. `crash_recovery::schtasks_exe`). Everything is
//! exit-code-driven (locale-independent), never stdout-scraping.
//!
//! Presence/trust (`install` verify, `status`, `is_ca_trusted`, `remove`) is matched by **CN**
//! (`-store`/`-delstore Root "Aztec Accelerator Local CA"`) — the store holds no foreign cert with our
//! CN, so a CN match is unambiguous, and this keeps the common paths independent of certutil's exact
//! serial-string format. The **serial** (parsed from the PEM via `x509-parser`) is used ONLY where CN
//! is ambiguous: rotation's delete-the-OLD-anchor (old + new briefly share the CN — D4). That serial
//! path is exercised by the manual release runbook, not headless CI.
//!
//! Consent: `certutil -user -addstore Root` raises the Windows root-CA trust dialog (that IS the
//! user's consent), so the CI integration test seeds non-interactively via PowerShell
//! `Import-Certificate` and exercises verify/remove instead (P4 spike I3; see `tests/trust_windows.rs`).

use super::{AnchorRef, StoreStatus, TrustReport};
use std::path::{Path, PathBuf};
use std::process::Command;

const STORE: &str = "Windows CurrentUser Root";
const CA_CN: &str = "Aztec Accelerator Local CA";

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

/// Is OUR anchor present in the store, matched by CN (exit code only — locale-independent). The Root
/// store means trusted, so presence == trusted. Uses CN (not serial) so the common paths don't depend
/// on the exact certutil serial-string format; the store is empty of foreign "Aztec Accelerator Local
/// CA" certs, so a CN match is unambiguous for the "is it there" question.
fn is_present_by_cn() -> bool {
    Command::new(certutil_exe())
        .args(["-user", "-store", "Root", CA_CN])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
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
fn delete_by_cn() {
    let _ = Command::new(certutil_exe())
        .args(["-user", "-delstore", "Root", CA_CN])
        .output();
}

pub fn install(ca_cert: &Path) -> TrustReport {
    let ok = add_store(ca_cert) && is_present_by_cn();
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

pub fn status(_ca_cert: &Path) -> TrustReport {
    TrustReport {
        stores: vec![StoreStatus {
            store: STORE.into(),
            installed: is_present_by_cn(),
            detail: None,
        }],
    }
}

pub fn remove(_ca_cert: &Path) -> TrustReport {
    // Uninstall: delete ALL our anchors by CN (covers rotation leftovers too).
    delete_by_cn();
    TrustReport {
        stores: vec![StoreStatus {
            store: STORE.into(),
            installed: is_present_by_cn(),
            detail: None,
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
    // Just-added anchor is present (by CN — the new one is among the matches).
    if is_present_by_cn() {
        Ok(())
    } else {
        Err("new anchor not present in CurrentUser Root after add".into())
    }
}

pub fn remove_anchor(old: AnchorRef) {
    if let Some(serial) = old.0 {
        delete_by_serial(&serial);
    }
}
