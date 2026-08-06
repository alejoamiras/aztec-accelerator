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

/// Serialises the tests in this file.
///
/// Each one redirects `HOME` — a PROCESS-global — to its own throwaway profile, and cargo runs the
/// tests in a file on parallel threads. That was harmless while there was exactly one test here,
/// which is what the "single-threaded ignored test" note on each `set_var` was asserting. Adding a
/// second test silently invalidated it: two tests can interleave `set_var` and then read each
/// other's profile, so one asserts against certs that were generated somewhere else.
///
/// Holding this for the whole body restores the invariant the note claims. Poisoning is ignored on
/// purpose — one failing test should report its own failure, not cascade into the others.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
#[ignore = "Windows trust read paths (add needs the interactive Root dialog — see module docs); CI runs with --ignored"]
fn read_paths_are_headless_safe() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // certs_exist() (called by generate_and_save) now validates the leaf+key load into a rustls
    // ServerConfig, which needs a process CryptoProvider — the real app installs it in main(); tests
    // must too (idempotent).
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();

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

/// The `Disallowed` (Untrusted Certificates) store OVERRIDES `Root` in Windows chain building, so
/// "trusted" must mean *in Root AND not in Disallowed*. Adding to `Disallowed` does NOT raise the
/// root-CA consent dialog (only `Root` does), so unlike the Root add this IS headless-safe: we can
/// seed it for real and assert the verdict flips.
#[test]
#[ignore = "drives real certutil against CurrentUser\\Disallowed (no dialog); CI runs with --ignored"]
fn disallowed_store_overrides_root() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();

    let home = tempfile::tempdir().expect("temp HOME");
    // SAFETY: single-threaded ignored test; isolates generated certs under a throwaway profile.
    std::env::set_var("USERPROFILE", home.path());
    std::env::set_var("HOME", home.path());

    aztec_accelerator::certs::generate_and_save().expect("generate certs");
    let ca = aztec_accelerator::certs::live_ca_cert_path();

    // Baseline: a fresh anchor is in neither store → not trusted.
    assert!(
        !aztec_accelerator::trust::is_ca_trusted(&ca),
        "a freshly generated anchor is in neither store, so it must not read as trusted"
    );

    // Seed CurrentUser\Disallowed with our CA (silent — no consent dialog for this store).
    let added = std::process::Command::new("certutil")
        .args(["-user", "-addstore", "Disallowed"])
        .arg(&ca)
        .output()
        .expect("run certutil -addstore Disallowed");
    assert!(
        added.status.success(),
        "certutil -addstore Disallowed failed: {}",
        String::from_utf8_lossy(&added.stderr)
    );

    // Still not trusted — and now for the RIGHT reason: explicit distrust. (Root was never seeded
    // here, so this asserts the Disallowed probe runs and is exit-code-correct against real certutil;
    // the Root-present-AND-Disallowed combination needs the interactive Root add, so it stays on the
    // manual runbook.)
    assert!(
        !aztec_accelerator::trust::is_ca_trusted(&ca),
        "a CA in the Disallowed store must never read as trusted"
    );

    // Clean up so the throwaway profile leaves nothing behind in the real user's store.
    let _ = std::process::Command::new("certutil")
        .args([
            "-user",
            "-delstore",
            "Disallowed",
            "Aztec Accelerator Local CA",
        ])
        .output();
}

/// F-05's other direction on the REAL OS: a removal that genuinely had nothing to remove must report
/// COMPLETE.
///
/// The fix makes "could not determine" report as still-trusted, which is the fail-closed direction —
/// but a control that fails closed too eagerly cries wolf on every ordinary uninstall, and that is
/// how a security control gets switched off. Here `certutil` works and our CA was never installed,
/// which is every normal user's machine: `delete_by_cn` reports clean via `CRYPT_E_NOT_FOUND` and
/// `presence_by_cn` reports absent, so removal must be reported as done.
///
/// Headless-safe: `-delstore` raises no dialog (only `-addstore Root` does), and there is nothing to
/// delete. This complements the NSIS harness, which covers the same direction for the INSTALLER path.
#[test]
#[ignore = "drives real certutil -delstore against CurrentUser\\Root (no dialog); CI runs with --ignored"]
fn removal_with_nothing_to_remove_reports_complete() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();

    let home = tempfile::tempdir().expect("temp HOME");
    // SAFETY: single-threaded ignored test; isolates generated certs under a throwaway profile.
    std::env::set_var("USERPROFILE", home.path());
    std::env::set_var("HOME", home.path());

    aztec_accelerator::certs::generate_and_save().expect("generate certs");
    let ca = aztec_accelerator::certs::live_ca_cert_path();

    // Print what certutil ACTUALLY says for a filtered miss. The first version of the F-05 fix keyed
    // "absent" on CRYPT_E_NOT_FOUND appearing here, which is false on a real runner — this line is so
    // the next person reads reality out of the CI log instead of assuming, as I did.
    let raw = std::process::Command::new("certutil")
        .args(["-user", "-store", "Root", "Aztec Accelerator Local CA"])
        .output();
    match &raw {
        Ok(o) => println!(
            "[diagnostic] certutil -store Root <CN>: status={:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            o.status.code(),
            String::from_utf8_lossy(&o.stdout).trim(),
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => println!("[diagnostic] certutil could not be spawned: {e}"),
    }

    let report = aztec_accelerator::trust::remove_ca_trust(&ca);
    assert!(
        !report.removal_incomplete(),
        "certutil works and nothing of ours is installed, so removal must report COMPLETE — a \
         false 'still trusted' here would fire on every ordinary uninstall; got {report:?}"
    );
}
