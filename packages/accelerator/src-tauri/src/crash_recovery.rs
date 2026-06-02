//! Platform-specific crash recovery.
//!
//! - **macOS**: Patches the LaunchAgent plist to add `KeepAlive` + `ThrottleInterval`,
//!   so launchd restarts the app if it crashes.
//! - **Linux**: Manages a systemd user service with `Restart=on-failure`.

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
pub fn enable_crash_recovery() {
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
pub fn disable_crash_recovery() {
    // The plugin recreates the plist from scratch on enable(), so disabling
    // just means the standard disable() removes the plist entirely. Nothing extra needed.
    tracing::info!("macOS crash recovery disabled (plist removed by plugin)");
}

#[cfg(target_os = "macos")]
fn macos_plist_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~"))
        .join("Library/LaunchAgents")
        .join(format!("{APP_NAME}.plist"))
}

/// Create and enable a systemd user service with `Restart=on-failure`.
/// Call this after `manager.enable()`.
#[cfg(target_os = "linux")]
pub fn enable_crash_recovery() {
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

    let service_path = service_dir.join(format!("{SYSTEMD_NAME}.service"));
    let service_content = format!(
        "[Unit]\n\
         Description=Aztec Accelerator\n\
         After=default.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart=\"{exe}\"\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         StartLimitBurst=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exe = exe.display()
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
pub fn disable_crash_recovery() {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", SYSTEMD_NAME])
        .output();
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();

    if let Some(config_dir) = dirs::config_dir() {
        let service_path = config_dir.join(format!("systemd/user/{SYSTEMD_NAME}.service"));
        let _ = std::fs::remove_file(&service_path);
    }

    tracing::info!("systemd user service disabled (crash recovery)");
}

// ── Windows ──────────────────────────────────────────────────────────────────
//
// PROVISIONAL mechanism (windows-release-2026-06-02 plan, owner decision):
// a Task Scheduler task with `RestartOnFailure` — a non-zero exit (crash) relaunches
// the app; a clean exit 0 (intentional quit) completes the task and is NOT restarted.
// Mirrors the Linux systemd approach: a separate recovery mechanism alongside the
// autostart entry created by tauri-plugin-autostart (a Run key on Windows).
//
// Known caveat the P4 gating tests must resolve: the Run key AND the task's logon
// trigger both fire at logon (potential double-launch). If the crash-vs-quit or
// updater-handoff tests fail, the sibling-crate watchdog is the documented fallback.

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
pub fn enable_crash_recovery() {
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

/// Remove the Task Scheduler crash-recovery task. Call after `manager.disable()`.
#[cfg(target_os = "windows")]
pub fn disable_crash_recovery() {
    let result = std::process::Command::new(schtasks_exe())
        .args(["/Delete", "/F", "/TN", TASK_NAME])
        .output();
    match result {
        Ok(o) if o.status.success() => {
            tracing::info!("Task Scheduler crash-recovery task removed");
        }
        Ok(_) => tracing::debug!("Task Scheduler task not present (already removed)"),
        Err(e) => tracing::warn!("Failed to run schtasks /Delete: {e}"),
    }
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
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
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
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
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
    #[test]
    #[cfg(target_os = "windows")]
    fn task_xml_has_restart_and_escapes_exe() {
        let xml = super::task_xml(r"C:\Program Files\A & B\aztec-accelerator.exe");
        // Crash → relaunch is the whole point.
        assert!(xml.contains("<RestartOnFailure>"));
        assert!(xml.contains("<LogonTrigger>"));
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
