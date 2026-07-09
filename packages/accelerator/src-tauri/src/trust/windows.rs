//! Windows trust backend — the CurrentUser `Root` store via `certutil.exe` (absolute System32 path).
//!
//! Design (plan D3/D4, codex/audit): `certutil.exe` is a system binary (zero new Rust deps; the
//! established shell-out-by-absolute-path pattern, cf. `crash_recovery::schtasks_exe`). We lean ONLY
//! on exit codes (`-verifystore` non-zero if absent — locale-independent) and, for identity-specific
//! deletes during rotation, the cert's **serial number** parsed from the PEM with the already-present
//! `x509-parser` (never scraping localized stdout). Uninstall deletes by **CN** (only our anchor
//! remains post-rotation, and the NSIS hook has no `x509-parser` at uninstall time).
//!
//! No GUI dialog is guaranteed for `-addstore Root` (it may be silent) — so the wizard's Start click
//! is the consent ceremony, NOT a Windows prompt (audit R8).
//!
//! NOTE (build): this module is `#[cfg(target_os = "windows")]`, so it is first compiled on the
//! Windows CI leg. The exact serial-string format `certutil` accepts for `-delstore` is confirmed by
//! the Phase-4 `trust_windows` CI spike; the arg construction here is the candidate under test.

use super::{AnchorRef, StoreStatus, TrustReport};
use std::path::{Path, PathBuf};
use std::process::Command;

const STORE: &str = "Windows CurrentUser Root";
const CA_CN: &str = "Aztec Accelerator Local CA";

/// Absolute path to `certutil.exe` — never a bare-name PATH lookup (a planted `certutil` earlier on
/// PATH must not win). Uses `SystemRoot`/`windir` with a hardcoded `C:\Windows` fallback, matching the
/// existing `crash_recovery::schtasks_exe` precedent (avoids a `windows-sys`/FFI dependency for
/// `GetSystemDirectoryW`; codex's preferred API is noted as a possible future hardening).
fn certutil_exe() -> PathBuf {
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

/// Verify our anchor is present in the store by serial (exit code only — locale-independent).
fn verify_by_serial(serial: &str) -> bool {
    Command::new(certutil_exe())
        .args(["-user", "-verifystore", "Root", serial])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn delete_by_serial(serial: &str) {
    let _ = Command::new(certutil_exe())
        .args(["-user", "-delstore", "Root", serial])
        .output();
}

/// Uninstall path: delete by CN (removes every anchor named `CA_CN` — only ours, and post-rotation
/// only one remains). Used by remove() + the `--remove-ca-trust` CLI the NSIS uninstaller calls.
fn delete_by_cn() {
    let _ = Command::new(certutil_exe())
        .args(["-user", "-delstore", "Root", CA_CN])
        .output();
}

pub fn install(ca_cert: &Path) -> TrustReport {
    let ok = add_store(ca_cert)
        && cert_serial(ca_cert)
            .map(|s| verify_by_serial(&s))
            .unwrap_or(false);
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
    let installed = cert_serial(ca_cert)
        .map(|s| verify_by_serial(&s))
        .unwrap_or(false);
    TrustReport {
        stores: vec![StoreStatus {
            store: STORE.into(),
            installed,
            detail: None,
        }],
    }
}

pub fn remove(ca_cert: &Path) -> TrustReport {
    // Uninstall: delete by CN (covers a lingering pre-rotation anchor too).
    delete_by_cn();
    let still = cert_serial(ca_cert)
        .map(|s| verify_by_serial(&s))
        .unwrap_or(false);
    TrustReport {
        stores: vec![StoreStatus {
            store: STORE.into(),
            installed: still,
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
    match cert_serial(staged_ca) {
        Some(s) if verify_by_serial(&s) => Ok(()),
        _ => Err("new anchor not verifiable in CurrentUser Root after add".into()),
    }
}

pub fn remove_anchor(old: AnchorRef) {
    if let Some(serial) = old.0 {
        delete_by_serial(&serial);
    }
}
