//! Platform-specific crash recovery.
//!
//! - **macOS**: Patches the LaunchAgent plist to add `KeepAlive` + `ThrottleInterval`,
//!   so launchd restarts the app if it crashes.
//! - **Linux**: Manages a systemd user service with `Restart=on-failure`.

const APP_NAME: &str = "aztec-accelerator";

/// Patch the LaunchAgent plist created by tauri-plugin-autostart to add crash recovery keys.
/// Call this after `manager.enable()`.
#[cfg(target_os = "macos")]
pub fn enable_crash_recovery() {
    let plist_path = macos_plist_path();
    match std::fs::read_to_string(&plist_path) {
        Ok(content) => {
            if content.contains("<key>KeepAlive</key>") {
                tracing::debug!("LaunchAgent already has KeepAlive");
                return;
            }
            // Insert KeepAlive + ThrottleInterval before the closing </dict>
            let patched = content.replace(
                "</dict>",
                "    <key>KeepAlive</key>\n    \
                 <dict>\n        \
                 <key>SuccessfulExit</key>\n        \
                 <false/>\n    \
                 </dict>\n    \
                 <key>ThrottleInterval</key>\n    \
                 <integer>5</integer>\n  \
                 </dict>",
            );
            if let Err(e) = std::fs::write(&plist_path, &patched) {
                tracing::warn!("Failed to patch LaunchAgent plist: {e}");
            } else {
                tracing::info!("LaunchAgent patched with KeepAlive (crash recovery)");
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

/// Remove crash recovery keys from the LaunchAgent plist.
/// Call this after `manager.disable()` to clean up.
#[cfg(target_os = "macos")]
pub fn disable_crash_recovery() {
    // The plugin recreates the plist from scratch on enable(), so disabling
    // just means the standard disable() removes the plist entirely. Nothing extra needed.
    tracing::debug!("macOS crash recovery disabled (plist removed by plugin)");
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

    let service_path = service_dir.join(format!("{APP_NAME}.service"));
    let service_content = format!(
        "[Unit]\n\
         Description=Aztec Accelerator\n\
         After=default.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exe}\n\
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
        .args(["--user", "enable", APP_NAME])
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
        .args(["--user", "disable", APP_NAME])
        .output();
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();

    if let Some(config_dir) = dirs::config_dir() {
        let service_path = config_dir.join(format!("systemd/user/{APP_NAME}.service"));
        let _ = std::fs::remove_file(&service_path);
    }

    tracing::info!("systemd user service disabled (crash recovery)");
}
