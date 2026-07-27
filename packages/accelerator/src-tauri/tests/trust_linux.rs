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

/// The nickname of OUR installed anchor (prefix `aztec-accelerator-ca-`), read back from `certutil -L`.
fn our_anchor_nick(nssdb: &Path) -> String {
    let out = Command::new("/usr/bin/certutil")
        .args(["-L", "-d", &sql(nssdb)])
        .output()
        .expect("certutil -L");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .find(|t| t.starts_with("aztec-accelerator-ca-"))
        .expect("our anchor nickname must be present after install")
        .to_string()
}

fn set_trust_flags(nssdb: &Path, nick: &str, flags: &str) {
    let m = Command::new("/usr/bin/certutil")
        .args(["-M", "-n", nick, "-t", flags, "-d", &sql(nssdb)])
        .output()
        .expect("run certutil -M");
    assert!(
        m.status.success(),
        "certutil -M {flags} failed: {}",
        String::from_utf8_lossy(&m.stderr)
    );
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

    // 3. is_ca_trusted must now see it (certutil -V -u L validates it as a trusted SSL CA).
    assert!(
        aztec_accelerator::trust::is_ca_trusted(&ca),
        "anchor should read as trusted"
    );

    let nssdb: PathBuf = home.path().join(".pki/nssdb");

    // 3b. presence != trust (post-impl codex Medium): mark our PRESENT anchor as a plain peer (drop the
    // `C` SSL-CA trust flag). is_ca_trusted must now be FALSE — proving it validates trust (`-V -u L`),
    // not mere existence (`-L -n`). Restore `C,,` afterwards so the rest of the test sees a normal install.
    let nick = our_anchor_nick(&nssdb);
    set_trust_flags(&nssdb, &nick, "p,,");
    assert!(
        !aztec_accelerator::trust::is_ca_trusted(&ca),
        "a present-but-distrusted anchor must NOT read as trusted"
    );
    set_trust_flags(&nssdb, &nick, "C,,");
    assert!(
        aztec_accelerator::trust::is_ca_trusted(&ca),
        "restoring the C trust flag should make it trusted again"
    );

    // 4. Chain-validate a leaf through the name-constrained anchor (R9/M-3).
    let leaf_pem = home.path().join(".aztec-accelerator/certs/localhost.pem");
    assert!(
        leaf_chain_validates(&nssdb, &leaf_pem),
        "the localhost leaf must validate through the trusted name-constrained anchor"
    );

    // 5. Remove trust → the report must CONFIRM removal (not just report success blindly), and the
    // anchor must no longer read as trusted (post-impl codex High: removal-error propagation).
    let remove_report = aztec_accelerator::trust::remove_ca_trust(&ca);
    assert!(
        !remove_report.removal_incomplete(),
        "remove must confirm the anchor is gone from every readable store: {remove_report:?}"
    );
    assert!(
        !aztec_accelerator::trust::is_ca_trusted(&ca),
        "anchor should be gone after remove_ca_trust"
    );
}
