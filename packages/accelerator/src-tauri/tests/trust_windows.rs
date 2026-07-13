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

/// Seed a cert into CurrentUser\Root non-interactively. Uses the .NET `X509Store` API directly —
/// which writes to the store WITHOUT the CryptoAPI trust dialog `certutil -addstore Root` raises, and
/// WITHOUT depending on the `Cert:` PSDrive (absent on the runner). PEM→DER via `certutil -decode`
/// first so `X509Certificate2` loads native DER.
fn seed_into_currentuser_root(ca_pem: &Path) {
    let der = ca_pem.with_extension("der");
    let dec = Command::new(certutil())
        .arg("-decode")
        .arg(ca_pem)
        .arg(&der)
        .output()
        .expect("run certutil -decode");
    assert!(
        dec.status.success(),
        "certutil -decode PEM->DER: {}",
        String::from_utf8_lossy(&dec.stderr)
    );

    let script = format!(
        "$c = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2('{}'); \
         $s = New-Object System.Security.Cryptography.X509Certificates.X509Store('Root','CurrentUser'); \
         $s.Open('ReadWrite'); $s.Add($c); $s.Close()",
        der.display()
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .expect("run powershell X509Store add");
    assert!(
        out.status.success(),
        "X509Store.Add should seed the anchor: {}",
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

    // Our verify path: `certutil -store Root <CN>` by exit code — the seeded anchor must read as
    // present/trusted. (Leaf name-constraint chain-validation is covered on the Linux + macOS legs;
    // `certutil -verify` here would additionally require revocation info our CRL-less CA lacks.)
    assert!(
        aztec_accelerator::trust::is_ca_trusted(&ca),
        "is_ca_trusted must confirm the seeded anchor by CN"
    );

    // Our remove path: `-delstore Root <CN>` (no dialog on delete) — must clear it.
    let _ = aztec_accelerator::trust::remove_ca_trust(&ca);
    assert!(
        !aztec_accelerator::trust::is_ca_trusted(&ca),
        "anchor should be gone after remove_ca_trust"
    );
}
