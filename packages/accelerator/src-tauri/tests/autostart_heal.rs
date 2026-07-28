//! L3 — real-OS autostart integration (plan §6). `#[ignore]`d so a normal `cargo test` skips it;
//! the per-OS CI legs run `cargo test --test autostart_heal -- --ignored --nocapture`
//! (`trust_linux.rs` shape). ONE test per OS: each drives the full lifecycle sequentially in a
//! throwaway `$HOME`, so there is no parallel `set_var` race and (on Windows) no concurrent
//! registry mutation — HKCU is per-user GLOBAL, a throwaway `$HOME` does NOT isolate it (audit r4),
//! hence the RAII snapshot/restore guard around the real Run value.

use aztec_accelerator::autostart::{
    enable_entry_at, heal_if_broken_at, intent_enabled_now, read_stored_target, remove_entry,
    HealOutcome, StoredTarget,
};
use std::path::{Path, PathBuf};

/// A real executable file the stored entry can resolve to.
fn make_exe(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, b"#!/bin/sh\nexit 0\n").expect("write exe");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    p
}

fn assert_healthy_at(target: &StoredTarget, expect: &Path) {
    match target {
        StoredTarget::Healthy { program, .. } => {
            assert_eq!(
                program.canonicalize().expect("canon stored"),
                expect.canonicalize().expect("canon expected"),
                "stored program must resolve to the expected executable"
            );
        }
        other => panic!("expected Healthy at {}, got {other:?}", expect.display()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Linux
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
#[test]
#[ignore = "real-OS integration: writes a .desktop under a throwaway $HOME; CI runs with --ignored"]
fn linux_full_lifecycle_enable_break_heal_disable() {
    let home = tempfile::tempdir().expect("temp HOME");
    // SAFETY: single #[test] in this binary on this OS — no parallel env mutation. Both HOME and
    // XDG_CONFIG_HOME are pinned (D9: production honours XDG; leaving it inherited would silently
    // point the test at the dev machine's real config dir).
    std::env::set_var("HOME", home.path());
    std::env::set_var("XDG_CONFIG_HOME", home.path().join(".config"));

    let bin = tempfile::tempdir().expect("bin dir");
    // A spaced path: the removed plugin wrote it unquoted (= broken first-token Exec); ours must
    // quote it and round-trip.
    let spaced_dir = bin.path().join("aztec accelerator");
    std::fs::create_dir_all(&spaced_dir).unwrap();
    let v1 = make_exe(&spaced_dir, "app-v1");
    let v2 = make_exe(&spaced_dir, "app-v2");

    // 1. Enable → artifact exists, quoted, Healthy, intent ON.
    enable_entry_at(&v1).expect("enable");
    let desktop = home
        .path()
        .join(".config/autostart")
        .join("Aztec Accelerator.desktop");
    let ini = std::fs::read_to_string(&desktop).expect(".desktop written");
    assert!(
        ini.contains("Exec=\""),
        "enable-time Exec must be QUOTED (spaced path), got:\n{ini}"
    );
    assert_healthy_at(&read_stored_target(Some(&v1)), &v1);
    assert!(intent_enabled_now().expect("intent readable"));

    // 2. Relocate (delete the stored target) → Broken.
    std::fs::remove_file(&v1).unwrap();
    assert!(matches!(
        read_stored_target(Some(&v2)),
        StoredTarget::Broken { .. }
    ));

    // 3. Heal to the new live path → Healed; every non-Exec line preserved byte-identically.
    let before: Vec<String> = ini
        .lines()
        .filter(|l| !l.starts_with("Exec"))
        .map(String::from)
        .collect();
    match heal_if_broken_at(&v2) {
        HealOutcome::Healed { from, to } => {
            // D24: redacted — no full home path may appear in the logged transition.
            let home_str = home.path().to_string_lossy().into_owned();
            assert!(!from.contains(&home_str) && !to.contains(&home_str));
        }
        other => panic!("expected Healed, got {other:?}"),
    }
    assert_healthy_at(&read_stored_target(Some(&v2)), &v2);
    let healed_ini = std::fs::read_to_string(&desktop).unwrap();
    let after: Vec<String> = healed_ini
        .lines()
        .filter(|l| !l.starts_with("Exec"))
        .map(String::from)
        .collect();
    assert_eq!(before, after, "heal must touch ONLY the Exec line");

    // 4. Idempotent + convergent: a second heal is NotNeeded.
    assert_eq!(heal_if_broken_at(&v2), HealOutcome::NotNeeded);

    // 5. Never steal (D1): entry healthy at v2; a process running from ANOTHER live copy must not
    //    repoint it.
    let elsewhere = make_exe(bin.path(), "other-copy");
    assert_eq!(heal_if_broken_at(&elsewhere), HealOutcome::NotNeeded);
    assert_healthy_at(&read_stored_target(Some(&v2)), &v2);

    // 6. Hidden=true (D20): platform OFF respected — heal repairs the POINTER only, intent stays off.
    let hidden_ini = healed_ini.replace("Terminal=false", "Terminal=false\nHidden=true");
    std::fs::write(&desktop, &hidden_ini).unwrap();
    assert!(
        !intent_enabled_now().expect("intent readable"),
        "Hidden=true must read as intent OFF"
    );
    std::fs::remove_file(&v2).unwrap();
    assert!(matches!(
        heal_if_broken_at(&elsewhere),
        HealOutcome::Healed { .. }
    ));
    let after_hidden = std::fs::read_to_string(&desktop).unwrap();
    assert!(
        after_hidden.contains("Hidden=true"),
        "heal must never clear a platform OFF"
    );
    assert!(!intent_enabled_now().unwrap());

    // 7. Unreadable never writes: duplicate Exec ⇒ Skipped, file untouched.
    let dup = format!("{after_hidden}Exec=/decoy\n");
    std::fs::write(&desktop, &dup).unwrap();
    assert_eq!(
        heal_if_broken_at(&elsewhere),
        HealOutcome::Skipped("artifact unreadable")
    );
    assert_eq!(std::fs::read_to_string(&desktop).unwrap(), dup);

    // 7b. …and an unreadable artifact must still leave the user BOTH controls. The first fix for
    // this only stopped `status()` returning Err on the classification, while `intent_enabled`
    // still re-parsed the file underneath and Err'd anyway — so assert the tolerant path directly,
    // on artifacts that no parser can read. (The Playwright twin mocks the status object and is
    // structurally unable to catch this.)
    for hostile in [
        &b"\xff\xfe not valid utf-8 at all"[..],
        b"[Desktop Entry]\nExec=\n",
        b"total garbage, no groups",
    ] {
        std::fs::write(&desktop, hostile).unwrap();
        assert!(
            intent_enabled_now().is_ok(),
            "an unparseable artifact must not make intent unreadable"
        );
        assert!(
            intent_enabled_now().unwrap(),
            "present-but-unparseable still means the user asked for autostart"
        );
        assert!(matches!(
            read_stored_target(Some(&elsewhere)),
            StoredTarget::Unreadable { .. } | StoredTarget::Broken { .. }
        ));
    }

    // 8. OFF removes; Absent never resurrects (§9 "never resurrect").
    remove_entry().expect("remove");
    assert!(!desktop.exists());
    assert!(matches!(
        read_stored_target(Some(&elsewhere)),
        StoredTarget::Absent
    ));
    assert_eq!(heal_if_broken_at(&elsewhere), HealOutcome::NotNeeded);
    assert!(!desktop.exists(), "a heal must NEVER create an entry");
    remove_entry().expect("OFF is idempotent");
}

// ─────────────────────────────────────────────────────────────────────────────
// Linux — L4 hermetic (Docker) legs. The lifecycle test above pins its OWN env, so re-running it
// in a container would prove nothing about env handling; these two read the env AS GIVEN and are
// therefore meaningful only under scripts/autostart-test.sh, which passes a DIVERGENT
// XDG_CONFIG_HOME vs HOME (impossible to prove on a dev host, where they coincide) and a second
// invocation with HOME/XDG unset entirely. Outside the harness they self-skip loudly.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
#[test]
#[ignore = "L4 container harness only (AZTEC_AUTOSTART_HERMETIC=1 + divergent XDG); vacuous elsewhere"]
fn linux_hermetic_xdg_divergence() {
    if std::env::var("AZTEC_AUTOSTART_HERMETIC").is_err() {
        eprintln!("SKIPPED (vacuous pass): not under the L4 harness — run bun run --cwd packages/accelerator test:autostart");
        return;
    }
    let xdg =
        PathBuf::from(std::env::var("XDG_CONFIG_HOME").expect("harness sets XDG_CONFIG_HOME"));
    let home = PathBuf::from(std::env::var("HOME").expect("harness sets HOME"));
    assert_ne!(
        xdg,
        home.join(".config"),
        "harness must pass DIVERGENT dirs or this proves nothing"
    );

    let bin = tempfile::tempdir().expect("bin dir");
    let v1 = make_exe(bin.path(), "app-v1");
    let v2 = make_exe(bin.path(), "app-v2");

    // D9: everything must land under XDG_CONFIG_HOME — the removed plugin hardcoded $HOME/.config,
    // and a reader watching the wrong dir is invisible outside a container.
    enable_entry_at(&v1).expect("enable");
    let right = xdg.join("autostart/Aztec Accelerator.desktop");
    let wrong = home.join(".config/autostart/Aztec Accelerator.desktop");
    assert!(
        right.exists(),
        "artifact must live under XDG_CONFIG_HOME (D9)"
    );
    assert!(
        !wrong.exists(),
        "nothing may be written under the $HOME/.config decoy"
    );

    std::fs::remove_file(&v1).unwrap();
    assert!(matches!(heal_if_broken_at(&v2), HealOutcome::Healed { .. }));
    assert_healthy_at(&read_stored_target(Some(&v2)), &v2);
    assert!(
        !wrong.exists(),
        "the heal must not touch the decoy dir either"
    );
    remove_entry().expect("remove");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "L4 container harness only (no-passwd uid + HOME/XDG unset); vacuous elsewhere"]
fn linux_hermetic_no_home_is_graceful() {
    if std::env::var("AZTEC_AUTOSTART_HERMETIC").is_err() {
        eprintln!("SKIPPED (vacuous pass): not under the L4 harness");
        return;
    }
    // C8's REAL trigger is narrower than "HOME unset": `dirs` falls back to getpwuid, so home
    // resolution only fails when the uid has NO passwd entry either (the k8s/random-uid container
    // case). The harness runs this leg as `docker run --user 12345:12345` with HOME/XDG scrubbed;
    // anywhere home still resolves, asserting Err would be wrong — skip loudly instead. (First
    // harness draft asserted Err under a mere `env -u HOME` and promptly wrote a REAL .desktop
    // into the invoking user's profile via the getpwuid fallback — lesson logged.)
    if std::env::var_os("HOME").is_some()
        || std::env::var_os("XDG_CONFIG_HOME").is_some()
        || dirs::home_dir().is_some()
    {
        eprintln!(
            "SKIPPED (vacuous pass): home still resolves (env or passwd) — this leg needs the \
             harness's no-passwd uid"
        );
        return;
    }
    // The removed plugin's `dirs::home_dir().unwrap()` PANICKED exactly here. Every owned
    // operation must degrade to an error or a skip — never abort.
    let bin = tempfile::tempdir().expect("bin dir");
    let live = make_exe(bin.path(), "app");
    assert!(
        enable_entry_at(&live).is_err(),
        "enable with unresolvable home must Err, not panic"
    );
    assert!(
        intent_enabled_now().is_err(),
        "intent with unresolvable home must Err, not panic"
    );
    assert!(
        matches!(
            heal_if_broken_at(&live),
            HealOutcome::Skipped(_) | HealOutcome::Failed(_)
        ),
        "heal with unresolvable home must skip/fail, not panic"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// macOS
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
#[ignore = "real-OS integration: writes a LaunchAgent plist under a throwaway $HOME; CI runs with --ignored"]
fn macos_heal_preserves_keepalive_through_a_real_patch() {
    let home = tempfile::tempdir().expect("temp HOME");
    // SAFETY: single #[test] in this binary on this OS.
    std::env::set_var("HOME", home.path());

    let bin = tempfile::tempdir().expect("bin dir");
    let spaced_dir = bin.path().join("Aztec Accelerator.app");
    std::fs::create_dir_all(&spaced_dir).unwrap();
    let v1 = make_exe(&spaced_dir, "app-v1");
    let v2 = make_exe(&spaced_dir, "app-v2");

    // 1. Enable, then arm crash recovery the way production does: patch KeepAlive INTO the same
    //    plist (crash_recovery's patch inserts before the last </dict>; emulated structurally here
    //    because enable_impl is private — same file, same keys).
    enable_entry_at(&v1).expect("enable");
    let plist_path = home
        .path()
        .join("Library/LaunchAgents")
        .join("Aztec Accelerator.plist");
    let bytes = std::fs::read(&plist_path).expect("plist written");
    let mut root = plist::Value::from_reader(std::io::Cursor::new(&bytes))
        .expect("our own enable artifact must parse")
        .into_dictionary()
        .expect("dict root");
    let mut keepalive = plist::Dictionary::new();
    keepalive.insert("SuccessfulExit".into(), plist::Value::Boolean(false));
    root.insert("KeepAlive".into(), plist::Value::Dictionary(keepalive));
    root.insert("ThrottleInterval".into(), plist::Value::Integer(5.into()));
    plist::Value::Dictionary(root)
        .to_file_xml(&plist_path)
        .expect("write armed plist");

    // 2. Break the target → heal → the C1 assertion: KeepAlive/ThrottleInterval SURVIVE the patch.
    std::fs::remove_file(&v1).unwrap();
    assert!(matches!(
        read_stored_target(Some(&v2)),
        StoredTarget::Broken { .. }
    ));
    assert!(matches!(heal_if_broken_at(&v2), HealOutcome::Healed { .. }));

    let healed = plist::Value::from_file(&plist_path).expect("healed plist parses");
    let dict = healed.as_dictionary().expect("dict");
    assert!(
        dict.get("KeepAlive").is_some(),
        "C1: KeepAlive must survive the heal"
    );
    assert!(
        dict.get("ThrottleInterval").is_some(),
        "C1: ThrottleInterval must survive"
    );
    assert_eq!(
        dict.get("RunAtLoad").and_then(|v| v.as_boolean()),
        Some(true),
        "RunAtLoad must survive"
    );
    assert_healthy_at(&read_stored_target(Some(&v2)), &v2);
    assert!(intent_enabled_now().expect("intent readable"));

    // 3. launchd `Disabled` (D20): intent OFF, heal never clears it.
    let mut root = plist::Value::from_file(&plist_path)
        .unwrap()
        .into_dictionary()
        .unwrap();
    root.insert("Disabled".into(), plist::Value::Boolean(true));
    plist::Value::Dictionary(root)
        .to_file_xml(&plist_path)
        .unwrap();
    assert!(
        !intent_enabled_now().unwrap(),
        "Disabled=true must read as intent OFF"
    );
    std::fs::remove_file(&v2).unwrap();
    let elsewhere = make_exe(bin.path(), "other");
    assert!(matches!(
        heal_if_broken_at(&elsewhere),
        HealOutcome::Healed { .. }
    ));
    let d = plist::Value::from_file(&plist_path).unwrap();
    assert_eq!(
        d.as_dictionary()
            .and_then(|d| d.get("Disabled"))
            .and_then(|v| v.as_boolean()),
        Some(true),
        "heal must never clear a platform OFF"
    );

    // 4. Absent never resurrects.
    remove_entry().expect("remove");
    assert_eq!(heal_if_broken_at(&elsewhere), HealOutcome::NotNeeded);
    assert!(!plist_path.exists(), "a heal must NEVER create an entry");
}

// ─────────────────────────────────────────────────────────────────────────────
// Windows — real HKCU. A throwaway $HOME does NOT isolate the registry, so everything runs inside
// ONE test with an RAII guard that snapshots and restores the REAL Run + StartupApproved values
// (panic-safe: Drop runs on unwind). CI runners are throwaway; local runs restore the dev machine.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(windows)]
mod win {
    use super::*;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
    use winreg::RegKey;

    pub const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
    pub const SA_KEY: &str =
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    pub const VALUE: &str = "Aztec Accelerator";

    pub struct RegRestore {
        run: Option<String>,
        sa: Option<winreg::RegValue>,
    }

    impl RegRestore {
        pub fn capture() -> Self {
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            let run = hkcu
                .open_subkey_with_flags(RUN_KEY, KEY_READ)
                .ok()
                .and_then(|k| k.get_value::<String, _>(VALUE).ok());
            let sa = hkcu
                .open_subkey_with_flags(SA_KEY, KEY_READ)
                .ok()
                .and_then(|k| k.get_raw_value(VALUE).ok());
            Self { run, sa }
        }
    }

    impl Drop for RegRestore {
        fn drop(&mut self) {
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            if let Ok(k) = hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE) {
                match &self.run {
                    Some(v) => {
                        let _ = k.set_value(VALUE, v);
                    }
                    None => {
                        let _ = k.delete_value(VALUE);
                    }
                }
            }
            if let Ok(k) = hkcu.open_subkey_with_flags(SA_KEY, KEY_SET_VALUE) {
                match &self.sa {
                    Some(v) => {
                        let _ = k.set_raw_value(VALUE, v);
                    }
                    None => {
                        let _ = k.delete_value(VALUE);
                    }
                }
            }
        }
    }

    pub fn write_raw_run_value(v: &str) {
        // create_subkey: a runner/profile that has never had a startup entry has no Run key at
        // all (this is exactly how the production open-instead-of-create bug surfaced).
        RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(RUN_KEY)
            .expect("open Run")
            .0
            .set_value(VALUE, &v.to_string())
            .expect("seed Run value");
    }

    pub fn read_raw_run_value() -> Option<String> {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN_KEY, KEY_READ)
            .ok()
            .and_then(|k| k.get_value::<String, _>(VALUE).ok())
    }

    /// Spawn `command_line` EXACTLY as Run-key processing does — CreateProcessW with
    /// lpApplicationName = NULL — and return the image path of the process that actually started.
    /// This is the execute-the-value proof (audit r4): the quoting claim measured against the OS,
    /// not against our own `run_value_candidates` model.
    pub fn createprocess_image(command_line: &str) -> Option<String> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            CreateProcessW, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
            CREATE_NO_WINDOW, PROCESS_INFORMATION, STARTUPINFOW,
        };

        let mut cl: Vec<u16> = command_line
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            CreateProcessW(
                std::ptr::null(), // lpApplicationName NULL ⇒ the Run-key parsing rules apply
                cl.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_NO_WINDOW,
                std::ptr::null(),
                std::ptr::null(),
                &si,
                &mut pi,
            )
        };
        if ok == 0 {
            return None;
        }
        let mut buf = [0u16; 4096];
        let mut len = buf.len() as u32;
        let name_ok =
            unsafe { QueryFullProcessImageNameW(pi.hProcess, 0, buf.as_mut_ptr(), &mut len) };
        let image = (name_ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]));
        unsafe {
            TerminateProcess(pi.hProcess, 0);
            WaitForSingleObject(pi.hProcess, 5_000);
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
        }
        image
    }
}

#[cfg(windows)]
#[test]
#[ignore = "real-OS integration: writes the REAL HKCU Run value (RAII-restored); CI runs with --ignored"]
fn windows_full_lifecycle_quoting_heal_and_createprocess_proof() {
    use win::*;

    let _restore = RegRestore::capture(); // panic-safe: restores the dev machine / runner state

    // NOTE: no HOME/USERPROFILE override here — it would be theatre. `dirs::home_dir()` on Windows
    // calls SHGetKnownFolderPath(FOLDERID_Profile) (dirs-6 → dirs-sys-0.5) and ignores both env
    // vars, so the lock files genuinely live in the real profile. That is harmless: they are
    // advisory locks held for one read-modify-write, and the registry state this test mutates is
    // restored by the RAII guard above. On a dev box running the installed app, a concurrent
    // update transaction can make the heal return Skipped("updater active") — self-explanatory if
    // it ever happens; CI runners are throwaway and single-tenant.

    // A REAL executable in a spaced dir, plus a decoy at the CreateProcess prefix position.
    let bin = tempfile::tempdir().expect("bin dir");
    let spaced = bin.path().join("Aztec Accelerator");
    std::fs::create_dir_all(&spaced).unwrap();
    let probe_src =
        std::path::PathBuf::from(std::env::var("SystemRoot").unwrap()).join("System32\\where.exe");
    let probe = spaced.join("Aztec Accelerator.exe");
    std::fs::copy(&probe_src, &probe).expect("copy probe exe");
    let decoy = bin.path().join("Aztec.exe"); // the §9 hijack position for the unquoted value
    std::fs::copy(&probe_src, &decoy).expect("copy decoy exe");

    // 0. Fresh-profile regression: a machine that has never had a startup entry has NO Run key at
    //    all, and merely OPENING it fails with NotFound. That made enable impossible and made a
    //    plain absent entry read as unreadable. Delete the key outright and prove both paths cope
    //    (this is how the bug surfaced — one CI runner had the key, the next did not).
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey(RUN_KEY);
        assert!(
            matches!(read_stored_target(Some(&probe)), StoredTarget::Absent),
            "a missing Run key must read as Absent, never Unreadable"
        );
        assert!(!intent_enabled_now().expect("intent readable without a Run key"));
        remove_entry().expect("OFF with no Run key must be a no-op, not an error");
    }

    // 1. Enable writes the QUOTED value and it classifies Healthy — creating the key if needed.
    enable_entry_at(&probe).expect("enable");
    let value = read_raw_run_value().expect("Run value written");
    assert_eq!(
        value,
        format!("\"{}\"", probe.display()),
        "enable-time value must be quoted (§9)"
    );
    assert_healthy_at(&read_stored_target(Some(&probe)), &probe);
    assert!(intent_enabled_now().expect("intent readable"));

    // 2. THE EXECUTE-THE-VALUE PROOF (audit r4). Quoted: CreateProcessW must start the probe —
    //    NOT the decoy — even with the decoy planted at the hijack position.
    let image = createprocess_image(&value).expect("quoted value must launch");
    assert!(
        image.eq_ignore_ascii_case(&probe.to_string_lossy()),
        "quoted Run value must launch the probe, launched {image}"
    );

    // 3. The negative: the legacy UNQUOTED value launches the DECOY — the live hijack the quoting
    //    fix closes, demonstrated on real CreateProcess, not our model.
    let unquoted = format!("{} ", probe.display());
    let hijacked = createprocess_image(&unquoted).expect("unquoted value launches SOMETHING");
    assert!(
        hijacked.eq_ignore_ascii_case(&decoy.to_string_lossy()),
        "unquoted value must demonstrate the prefix hijack, launched {hijacked}"
    );

    // 4. Stale spaced UNQUOTED value (what the removed plugin wrote) with a dead target and no
    //    decoy on disk → Broken → heal writes the exactly quoted live path.
    std::fs::remove_file(&decoy).unwrap();
    std::fs::remove_file(&probe).unwrap();
    let v2 = spaced.join("Aztec Accelerator v2.exe");
    std::fs::copy(&probe_src, &v2).expect("copy v2");
    write_raw_run_value(&unquoted);
    assert!(matches!(
        read_stored_target(Some(&v2)),
        StoredTarget::Broken { .. }
    ));
    assert!(matches!(heal_if_broken_at(&v2), HealOutcome::Healed { .. }));
    assert_eq!(
        read_raw_run_value().unwrap(),
        format!("\"{}\"", v2.display()),
        "healed value must be exactly quoted"
    );

    // 5. StartupApproved OFF (D20/§4.5): heal repairs the pointer, never the override; explicit
    //    enable resets it (auto-launch parity).
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
        use winreg::{RegKey, RegValue};
        let disabled = RegValue {
            vtype: winreg::enums::RegType::REG_BINARY,
            bytes: vec![
                0x03, 0, 0, 0, 0x9a, 0xde, 0x9f, 0x3e, 0x9c, 0x5c, 0xd9, 0x01,
            ],
        };
        RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(SA_KEY)
            .expect("open StartupApproved")
            .0
            .set_raw_value(VALUE, &disabled)
            .expect("seed disabled blob");
    }
    assert!(
        !intent_enabled_now().unwrap(),
        "Task-Manager OFF must read as intent OFF"
    );
    std::fs::remove_file(&v2).unwrap();
    let v3 = spaced.join("Aztec Accelerator v3.exe");
    std::fs::copy(&probe_src, &v3).expect("copy v3");
    assert!(matches!(heal_if_broken_at(&v3), HealOutcome::Healed { .. }));
    assert!(
        !intent_enabled_now().unwrap(),
        "heal must never clear a Task-Manager OFF"
    );
    enable_entry_at(&v3).expect("explicit enable");
    assert!(
        intent_enabled_now().unwrap(),
        "explicit ON resets StartupApproved"
    );

    // 6. OFF deletes the Run value but leaves StartupApproved (§4.5); Absent never resurrects;
    //    OFF is idempotent (the removed plugin errored on an already-absent value).
    remove_entry().expect("remove");
    assert_eq!(read_raw_run_value(), None);
    assert_eq!(heal_if_broken_at(&v3), HealOutcome::NotNeeded);
    assert_eq!(
        read_raw_run_value(),
        None,
        "a heal must NEVER create an entry"
    );
    remove_entry().expect("OFF is idempotent");
}
