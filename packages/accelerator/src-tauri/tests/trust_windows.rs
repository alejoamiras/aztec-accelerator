//! Windows CurrentUser-Root trust integration test. `#[ignore]`d; CI runs it with `--ignored`.
//!
//! P4 spike I3 outcome: `certutil -user -addstore Root` **prompts** (the Windows root-CA "Security
//! Warning" dialog) and therefore HANGS in a headless CI session — which is the correct behavior for
//! a real user (the dialog IS the consent). So per the plan's R5/I3 fallback, CI does NOT run our
//! (prompting) `install()`; instead it seeds the anchor NON-interactively via PowerShell
//! `Import-Certificate` and then exercises our real `is_ca_trusted()` (`certutil -verifystore <serial>`)
//! and `remove_ca_trust()` (`-delstore <CN>`) code — validating the risky serial-format arg mechanics
//! headlessly. The production add-store path (with its consent dialog) is covered by the manual
//! release runbook, NOT here.
#![cfg(target_os = "windows")]

use std::path::{Path, PathBuf};
use std::process::Command;

fn certutil() -> PathBuf {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    Path::new(&system_root)
        .join("System32")
        .join("certutil.exe")
}

/// Delete our anchor by CN from CurrentUser Root regardless of test outcome (no dialog on delete).
struct CnCleanup;
impl Drop for CnCleanup {
    fn drop(&mut self) {
        let _ = Command::new(certutil())
            .args(["-user", "-delstore", "Root", "Aztec Accelerator Local CA"])
            .output();
    }
}

/// Seed a cert into CurrentUser\Root non-interactively (Import-Certificate writes via the PKI API and
/// does NOT raise the CryptoAPI trust dialog, unlike `certutil -addstore Root`).
fn seed_into_currentuser_root(ca_pem: &Path) {
    let script = format!(
        "Import-Certificate -FilePath '{}' -CertStoreLocation Cert:\\CurrentUser\\Root | Out-Null",
        ca_pem.display()
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .expect("run powershell Import-Certificate");
    assert!(
        out.status.success(),
        "Import-Certificate should seed the anchor: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[ignore = "touches the real CurrentUser Root store; run in CI with --ignored"]
fn currentuser_root_verify_and_remove() {
    let _cleanup = CnCleanup;

    let home = tempfile::tempdir().expect("temp HOME");
    // SAFETY: single-threaded ignored test; isolates generated certs under a throwaway profile.
    std::env::set_var("USERPROFILE", home.path());
    std::env::set_var("HOME", home.path());

    aztec_accelerator::certs::generate_and_save().expect("generate certs");
    let ca = aztec_accelerator::certs::live_ca_cert_path();
    assert!(ca.exists(), "ca.pem should exist after generate");

    // Seed non-interactively (the dialog-raising add-store path is runbook-only — see module docs).
    seed_into_currentuser_root(&ca);

    // Our verify path: `certutil -verifystore Root <serial>` by exit code — validates that the serial
    // string we derive from the PEM (x509-parser, compact lowercase hex) matches what certutil expects.
    assert!(
        aztec_accelerator::trust::is_ca_trusted(&ca),
        "is_ca_trusted must confirm the seeded anchor by serial"
    );

    // Chain-validate the leaf as a TLS server cert (anchor accepted + honored — M-3).
    let leaf = home.path().join(".aztec-accelerator/certs/localhost.pem");
    let verify = Command::new(certutil())
        .arg("-verify")
        .arg(&leaf)
        .output()
        .expect("run certutil -verify");
    assert!(
        verify.status.success(),
        "leaf should verify through the trusted anchor: {}",
        String::from_utf8_lossy(&verify.stdout)
    );

    // Our remove path: `-delstore Root <CN>` (no dialog on delete) — must clear it.
    let _ = aztec_accelerator::trust::remove_ca_trust(&ca);
    assert!(
        !aztec_accelerator::trust::is_ca_trusted(&ca),
        "anchor should be gone after remove_ca_trust"
    );
}
