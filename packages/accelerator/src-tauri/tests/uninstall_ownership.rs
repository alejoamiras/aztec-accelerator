//! L3 — real-OS uninstall-ownership integration (B5). `#[ignore]`d so a normal `cargo test` skips it;
//! the per-OS CI legs run `cargo test --test uninstall_ownership -- --ignored --nocapture`. ONE test per
//! OS, driven sequentially in a throwaway `$HOME` so there is no parallel `set_var` race.
//!
//! The property under test is the #429 hazard: a SECOND install (copied instance) that shares this user's
//! `~/.aztec-accelerator`. `--prepare-uninstall` for THIS copy must detect the foreign owner from the
//! stored autostart entry and LEAVE all shared state (trust + certs) and the other copy's autostart entry.

use aztec_accelerator::autostart::enable_entry_at;
use aztec_accelerator::uninstall::{prepare_uninstall, Step};

#[cfg(unix)]
fn make_exe(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let p = dir.join(name);
    std::fs::write(&p, b"#!/bin/sh\nexit 0\n").expect("write exe");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    p
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "real-OS integration: writes a .desktop + certs under a throwaway $HOME; CI runs with --ignored"]
fn foreign_entry_leaves_everything_incl_certs() {
    let home = tempfile::tempdir().expect("temp HOME");
    // SAFETY: single #[test] in this binary on this OS — no parallel env mutation.
    std::env::set_var("HOME", home.path());
    std::env::set_var("XDG_CONFIG_HOME", home.path().join(".config"));

    // Arm the autostart entry pointing at ANOTHER install's binary (a copied instance, #429). Our own
    // identity is `current_exe()` (this test harness), which differs → the entry is FOREIGN.
    let other = tempfile::tempdir().expect("other install dir");
    let foreign_exe = make_exe(other.path(), "AztecAccelerator");
    enable_entry_at(&foreign_exe).expect("enable foreign entry");

    // The shared state the OTHER install still relies on.
    let certs = home.path().join(".aztec-accelerator/certs");
    std::fs::create_dir_all(&certs).unwrap();
    let anchor = certs.join("ca.pem");
    std::fs::write(&anchor, b"-----BEGIN CERTIFICATE-----\n").unwrap();

    let outcome = prepare_uninstall();

    assert!(
        outcome.foreign_detected,
        "a foreign autostart entry must be detected"
    );
    assert_eq!(
        outcome.trust,
        Step::LeftForeign,
        "shared trust/certs must be LEFT for the other install"
    );
    assert_eq!(
        outcome.autostart,
        Step::LeftForeign,
        "the other install's autostart entry must be LEFT"
    );
    assert_eq!(outcome.crash_recovery, Step::LeftForeign);
    assert!(
        !outcome.incomplete(),
        "a deliberate foreign skip is success (exit 0), not a failure"
    );
    // The concrete guarantee. Mutation proof: make `classify_ownership` always return `Ours` and this
    // file is deleted (the Ours path removes trust+certs) → this assertion fails.
    assert!(
        anchor.exists(),
        "prepare-uninstall must NOT delete another install's shared certs"
    );
    // And the foreign autostart .desktop must survive untouched.
    let desktop = home
        .path()
        .join(".config/autostart/Aztec Accelerator.desktop");
    assert!(
        desktop.exists(),
        "prepare-uninstall must NOT remove another install's autostart entry"
    );
}
