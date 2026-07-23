//! macOS trust integration (headless-safe subset). `#[ignore]`d; CI runs it with `--ignored`.
//!
//! Audit R5 / H-1: `security add-trusted-cert` records trust in the user trust-settings *domain*,
//! which requires interactive authorization — so the FULL install flow cannot run non-interactively
//! on a headless runner (it would hang or return `errSecAuthorizationDenied`). This test therefore
//! exercises only the headless-SAFE paths: cert generation, and the status/verify query
//! (`security verify-cert`), confirming `crate::trust`'s macOS backend runs end-to-end without a
//! prompt. The real install/trust flow is covered by the manual pre-release runbook (spike I7), not
//! by CI — do NOT read a green here as "the production login-keychain trust path is CI-covered."
#![cfg(target_os = "macos")]

#[test]
#[ignore = "macOS trust query (install needs interactive auth — see module docs); CI runs with --ignored"]
fn generate_and_status_query_are_headless_safe() {
    let home = tempfile::tempdir().expect("temp HOME");
    // SAFETY: single-threaded ignored test; isolates the generated certs under a throwaway profile.
    std::env::set_var("HOME", home.path());

    aztec_accelerator::certs::generate_and_save().expect("generate certs");
    let ca = aztec_accelerator::certs::live_ca_cert_path();
    assert!(ca.exists(), "ca.pem should exist after generate");

    // A fresh, un-installed anchor must read as NOT trusted — and crucially this must return without
    // raising any prompt (exercises the `security verify-cert` path in the macOS backend).
    assert!(
        !aztec_accelerator::trust::is_ca_trusted(&ca),
        "a freshly generated, un-installed anchor must not read as trusted"
    );

    // trust_status must enumerate the Keychain store without panicking or prompting.
    let report = aztec_accelerator::trust::trust_status(&ca);
    assert!(
        report.stores.iter().any(|s| s.store.contains("Keychain")),
        "status should report the macOS Keychain store; got {report:?}"
    );
}
