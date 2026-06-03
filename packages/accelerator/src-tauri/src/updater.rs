//! Auto-update logic shared between the Tauri app (main.rs) and commands.
//!
//! The background loop in main.rs calls `check_for_update()` periodically.
//! When the user clicks "Update Now" in the prompt, `respond_update_prompt`
//! calls `perform_update()` directly — no redundant network re-check.

use crate::commands::ConfigState;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

/// Check for updates and act based on the user's auto_update preference.
/// Returns the `Update` if one is available and the user hasn't opted into auto-update
/// (so the caller can show a prompt or store it for later use).
pub async fn check_for_update(
    app: &AppHandle,
    config_state: &ConfigState,
) -> Option<tauri_plugin_updater::Update> {
    tracing::info!("Checking for updates...");
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!("Failed to build updater: {e}");
            return None;
        }
    };

    let update = match updater.check().await {
        Ok(Some(update)) => update,
        Ok(None) => {
            tracing::info!("No update available");
            return None;
        }
        Err(e) => {
            tracing::warn!("Update check failed: {e}");
            return None;
        }
    };

    let new_version = update.version.clone();
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    tracing::info!(current = %current_version, new = %new_version, "Update available");

    let auto_update_pref = { config_state.read().auto_update };
    tracing::info!(?auto_update_pref, "Auto-update preference");

    match auto_update_pref {
        Some(true) => {
            tracing::info!("Auto-update enabled, performing update");
            perform_update(app, update).await;
            None
        }
        _ => {
            // None (never asked) or Some(false) (manual) — return the update
            // so the caller can show a prompt or add a tray menu item
            Some(update)
        }
    }
}

/// Download, verify Ed25519 signature, install, and restart the app.
pub async fn perform_update(app: &AppHandle, update: tauri_plugin_updater::Update) {
    tracing::info!(version = %update.version, "Downloading update");

    match update
        .download_and_install(
            |chunk_length, content_length| {
                tracing::info!(
                    chunk_length,
                    content_length = content_length.unwrap_or(0),
                    "Download progress"
                );
            },
            || {
                tracing::info!("Download complete, installing");
                // Windows: disarm the always-armed repeating crash-recovery task ONLY now —
                // right before NSIS mutates the install tree. A tick during that window could
                // spawn the exe mid-update (lock the file being replaced / launch a half-written
                // binary). Disarming here (not before the download) keeps a mid-download crash
                // recoverable. No-op if the task was never armed (autostart off).
                #[cfg(target_os = "windows")]
                crate::crash_recovery::disable_crash_recovery();
            },
        )
        .await
    {
        Ok(()) => {
            // Re-arm BEFORE restarting: a failed relaunch must not leave recovery off while
            // autostart is on. IgnoreNew + the exit-0-if-healthy guard absorb any brief
            // double-launch with the restarted build.
            #[cfg(target_os = "windows")]
            rearm_crash_recovery_if_enabled(app);
            tracing::info!("Update installed, restarting");
            app.restart();
        }
        Err(e) => {
            tracing::error!("Update failed: {e}");
            // The app keeps running, so crash-recovery must resume (only if it should be
            // armed — autostart on). Idempotent if it was never disarmed (crash mid-download).
            #[cfg(target_os = "windows")]
            rearm_crash_recovery_if_enabled(app);
        }
    }
}

/// Re-arm the Windows crash-recovery task iff it should be armed (autostart on). Idempotent —
/// `enable_crash_recovery` overwrites any existing task.
#[cfg(target_os = "windows")]
fn rearm_crash_recovery_if_enabled(app: &AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    if app.autolaunch().is_enabled().unwrap_or(false) {
        crate::crash_recovery::enable_crash_recovery();
    }
}
