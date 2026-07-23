//! Windows trust integration (headless-safe subset). `#[ignore]`d; CI runs it with `--ignored`.
//!
//! P4 spike I3 outcome: adding a cert to `CurrentUser\Root` — via `certutil -addstore Root` OR the
//! .NET `X509Store.Add` — raises the Windows root-CA "Security Warning" dialog and cannot be done
//! silently (Windows design; the dialog IS the user's consent, which is correct for a real user but
//! un-answerable in a headless runner, so it hangs). So, exactly like the macOS leg, CI exercises only
//! the headless-SAFE read paths (`is_ca_trusted`/`trust_status` → `certutil -store`, which do NOT
//! prompt); the real add/remove flow (with its consent dialog) is covered by the manual release
//! runbook, NOT here. Do NOT read a green here as "the production Root add/remove is CI-covered".
#![cfg(target_os = "windows")]

#[test]
#[ignore = "Windows trust read paths (add needs the interactive Root dialog — see module docs); CI runs with --ignored"]
fn read_paths_are_headless_safe() {
    let home = tempfile::tempdir().expect("temp HOME");
    // SAFETY: single-threaded ignored test; isolates generated certs under a throwaway profile.
    std::env::set_var("USERPROFILE", home.path());
    std::env::set_var("HOME", home.path());

    aztec_accelerator::certs::generate_and_save().expect("generate certs");
    let ca = aztec_accelerator::certs::live_ca_cert_path();
    assert!(ca.exists(), "ca.pem should exist after generate");

    // A fresh, un-installed anchor must read as NOT trusted — and this must return WITHOUT prompting
    // (exercises `certutil -user -store Root <CN>` exit-code handling, the CN-based presence check our
    // status/is_ca_trusted/remove paths all use, running cleanly on real certutil).
    assert!(
        !aztec_accelerator::trust::is_ca_trusted(&ca),
        "a freshly generated, un-installed anchor must not read as trusted"
    );

    // trust_status must enumerate the CurrentUser Root store without panicking or prompting.
    let report = aztec_accelerator::trust::trust_status(&ca);
    assert!(
        report
            .stores
            .iter()
            .any(|s| s.store.contains("CurrentUser Root")),
        "status should report the Windows CurrentUser Root store; got {report:?}"
    );
}
