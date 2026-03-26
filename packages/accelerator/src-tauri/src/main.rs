// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod tray;
mod windows;

use aztec_accelerator::authorization::AuthorizationManager;
use aztec_accelerator::commands::{AuthState, ConfigState, PendingUpdate, SharedAppState};
use aztec_accelerator::server::{AppState, HTTPS_PORT};
use aztec_accelerator::{certs, commands, config, log_dir};
use parking_lot::RwLock;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::menu::MenuItemBuilder;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::MacosLauncher;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Returns true in debug builds (`cargo tauri dev`), false in release.
fn is_dev_mode() -> bool {
    cfg!(debug_assertions)
}

/// Open a path or URL in the platform's default handler.
fn open_in_browser(target: &impl AsRef<Path>) {
    let path = target.as_ref();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer").arg(path).spawn();

    if let Err(e) = result {
        tracing::warn!(path = %path.display(), error = %e, "Failed to open in browser");
    }
}

// ── HTTPS startup ────────────────────────────────────────────────────────

/// Try to start HTTPS server if Safari Support is configured and certs are valid.
/// Uses a clone of the full `AppState` so the HTTPS server has auth, config, and callbacks.
/// Returns the HTTPS port if started, None otherwise.
fn try_start_https(state: &AppState) -> Option<u16> {
    let cfg = config::load();
    if !cfg.safari_support {
        return None;
    }
    if !certs::certs_exist() {
        tracing::warn!("Safari Support enabled but certs missing — resetting config");
        if let Some(ref cfg_lock) = state.config {
            let mut cfg = cfg_lock.write();
            cfg.safari_support = false;
            let _ = config::save(&cfg);
        }
        return None;
    }

    // Auto-renew leaf cert if expiring
    if let Err(e) = certs::regenerate_leaf_if_expiring() {
        tracing::warn!("Failed to check/renew leaf cert: {e}");
    }

    // Verify CA is still trusted (macOS only)
    if !certs::is_ca_trusted() {
        tracing::warn!("CA not trusted in Keychain — skipping HTTPS");
        return None;
    }

    match certs::load_rustls_config() {
        Ok(tls_config) => {
            let mut state_for_https = state.clone();
            state_for_https.https_port = Some(HTTPS_PORT);
            tauri::async_runtime::spawn(async move {
                if let Err(e) =
                    aztec_accelerator::server::start_https(state_for_https, tls_config).await
                {
                    tracing::error!("HTTPS server error: {e}");
                }
            });
            Some(HTTPS_PORT)
        }
        Err(e) => {
            tracing::warn!("Failed to load TLS config: {e} — skipping HTTPS");
            None
        }
    }
}

// ── Auto-update ──────────────────────────────────────────────────────────

/// Background update check wrapper. Calls the shared updater module and
/// shows the prompt window if an update is available and the user hasn't chosen yet.
async fn run_update_check(app: &AppHandle, config_state: &ConfigState) {
    if let Some(update) = aztec_accelerator::updater::check_for_update(app, config_state).await {
        let auto_update_pref = { config_state.read().auto_update };
        let current_version = env!("CARGO_PKG_VERSION").to_string();
        let new_version = update.version.clone();

        // Store the update so respond_update_prompt can use it directly
        if let Some(pending) = app.try_state::<PendingUpdate>() {
            *pending.lock() = Some(update);
        }

        // Show prompt for both None (first time) and Some(false) (manual mode).
        // Some(true) users never reach here — check_for_update auto-installs for them.
        tracing::info!(
            ?auto_update_pref,
            version = %new_version,
            "Showing update prompt"
        );
        windows::show_update_prompt_window(app, &current_version, &new_version);
    }
}

// ── Exit handling ────────────────────────────────────────────────────────

/// Returns true if the exit should be prevented.
/// Window-close events have code=None and should be prevented (tray-only app).
/// Explicit exits (Quit menu, restart) have code=Some(_) and must go through.
fn should_prevent_exit(code: Option<i32>) -> bool {
    code.is_none()
}

// ── Main ─────────────────────────────────────────────────────────────────

fn main() {
    // Install a default rustls CryptoProvider. Both aws-lc-rs (from tauri-plugin-updater)
    // and ring (from tokio-rustls) are available — rustls panics if it can't auto-detect.
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();

    let log_path = log_dir();
    std::fs::create_dir_all(&log_path).ok();

    // Restrict log directory permissions to owner-only on Unix (0o700)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&log_path, std::fs::Permissions::from_mode(0o700));
    }

    let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("accelerator")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&log_path)
        .expect("failed to create log appender");
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stdout))
        .with(fmt::layer().with_writer(file_writer).with_ansi(false))
        .init();

    tracing::info!(log_dir = %log_path.display(), "Logging initialized");

    let dev_mode = is_dev_mode();
    if dev_mode {
        tracing::info!("Developer mode enabled");
    }

    // Load config early so it can be shared with AppState and Tauri commands
    let config_state: ConfigState = Arc::new(RwLock::new(config::load()));
    let auth_manager: AuthState = Arc::new(AuthorizationManager::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(config_state.clone())
        .manage(auth_manager.clone())
        .manage::<PendingUpdate>(Arc::new(parking_lot::Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::get_autostart_enabled,
            commands::set_autostart,
            commands::set_speed,
            commands::remove_approved_origin,
            commands::get_system_info,
            commands::respond_auth,
            commands::enable_safari_support,
            commands::disable_safari_support,
            commands::set_auto_update,
            commands::respond_update_prompt,
        ])
        .setup(move |app| {
            // Hide from Dock — tray-only app
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let bundled_version = env!("AZTEC_BB_VERSION").to_string();

            let status = MenuItemBuilder::with_id("status", "Status: Idle")
                .enabled(false)
                .build(app)?;

            // Check autostart on launch for crash recovery
            {
                use tauri_plugin_autostart::ManagerExt;
                if app.autolaunch().is_enabled().unwrap_or(false) {
                    aztec_accelerator::crash_recovery::enable_crash_recovery();
                }
            }

            // ── Build tray ──
            let menu =
                tray::build_tray_menu(&app.handle().clone(), dev_mode, &bundled_version, &status)?;

            let tray =
                tray::build_tray_icon(app, &menu, move |app, event| match event.id().as_ref() {
                    "quit" => app.exit(0),
                    "show_logs" => open_in_browser(&log_dir()),
                    "open_github" => {
                        open_in_browser(&"https://github.com/alejoamiras/aztec-accelerator");
                    }
                    "settings" => windows::open_settings_window(app),
                    _ => {}
                })?;

            // ── Animation ──
            let is_animating = Arc::new(AtomicBool::new(false));
            tray::start_animation_loop(tray.clone(), app.handle().clone(), is_animating.clone());

            // ── Callbacks and AppState wiring ──
            let status_clone = status.clone();
            let status_for_diagnostics = status.clone();
            let tray_clone = tray.clone();

            // Versions changed callback: rebuild the Versions submenu when versions change.
            let app_handle = app.handle().clone();
            let bundled_for_cb = bundled_version.clone();
            let tray_for_versions = tray.clone();
            let on_versions_changed: aztec_accelerator::server::VersionsChangedCallback =
                Arc::new(move || {
                    if !dev_mode {
                        return;
                    }
                    match tray::build_tray_menu(&app_handle, dev_mode, &bundled_for_cb, &status) {
                        Ok(new_menu) => {
                            let _ = tray_for_versions.set_menu(Some(new_menu));
                            tracing::info!("Tray menu rebuilt (versions changed)");
                        }
                        Err(e) => {
                            tracing::warn!("Failed to rebuild tray menu: {e}");
                        }
                    }
                });

            // Auth popup callback
            let app_handle_for_auth = app.handle().clone();
            let auth_manager_for_timeout = auth_manager.clone();
            let show_auth_popup: aztec_accelerator::server::ShowAuthPopupCallback =
                Arc::new(move |origin: &str| {
                    windows::show_auth_popup_window(
                        &app_handle_for_auth,
                        origin,
                        &auth_manager_for_timeout,
                    );
                });

            let is_animating_for_status = is_animating.clone();
            let state = AppState {
                on_status: Some(Arc::new(move |text: &str| {
                    tracing::info!(text, "on_status callback fired");
                    if let Err(e) = status_clone.set_text(text) {
                        tracing::error!("set_text failed: {e}");
                    }
                    if let Err(e) = tray_clone.set_tooltip(Some(text)) {
                        tracing::error!("set_tooltip failed: {e}");
                    }
                    let active = text.contains("Proving") || text.contains("Downloading");
                    is_animating_for_status.store(active, Ordering::Relaxed);
                })),
                bundled_version: Some(bundled_version),
                on_versions_changed: Some(on_versions_changed),
                https_port: None,
                config: Some(config_state.clone()),
                auth_manager: Some(auth_manager.clone()),
                show_auth_popup: Some(show_auth_popup),
                prove_semaphore: Some(Arc::new(tokio::sync::Semaphore::new(1))),
            };

            // ── HTTPS startup ──
            let https_port = try_start_https(&state);

            // Manage the shared state for Tauri commands (e.g. enable_safari_support)
            let mut state_with_https = state.clone();
            state_with_https.https_port = https_port;
            app.manage::<SharedAppState>(Arc::new(state_with_https));

            // ── Startup diagnostics ──
            // Update both the status menu item text AND tray tooltip so the
            // message is visible in production builds (where the status item
            // is not in the tray menu but the tooltip is always visible).
            let tray_for_diagnostics = tray.clone();
            if aztec_accelerator::bb::find_bb(None).is_err() {
                tracing::warn!("bb binary not found at startup");
                let _ = status_for_diagnostics.set_text("Warning: bb not found");
                let _ = tray_for_diagnostics.set_tooltip(Some("Warning: bb not found"));
            }

            // ── HTTP server ──
            let mut http_state = state;
            http_state.https_port = https_port;
            let status_for_server = status_for_diagnostics;
            tauri::async_runtime::spawn(async move {
                if let Err(e) = aztec_accelerator::server::start(http_state).await {
                    tracing::error!("Accelerator server error: {e}");
                    let msg = if e.to_string().contains("Address already in use")
                        || e.to_string().contains("address already in use")
                    {
                        "Error: port 59833 in use"
                    } else {
                        "Error: server failed"
                    };
                    let _ = status_for_server.set_text(msg);
                    let _ = tray_for_diagnostics.set_tooltip(Some(msg));
                }
            });

            // ── Background update check ──
            let update_handle = app.handle().clone();
            let update_config = config_state.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                loop {
                    run_update_check(&update_handle, &update_config).await;
                    tokio::time::sleep(Duration::from_secs(12 * 3600)).await;
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Aztec Accelerator")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if should_prevent_exit(code) {
                    api.prevent_exit();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_prevented_for_window_close() {
        // code=None is sent when the last window closes — must be prevented (tray-only app)
        assert!(should_prevent_exit(None));
    }

    #[test]
    fn exit_allowed_for_explicit_quit() {
        // code=Some(0) is sent by app.exit(0) from the Quit menu
        assert!(!should_prevent_exit(Some(0)));
    }

    #[test]
    fn exit_allowed_for_restart() {
        // code=Some(i32::MAX) is sent by app.restart() during auto-update
        assert!(!should_prevent_exit(Some(i32::MAX)));
    }
}
