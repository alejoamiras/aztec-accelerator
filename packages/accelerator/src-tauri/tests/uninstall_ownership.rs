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
#[serial_test::serial]
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

/// codex r2 #3: a FOREIGN crash-recovery unit is itself evidence of a copied install, so the SHARED
/// trust/certs must be LEFT even when this install has no autostart entry (NoEntry). Before the fix,
/// `remove_trust_and_certs` ran unconditionally after leaving the foreign task.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "real-OS integration: writes a systemd unit + certs under a throwaway $HOME; CI runs with --ignored"]
#[serial_test::serial]
fn foreign_recovery_unit_leaves_shared_trust() {
    use aztec_accelerator::uninstall::{prepare_uninstall, Step};
    let home = tempfile::tempdir().expect("temp HOME");
    let xdg = home.path().join(".config");
    std::env::set_var("HOME", home.path());
    std::env::set_var("XDG_CONFIG_HOME", &xdg);

    // No autostart entry (NoEntry), but a crash-recovery unit whose ExecStart targets ANOTHER install
    // (its serialized `:`-prefixed form never equals our reference's) ⇒ Foreign.
    let unit_dir = xdg.join("systemd/user");
    std::fs::create_dir_all(&unit_dir).unwrap();
    std::fs::write(
        unit_dir.join("aztec-accelerator.service"),
        "[Service]\nExecStart=\":/opt/other-install/AztecAccelerator\"\n",
    )
    .unwrap();

    // Shared state the OTHER install relies on.
    let certs = home.path().join(".aztec-accelerator/certs");
    std::fs::create_dir_all(&certs).unwrap();
    let anchor = certs.join("ca.pem");
    std::fs::write(&anchor, b"-----BEGIN CERTIFICATE-----\n").unwrap();

    let outcome = prepare_uninstall();
    assert!(
        outcome.foreign_detected,
        "a foreign crash-recovery unit must be detected"
    );
    assert_eq!(
        outcome.trust,
        Step::LeftForeign,
        "shared trust must be LEFT"
    );
    assert_eq!(outcome.crash_recovery, Step::LeftForeign);
    assert!(!outcome.incomplete(), "a foreign skip is success (exit 0)");
    assert!(
        anchor.exists(),
        "a foreign recovery unit must NOT strip the shared certs"
    );
}

/// End-to-end via the BUILT binary (establishes the CARGO_BIN_EXE pattern the plan asked for): an OWNED
/// install is torn down and a re-run is idempotent. Runs `AztecAccelerator --prepare-uninstall` for real,
/// so it also proves the early-argv branch + exit code, not just the library.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "real-OS integration: spawns the built binary under a throwaway $HOME; CI runs with --ignored"]
#[serial_test::serial]
fn owned_prepare_uninstall_removes_autostart_and_is_idempotent() {
    use aztec_accelerator::autostart::enable_entry_at;
    let home = tempfile::tempdir().expect("temp HOME");
    let xdg = home.path().join(".config");
    std::env::set_var("HOME", home.path());
    std::env::set_var("XDG_CONFIG_HOME", &xdg);

    let bin = env!("CARGO_BIN_EXE_AztecAccelerator");
    // Arm autostart pointing at the binary itself → its owned_reference_path (current_exe) resolves to the
    // same exe → ConfirmedOurs.
    enable_entry_at(std::path::Path::new(bin)).expect("arm autostart");
    let desktop = home
        .path()
        .join(".config/autostart/Aztec Accelerator.desktop");
    assert!(desktop.exists(), "sanity: the entry was armed");

    let run = || {
        std::process::Command::new(bin)
            .arg("--prepare-uninstall")
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", &xdg)
            .output()
            .expect("run --prepare-uninstall")
    };

    let out = run();
    assert!(
        out.status.success(),
        "prepare-uninstall must exit 0 for an owned install: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!desktop.exists(), "our autostart entry must be removed");

    // Idempotent: a second run (now NoEntry) is still a clean success and leaves nothing.
    let out2 = run();
    assert!(
        out2.status.success(),
        "a second --prepare-uninstall must be idempotent success: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    assert!(!desktop.exists());
}

/// Windows: exercise `recovery_ownership` against a REAL scheduled task created the way the app creates it
/// (`enable_crash_recovery` → `task_xml` → schtasks `/XML`), so the schtasks `/Query /XML` round-trip and
/// UTF-16 decode are validated on a real runner — the layer Wine cannot cover (codex #7b). The task is the
/// app's own name, so the (ephemeral) runner is left clean.
#[cfg(windows)]
#[test]
#[ignore = "real-OS integration: creates/deletes the REAL crash-recovery task; CI runs with --ignored"]
fn windows_recovery_ownership_round_trips_our_task_and_rejects_foreign() {
    use aztec_accelerator::crash_recovery::{
        disable_crash_recovery, enable_crash_recovery, recovery_ownership, RecoveryOwnership,
    };
    let me = std::env::current_exe().expect("current_exe");

    disable_crash_recovery(); // clean slate
    assert_eq!(recovery_ownership(&me), RecoveryOwnership::Absent);

    // Create the task exactly as the app does — its <Command> is this exe (task_xml).
    enable_crash_recovery().expect("create crash-recovery task");
    assert_eq!(
        recovery_ownership(&me),
        RecoveryOwnership::Ours,
        "our own task's <Command> must be recognized"
    );
    // A different reference is NOT ours (its <Command> element won't match).
    assert_eq!(
        recovery_ownership(std::path::Path::new("C:\\Other\\AztecAccelerator.exe")),
        RecoveryOwnership::Foreign
    );

    disable_crash_recovery(); // cleanup
    assert_eq!(recovery_ownership(&me), RecoveryOwnership::Absent);
}
