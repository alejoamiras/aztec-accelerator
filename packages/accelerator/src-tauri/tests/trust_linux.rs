//! Real-`certutil` NSS integration test (Linux). `#[ignore]`d so a normal `cargo test` (and a dev
//! machine without libnss3-tools) skips it; CI runs it with `--ignored` after `apt-get install
//! libnss3-tools`. It drives the ACTUAL `crate::trust` code path against a real NSS database in a
//! throwaway `$HOME`, then chain-validates a leaf through the name-constrained anchor (audit R9/M-3 —
//! proves NSS accepts + honors the anchor, not just that the row is present).
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;

fn sql(dir: &Path) -> String {
    format!("sql:{}", dir.display())
}

/// Add a leaf PEM into an NSS DB and return whether `certutil -V` validates it as a TLS server cert —
/// i.e. it chains up to (and is authorized by) the trusted anchor already in the DB.
fn leaf_chain_validates(nssdb: &Path, leaf_pem: &Path) -> bool {
    // Import the leaf under a throwaway nickname (no trust flags — it must validate via the anchor).
    let add = Command::new("/usr/bin/certutil")
        .args(["-A", "-t", ",,", "-n", "aztec-test-leaf", "-i"])
        .arg(leaf_pem)
        .args(["-d", &sql(nssdb)])
        .output()
        .expect("run certutil -A leaf");
    assert!(
        add.status.success(),
        "certutil -A leaf failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    // -V -u V: verify the cert is valid for the TLS *server* usage → exercises chain + NameConstraints.
    Command::new("/usr/bin/certutil")
        .args(["-V", "-u", "V", "-n", "aztec-test-leaf", "-d", &sql(nssdb)])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
#[ignore = "needs libnss3-tools (certutil); run in CI with --ignored"]
fn nss_install_verify_chain_remove() {
    let home = tempfile::tempdir().expect("temp HOME");
    // SAFETY: single-threaded ignored test; isolates all HOME-derived paths off the real profile.
    std::env::set_var("HOME", home.path());

    // 1. Generate a real keyless CA + leaf (writes to $HOME/.aztec-accelerator/certs).
    aztec_accelerator::certs::generate_and_save().expect("generate certs");
    let ca = aztec_accelerator::certs::live_ca_cert_path();
    assert!(ca.exists(), "ca.pem should exist after generate");

    // 2. Install into the browser trust stores (creates $HOME/.pki/nssdb).
    let report = aztec_accelerator::trust::install_ca_trust(&ca);
    assert!(
        report.any_installed(),
        "install should land in >=1 store; report = {report:?}"
    );

    // 3. is_ca_trusted must now see it (certutil -L found the nickname).
    assert!(
        aztec_accelerator::trust::is_ca_trusted(&ca),
        "anchor should read as trusted"
    );

    // 4. Chain-validate a leaf through the name-constrained anchor (R9/M-3).
    let nssdb: PathBuf = home.path().join(".pki/nssdb");
    let leaf_pem = home.path().join(".aztec-accelerator/certs/localhost.pem");
    assert!(
        leaf_chain_validates(&nssdb, &leaf_pem),
        "the localhost leaf must validate through the trusted name-constrained anchor"
    );

    // 5. Remove trust → anchor must no longer read as trusted.
    let _ = aztec_accelerator::trust::remove_ca_trust(&ca);
    assert!(
        !aztec_accelerator::trust::is_ca_trusted(&ca),
        "anchor should be gone after remove_ca_trust"
    );
}
