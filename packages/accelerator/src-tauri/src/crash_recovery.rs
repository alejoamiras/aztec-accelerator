//! Platform-specific crash recovery.
//!
//! - **macOS**: Patches the LaunchAgent plist to add `KeepAlive` + `ThrottleInterval`,
//!   so launchd restarts the app if it crashes.
//! - **Linux**: Manages a systemd user service with `Restart=on-failure`.
//!
//! The per-platform logic lives behind the [`CrashRecovery`] trait, implemented by the
//! platform-specific `PlatformRecovery` ZST. The `enable_crash_recovery` / `disable_crash_recovery`
//! free functions are thin dispatch onto it, so callers stay platform-agnostic and the surface is
//! mockable in tests.

/// Platform crash-recovery control. `disable` returns whether the recovery mechanism is confirmed
/// disarmed — always `true` where disarm is unconditional (macOS/Linux), the real /Query-verified
/// result on Windows (where the updater MUST know the always-armed task is gone before NSIS mutates
/// files).
pub trait CrashRecovery {
    fn enable(&self);
    fn disable(&self) -> bool;
}

/// Enable crash recovery for the current platform (thin dispatch to the platform `CrashRecovery`).
pub fn enable_crash_recovery() {
    PlatformRecovery.enable();
}

/// Disable crash recovery. See [`CrashRecovery::disable`] for the `bool` contract — callers that must
/// know the recovery is gone (the updater, before install) check it.
pub fn disable_crash_recovery() -> bool {
    PlatformRecovery.disable()
}

/// The current platform's crash-recovery implementation. A unit struct — the actual state lives in
/// the OS (launchd plist / systemd unit / Task Scheduler task). Dispatches to the `#[cfg]`-selected
/// `enable_impl` / `disable_impl`.
pub struct PlatformRecovery;

impl CrashRecovery for PlatformRecovery {
    fn enable(&self) {
        enable_impl();
    }
    fn disable(&self) -> bool {
        disable_impl()
    }
}

/// Must match `productName` in tauri.conf.json — the auto-launch crate uses this
/// (not the identifier) as the LaunchAgent plist filename.
#[cfg(target_os = "macos")]
const APP_NAME: &str = "Aztec Accelerator";

/// Hyphenated name for systemd unit files (spaces break `systemctl` arguments).
#[cfg(target_os = "linux")]
const SYSTEMD_NAME: &str = "aztec-accelerator";

/// Patch the LaunchAgent plist created by tauri-plugin-autostart to add crash recovery keys.
/// Call this after `manager.enable()`.
///
/// Inserts KeepAlive + ThrottleInterval before the LAST `</dict>` (the top-level one).
/// Previous implementation used `.replace("</dict>", ...)` which replaced ALL occurrences
/// and could corrupt plists with nested dicts.
#[cfg(target_os = "macos")]
fn enable_impl() {
    let plist_path = macos_plist_path();
    match std::fs::read_to_string(&plist_path) {
        Ok(content) => {
            if content.contains("<key>KeepAlive</key>") {
                tracing::debug!("LaunchAgent already has KeepAlive");
                return;
            }
            match patch_plist_with_keepalive(&content) {
                Some(patched) => {
                    if let Err(e) = std::fs::write(&plist_path, &patched) {
                        tracing::warn!("Failed to write patched LaunchAgent plist: {e}");
                    } else {
                        tracing::info!("LaunchAgent patched with KeepAlive (crash recovery)");
                    }
                }
                None => {
                    tracing::warn!("Could not find closing </dict> in LaunchAgent plist");
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                path = %plist_path.display(),
                "Cannot read LaunchAgent plist (not yet enabled?): {e}"
            );
        }
    }
}

/// Insert KeepAlive and ThrottleInterval keys before the last `</dict>` in a plist string.
/// Returns None if no `</dict>` is found.
#[cfg(target_os = "macos")]
fn patch_plist_with_keepalive(content: &str) -> Option<String> {
    let insert_pos = content.rfind("</dict>")?;
    let keep_alive = "\
    <key>KeepAlive</key>\n\
    <dict>\n\
        <key>SuccessfulExit</key>\n\
        <false/>\n\
    </dict>\n\
    <key>ThrottleInterval</key>\n\
    <integer>5</integer>\n  ";
    let mut patched = String::with_capacity(content.len() + keep_alive.len());
    patched.push_str(&content[..insert_pos]);
    patched.push_str(keep_alive);
    patched.push_str(&content[insert_pos..]);
    Some(patched)
}

/// Remove crash recovery keys from the LaunchAgent plist.
/// Call this after `manager.disable()` to clean up.
#[cfg(target_os = "macos")]
fn disable_impl() -> bool {
    // The plugin recreates the plist from scratch on enable(), so disabling
    // just means the standard disable() removes the plist entirely. Nothing extra needed.
    tracing::info!("macOS crash recovery disabled (plist removed by plugin)");
    true
}

#[cfg(target_os = "macos")]
fn macos_plist_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~"))
        .join("Library/LaunchAgents")
        .join(format!("{APP_NAME}.plist"))
}

/// F-010: conservative cross-platform preflight for enabling autostart. Rejects a path whose bytes could
/// INJECT into any OS launcher serializer (systemd unit / `.desktop` / plist XML / Windows Run-key): a
/// non-absolute path, non-UTF-8 (systemd rejects it; the plugin serializes it lossily), or any control /
/// newline / DEL byte (the injection vector for line/element-based unit + plist formats). Platform-specific
/// NON-injection formatting quirks in the third-party `auto-launch` crate (e.g. space-splitting in a raw
/// `.desktop` `Exec=` / Run-key) are a documented ROBUSTNESS residual, not a same-process injection we can
/// close without patching that crate. `set_autostart` calls this BEFORE invoking the plugin, refusing (and
/// disabling) rather than letting an unsafe path be serialized.
pub fn autostart_path_is_safe(exe: &std::path::Path) -> bool {
    match exe.to_str() {
        None => false, // non-UTF-8
        Some(s) => exe.is_absolute() && !s.bytes().any(|b| b < 0x20 || b == 0x7f),
    }
}

/// F-010: serialize an absolute executable path into a safe systemd `ExecStart` value, or `None` if the
/// path is not representable. systemd's `string_is_safe` REJECTS a decoded executable containing controls,
/// `\`, `"`, `'`, or a glob introducer (`*`/`?`/`[`), and requires valid UTF-8 — those are not escapable, so
/// we fail closed. The returned value uses the `:` prefix (disables systemd `$`-environment expansion) INSIDE
/// the quoted first token, and doubles `%` (systemd specifier). No `\`/`"` escaping is needed because they
/// are rejected. Result form: `":/path/with %% doubled"`.
#[cfg(target_os = "linux")]
fn systemd_exec_start(exe: &std::path::Path) -> Option<String> {
    let s = exe.to_str()?; // None ⇒ non-UTF-8
    if !exe.is_absolute() || s.ends_with('/') {
        return None; // must be an absolute file path, not a directory shape
    }
    if s.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return None; // controls / newline / DEL
    }
    if s.chars()
        .any(|c| matches!(c, '\\' | '"' | '\'' | '*' | '?' | '['))
    {
        return None; // systemd `string_is_safe` forbids these in the executable path
    }
    Some(format!("\":{}\"", s.replace('%', "%%")))
}

/// Create and enable a systemd user service with `Restart=on-failure`.
/// Call this after `manager.enable()`.
#[cfg(target_os = "linux")]
fn enable_impl() {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Cannot determine executable path for systemd service: {e}");
            return;
        }
    };

    let service_dir = match dirs::config_dir() {
        Some(d) => d.join("systemd/user"),
        None => {
            tracing::warn!("Cannot determine config dir for systemd service");
            return;
        }
    };

    if let Err(e) = std::fs::create_dir_all(&service_dir) {
        tracing::warn!("Cannot create systemd user dir: {e}");
        return;
    }

    // F-010: build a systemd-escaped ExecStart. An unsafe path (systemd would reject or it could inject
    // a directive) fails CLOSED — remove any stale unit and bail rather than write a corrupt/injected one.
    let exec_start = match systemd_exec_start(&exe) {
        Some(v) => v,
        None => {
            tracing::warn!(
                "Executable path is not representable as a safe systemd ExecStart; \
                 skipping crash-recovery unit and removing any stale one"
            );
            disable_impl();
            return;
        }
    };

    let service_path = service_dir.join(format!("{SYSTEMD_NAME}.service"));
    let service_content = format!(
        "[Unit]\n\
         Description=Aztec Accelerator\n\
         After=default.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exec_start}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         StartLimitBurst=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
    );

    if let Err(e) = std::fs::write(&service_path, &service_content) {
        tracing::warn!("Failed to write systemd service: {e}");
        return;
    }

    // Reload and enable
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
    let result = std::process::Command::new("systemctl")
        .args(["--user", "enable", SYSTEMD_NAME])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            tracing::info!("systemd user service enabled (crash recovery)");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("systemctl enable failed: {stderr}");
        }
        Err(e) => {
            tracing::warn!("Failed to run systemctl: {e}");
        }
    }
}

/// Disable and remove the systemd user service.
/// Call this after `manager.disable()`.
#[cfg(target_os = "linux")]
fn disable_impl() -> bool {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", SYSTEMD_NAME])
        .output();

    // F-010: remove the unit file BEFORE the final daemon-reload (so the reload reflects the removal), and
    // report whether disarm is CONFIRMED — a missing file is success; a file that cannot be removed is not.
    let mut removed = true;
    if let Some(config_dir) = dirs::config_dir() {
        let service_path = config_dir.join(format!("systemd/user/{SYSTEMD_NAME}.service"));
        if let Err(e) = std::fs::remove_file(&service_path) {
            if service_path.exists() {
                tracing::warn!(
                    "Failed to remove systemd unit (crash recovery not confirmed disarmed): {e}"
                );
                removed = false;
            }
        }
    }

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();

    if removed {
        tracing::info!("systemd user service disabled (crash recovery)");
    }
    removed
}

// ── Windows ──────────────────────────────────────────────────────────────────
//
// A Task Scheduler task with a REPEATING TimeTrigger (every PT1M) + IgnoreNew. Every
// minute Task Scheduler tries to start the app; IgnoreNew makes that a no-op if it's
// already running and a RELAUNCH if it died — so a crash recovers within <=1 min.
//
// Why not `RestartOnFailure`: it was the original design, but it's BROKEN for this —
// it does NOT relaunch on a non-zero/abnormal process exit (proven empirically on a
// windows-2025 runner; see lessons/phase-4.md). It only restarts when the task ENGINE
// fails to start the action, not when the action runs then dies. mac launchd
// (KeepAlive{SuccessfulExit:false}) and linux systemd (Restart=on-failure) genuinely
// key on the exit code; the repeating trigger is the working Windows equivalent.
//
// The repeating trigger relaunches ANYTHING not running, so it can't distinguish an
// intentional quit from a crash. The Quit menu therefore calls disable_crash_recovery()
// (delete this task) BEFORE exiting — see the `"quit"` handler in main.rs. A crash skips
// that path → the task survives → relaunch; a clean quit deletes it first → stays down.
// Logon start is handled by the autostart Run key (tauri-plugin-autostart), not this
// task; the Run-key-vs-tick race is absorbed by the exit-0-if-healthy guard in main.rs.

#[cfg(target_os = "windows")]
const TASK_NAME: &str = "Aztec Accelerator Crash Recovery";

/// Absolute path to schtasks.exe — avoids a bare-name PATH lookup (same defense as the
/// absolute System32 tar.exe in copy-bb.ts: a planted `schtasks` earlier on PATH can't win).
#[cfg(target_os = "windows")]
fn schtasks_exe() -> std::path::PathBuf {
    let system_root = std::env::var("SystemRoot")
        .or_else(|_| std::env::var("windir"))
        .unwrap_or_else(|_| "C:\\Windows".to_string());
    std::path::Path::new(&system_root)
        .join("System32")
        .join("schtasks.exe")
}

/// Register the Task Scheduler crash-recovery task. Call after `manager.enable()`.
#[cfg(target_os = "windows")]
fn enable_impl() {
    use std::io::Write;

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Cannot determine executable path for Task Scheduler: {e}");
            return;
        }
    };

    // schtasks /XML expects UTF-16LE with a BOM.
    let mut bytes = vec![0xFFu8, 0xFE];
    for unit in task_xml(&exe.display().to_string()).encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }

    // Random temp filename (not a predictable %TEMP% path a local user could pre-create or
    // symlink), written + closed before schtasks reads it, auto-deleted when it drops.
    let xml_path = {
        let mut tmp = match tempfile::Builder::new()
            .prefix("aztec-accel-recovery-")
            .suffix(".xml")
            .tempfile()
        {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("Failed to create Task Scheduler XML temp file: {e}");
                return;
            }
        };
        if let Err(e) = tmp.write_all(&bytes) {
            tracing::warn!("Failed to write Task Scheduler XML: {e}");
            return;
        }
        if let Err(e) = tmp.flush() {
            tracing::warn!("Failed to flush Task Scheduler XML: {e}");
            return;
        }
        // Close our handle so schtasks can open it; the file persists until this drops.
        tmp.into_temp_path()
    };

    let result = std::process::Command::new(schtasks_exe())
        .args(["/Create", "/F", "/TN", TASK_NAME, "/XML"])
        .arg(&*xml_path)
        .output();
    // xml_path (TempPath) drops at end of scope → the temp file is removed.

    match result {
        Ok(output) if output.status.success() => {
            tracing::info!("Task Scheduler crash-recovery task registered");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("schtasks /Create failed: {stderr}");
        }
        Err(e) => tracing::warn!("Failed to run schtasks: {e}"),
    }
}

/// Remove the Task Scheduler crash-recovery task. Returns `true` if the task is confirmed gone
/// (removed or already absent), `false` if removal could not be verified — callers that rely on
/// the task being gone (the updater, before NSIS mutates files) MUST check this and not proceed.
#[cfg(target_os = "windows")]
fn disable_impl() -> bool {
    // Deletion is correctness-critical now: the repeating-trigger task relaunches the app a
    // minute after an intentional quit if it survives. So retry, and verify via /Query
    // (locale-independent — it exits non-zero when the task is absent) rather than trusting
    // a single best-effort /Delete whose stderr wording varies by Windows language.
    for attempt in 1..=3 {
        let _ = std::process::Command::new(schtasks_exe())
            .args(["/Delete", "/F", "/TN", TASK_NAME])
            .output();
        let still_present = std::process::Command::new(schtasks_exe())
            .args(["/Query", "/TN", TASK_NAME])
            .output()
            .map(|o| o.status.success())
            // If /Query itself can't run, don't claim the task is gone — assume it may persist
            // so we keep retrying and ultimately report failure rather than a false success.
            .unwrap_or(true);
        if !still_present {
            tracing::info!("Task Scheduler crash-recovery task removed (or absent)");
            return true;
        }
        tracing::warn!("crash-recovery task still present after /Delete (attempt {attempt})");
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    tracing::error!(
        "crash-recovery task could NOT be removed after retries — the app may relaunch after an intentional quit"
    );
    false
}

/// Build the Task Scheduler task definition. The exe path is XML-escaped.
#[cfg(target_os = "windows")]
fn task_xml(exe_path: &str) -> String {
    let exe = xml_escape(exe_path);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Aztec Accelerator crash recovery</Description>
  </RegistrationInfo>
  <Triggers>
    <TimeTrigger>
      <StartBoundary>2024-01-01T00:00:00</StartBoundary>
      <Enabled>true</Enabled>
      <Repetition>
        <Interval>PT1M</Interval>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
    </TimeTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
    </Exec>
  </Actions>
</Task>"#
    )
}

#[cfg(target_os = "windows")]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// F-010: reproduce systemd's ExecStart decode (unquote → strip `:` prefix → specifier-expand `%%`→`%`)
    /// to prove the serializer round-trips exactly to the intended path — i.e. no injection, exact argv0.
    #[cfg(target_os = "linux")]
    fn decode_systemd_exec_start(value: &str) -> String {
        // Our serializer always emits `":<...>"` — one double-quoted token.
        let inner = value
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .expect("quoted token");
        let after_prefix = inner.strip_prefix(':').expect(": prefix"); // strip the exec prefix
        after_prefix.replace("%%", "%") // specifier expansion (only %% appears)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn systemd_exec_start_serializes_and_round_trips() {
        // Plain path.
        let v = super::systemd_exec_start(Path::new("/usr/bin/aztec-accelerator")).unwrap();
        assert_eq!(v, "\":/usr/bin/aztec-accelerator\"");
        assert_eq!(decode_systemd_exec_start(&v), "/usr/bin/aztec-accelerator");
        // The serialized value can NEVER contain a newline (the unit-injection vector) for any accepted path.
        assert!(!v.contains('\n'));
        // A `%`, a space, and a `$` all survive as literals (— `%` doubled, `:` disables `$` expansion).
        let v = super::systemd_exec_start(Path::new("/opt/my app/100% $HOME/bb")).unwrap();
        assert_eq!(v, "\":/opt/my app/100%% $HOME/bb\"");
        assert_eq!(decode_systemd_exec_start(&v), "/opt/my app/100% $HOME/bb");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn systemd_exec_start_rejects_unrepresentable_paths() {
        for bad in [
            "relative/bb",                 // not absolute
            "/dir/",                       // directory shape
            "/x/\nExecStartPre=/bin/evil", // newline injection
            "/x/\tbb",                     // control
            "/x/a\"b",                     // quote (systemd rejects)
            "/x/a\\b",                     // backslash
            "/x/a'b",                      // single quote
            "/x/a*b",                      // glob
            "/x/a?b",
            "/x/a[b",
        ] {
            assert!(
                super::systemd_exec_start(Path::new(bad)).is_none(),
                "should reject {bad:?}"
            );
        }
    }

    #[test]
    fn autostart_preflight_rejects_injection_and_accepts_normal_paths() {
        // A PLATFORM-absolute path is accepted (Windows `is_absolute` needs a drive prefix, so `/usr/...`
        // is NOT absolute there — the test binary runs on Windows CI too).
        #[cfg(unix)]
        let (ok1, ok2, inj_nl, inj_del) = (
            "/usr/bin/aztec-accelerator",
            "/opt/my app/aztec",
            "/x/\nInject",
            "/x/\u{7f}bb",
        );
        #[cfg(windows)]
        let (ok1, ok2, inj_nl, inj_del) = (
            r"C:\Program Files\Aztec\aztec.exe",
            r"C:\my app\aztec.exe", // space + backslash are fine (formatting), not injection
            "C:\\x\\\nInject",
            "C:\\x\\\u{7f}bb",
        );
        assert!(super::autostart_path_is_safe(Path::new(ok1)));
        assert!(super::autostart_path_is_safe(Path::new(ok2)));
        assert!(!super::autostart_path_is_safe(Path::new("relative/bb"))); // not absolute (both platforms)
        assert!(!super::autostart_path_is_safe(Path::new(inj_nl))); // newline injection
        assert!(!super::autostart_path_is_safe(Path::new(inj_del))); // DEL control
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn task_xml_uses_repeating_trigger_and_escapes_exe() {
        let xml = super::task_xml(r"C:\Program Files\A & B\aztec-accelerator.exe");
        // Crash → relaunch is a REPEATING TimeTrigger (every PT1M) + IgnoreNew, proven
        // on a real runner. NOT RestartOnFailure, which does NOT relaunch a dead/crashed
        // process (see the module comment + lessons/phase-4.md).
        assert!(xml.contains("<TimeTrigger>"));
        assert!(xml.contains("<Repetition>"));
        assert!(xml.contains("<Interval>PT1M</Interval>"));
        // Regression guards: the broken mechanism must not come back, and logon-start is
        // the autostart Run key's job (not a LogonTrigger here).
        assert!(
            !xml.contains("<RestartOnFailure>"),
            "RestartOnFailure does not relaunch a crash — regression"
        );
        assert!(
            !xml.contains("<LogonTrigger>"),
            "logon start is the autostart Run key's job, not this task"
        );
        // IgnoreNew = the every-minute tick is a no-op if the app is alive, a relaunch if
        // it died (the exit-0-if-healthy guard in main.rs absorbs the Run-key-vs-tick race).
        assert!(xml.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
        // The raw ampersand must be escaped or the XML is invalid.
        assert!(xml.contains("A &amp; B"));
        assert!(!xml.contains("A & B"));
        // Path text (sans the escaped char) survives.
        assert!(xml.contains(r"C:\Program Files"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn patch_plist_inserts_before_last_dict() {
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>aztec-accelerator</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Applications/Aztec Accelerator.app/Contents/MacOS/aztec-accelerator</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>"#;

        let patched = super::patch_plist_with_keepalive(plist).unwrap();
        assert!(patched.contains("<key>KeepAlive</key>"));
        assert!(patched.contains("<key>ThrottleInterval</key>"));
        assert!(patched.contains("<integer>5</integer>"));
        // Should still have exactly one </plist> and the KeepAlive should be inside the dict
        assert_eq!(patched.matches("</plist>").count(), 1);
        assert_eq!(patched.matches("</dict>").count(), 2); // inner KeepAlive dict + outer
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn patch_plist_handles_nested_dicts() {
        // Plist with a nested dict — the old .replace() would have broken this
        let plist = r#"<dict>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>/usr/bin</string>
    </dict>
    <key>Label</key>
    <string>test</string>
</dict>"#;

        let patched = super::patch_plist_with_keepalive(plist).unwrap();
        assert!(patched.contains("<key>KeepAlive</key>"));
        // The nested EnvironmentVariables dict should be untouched
        assert!(patched.contains("<key>EnvironmentVariables</key>"));
        // KeepAlive should be inserted before the LAST </dict>, not inside the nested one
        let keepalive_pos = patched.find("<key>KeepAlive</key>").unwrap();
        let nested_dict_end = patched.find("<string>/usr/bin</string>").unwrap();
        assert!(
            keepalive_pos > nested_dict_end,
            "KeepAlive should be after the nested dict"
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn patch_plist_returns_none_for_invalid() {
        assert!(super::patch_plist_with_keepalive("not a plist").is_none());
    }
}
