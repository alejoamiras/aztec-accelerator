//! Real-`certutil.exe` CurrentUser-Root integration test (Windows). `#[ignore]`d; CI runs it with
//! `--ignored`. Drives the ACTUAL `crate::trust` code path against the real store, then cleans up by
//! CN in a guard so a failed assertion can't leave the anchor behind (audit R5 / P4 spike I3 — adding
//! to `CurrentUser\Root` is expected to be silent in a CI session; if it prompts, this hangs and the
//! job's timeout catches it, signalling the spike outcome).
#![cfg(target_os = "windows")]

/// Delete our test anchor by CN regardless of test outcome.
struct CnCleanup;
impl Drop for CnCleanup {
    fn drop(&mut self) {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let certutil = std::path::Path::new(&system_root)
            .join("System32")
            .join("certutil.exe");
        let _ = std::process::Command::new(certutil)
            .args(["-user", "-delstore", "Root", "Aztec Accelerator Local CA"])
            .output();
    }
}

#[test]
#[ignore = "adds to the real CurrentUser Root store; run in CI with --ignored"]
fn currentuser_root_add_verify_remove() {
    let _cleanup = CnCleanup;

    let home = tempfile::tempdir().expect("temp HOME");
    // SAFETY: single-threaded ignored test; isolates the generated certs under a throwaway profile.
    std::env::set_var("USERPROFILE", home.path());
    std::env::set_var("HOME", home.path());

    aztec_accelerator::certs::generate_and_save().expect("generate certs");
    let ca = aztec_accelerator::certs::live_ca_cert_path();
    assert!(ca.exists(), "ca.pem should exist after generate");

    // Install into CurrentUser Root and confirm it verifies.
    let report = aztec_accelerator::trust::install_ca_trust(&ca);
    assert!(
        report.any_installed(),
        "install should land in CurrentUser Root; report = {report:?}"
    );
    assert!(
        aztec_accelerator::trust::is_ca_trusted(&ca),
        "anchor should verify as trusted"
    );

    // Chain-validate the leaf as a TLS server cert (proves the anchor is accepted + honored — M-3).
    let leaf = home.path().join(".aztec-accelerator/certs/localhost.pem");
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let certutil = std::path::Path::new(&system_root)
        .join("System32")
        .join("certutil.exe");
    let verify = std::process::Command::new(&certutil)
        .arg("-verify")
        .arg(&leaf)
        .output()
        .expect("run certutil -verify");
    assert!(
        verify.status.success(),
        "leaf should verify through the trusted anchor: {}",
        String::from_utf8_lossy(&verify.stdout)
    );

    // Remove and confirm it's gone.
    let _ = aztec_accelerator::trust::remove_ca_trust(&ca);
    assert!(
        !aztec_accelerator::trust::is_ca_trusted(&ca),
        "anchor should be gone after remove_ca_trust"
    );
}
