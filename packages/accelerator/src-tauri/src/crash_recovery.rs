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

#[cfg(test)]
mod tests {
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
