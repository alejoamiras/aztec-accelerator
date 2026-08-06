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
    // certs_exist() (via generate_and_save) validates the leaf+key into a rustls ServerConfig, which
    // needs a process CryptoProvider (installed in the real app's main(); idempotent here).
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();

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

/// F-05 on the REAL OS: a removal we could not verify must not report success.
///
/// The existing test above covers only the status/verify query, so every line the F-05 fix touched
/// on macOS — `keychain_anchor`, `keychain_is_readable`, `remove` — was compiled by CI and never
/// executed by it. A green `Cert Trust (macos)` therefore proved the backend builds, not that the
/// removal classification works. Found while checking the fixes after PR #434 went green.
///
/// This is headless-safe for the same reason the test above is: `security find-certificate` reads,
/// it never raises the authorization prompt that `add-trusted-cert` does. Pointing `HOME` at a
/// throwaway directory means there is no `login.keychain-db` at all, which is exactly the
/// "could not ask" state the fix is about — and pre-F-05 that state returned `None` from
/// `keychain_sha1`, sailed through the delete loop without attempting anything, and reported a
/// clean removal.
#[test]
#[ignore = "macOS trust removal against an unreachable keychain (no prompt); CI runs with --ignored"]
fn removal_against_an_unreachable_keychain_reports_incomplete() {
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();

    let home = tempfile::tempdir().expect("temp HOME");
    // SAFETY: single-threaded ignored test; isolates the generated certs under a throwaway profile.
    std::env::set_var("HOME", home.path());

    aztec_accelerator::certs::generate_and_save().expect("generate certs");
    let ca = aztec_accelerator::certs::live_ca_cert_path();

    // There is no login keychain under this HOME, so removal cannot be confirmed either way.
    let report = aztec_accelerator::trust::remove_ca_trust(&ca);
    assert!(
        report.removal_incomplete(),
        "an unverifiable removal must report INCOMPLETE, not success; got {report:?}"
    );
    let detail = report
        .removal_failure_detail()
        .expect("an incomplete removal must carry a reason");
    assert!(
        detail.contains("could not verify") || detail.contains("still trusted"),
        "the reason should say why it could not be confirmed; got {detail:?}"
    );
}
