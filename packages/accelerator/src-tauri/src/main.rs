// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use aztec_accelerator::authorization::{AuthDecision, AuthorizationManager};
use aztec_accelerator::commands::{AuthState, ConfigState, SharedAppState};
use aztec_accelerator::server::{AppState, HTTPS_PORT};
use aztec_accelerator::versions;
use aztec_accelerator::{certs, commands, config, log_dir};
use parking_lot::RwLock;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::MacosLauncher;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

// Tray icon variants (44x44 RGBA PNGs, macOS template mode)
static ICON_IDLE: &[u8] = include_bytes!("../icons/tray-idle.png");
static ICON_PROVING: [&[u8]; 24] = [
    include_bytes!("../icons/tray-proving-1.png"),
    include_bytes!("../icons/tray-proving-2.png"),
    include_bytes!("../icons/tray-proving-3.png"),
    include_bytes!("../icons/tray-proving-4.png"),
    include_bytes!("../icons/tray-proving-5.png"),
    include_bytes!("../icons/tray-proving-6.png"),
    include_bytes!("../icons/tray-proving-7.png"),
    include_bytes!("../icons/tray-proving-8.png"),
    include_bytes!("../icons/tray-proving-9.png"),
    include_bytes!("../icons/tray-proving-10.png"),
    include_bytes!("../icons/tray-proving-11.png"),
    include_bytes!("../icons/tray-proving-12.png"),
    include_bytes!("../icons/tray-proving-13.png"),
    include_bytes!("../icons/tray-proving-14.png"),
    include_bytes!("../icons/tray-proving-15.png"),
    include_bytes!("../icons/tray-proving-16.png"),
    include_bytes!("../icons/tray-proving-17.png"),
    include_bytes!("../icons/tray-proving-18.png"),
    include_bytes!("../icons/tray-proving-19.png"),
    include_bytes!("../icons/tray-proving-20.png"),
    include_bytes!("../icons/tray-proving-21.png"),
    include_bytes!("../icons/tray-proving-22.png"),
    include_bytes!("../icons/tray-proving-23.png"),
    include_bytes!("../icons/tray-proving-24.png"),
];

/// Returns true in debug builds (`cargo tauri dev`), false in release.
fn is_dev_mode() -> bool {
    cfg!(debug_assertions)
}

/// Open a path or URL in the platform's default handler.
fn open_in_browser(target: &impl AsRef<Path>) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg(target.as_ref())
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(target.as_ref())
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(target.as_ref())
            .spawn();
    }
}

/// Build a "Versions" submenu listing the bundled + cached bb versions.
fn build_versions_submenu(
    app: &AppHandle,
    bundled_version: &str,
) -> Result<tauri::menu::Submenu<tauri::Wry>, Box<dyn std::error::Error>> {
    let mut builder = SubmenuBuilder::with_id(app, "versions", "Versions");

    // Bundled version always first
    let bundled_item = MenuItemBuilder::with_id(
        format!("version_{bundled_version}"),
        format!("{bundled_version} (bundled)"),
    )
    .enabled(false)
    .build(app)?;
    builder = builder.item(&bundled_item);

    // Cached versions (exclude bundled to avoid duplicate)
    let cached = versions::list_cached_versions();
    for v in &cached {
        if v != bundled_version {
            let item = MenuItemBuilder::with_id(format!("version_{v}"), v.as_str())
                .enabled(false)
                .build(app)?;
            builder = builder.item(&item);
        }
    }

    Ok(builder.build()?)
}

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

            let settings = MenuItemBuilder::with_id("settings", "Settings").build(app)?;

            // About section: version info + GitHub link (always shown)
            let app_version = env!("CARGO_PKG_VERSION");
            let aztec_bb_version = env!("AZTEC_BB_VERSION");
            let version_text = MenuItemBuilder::with_id(
                "version_info",
                format!("v{app_version} · Aztec {aztec_bb_version}"),
            )
            .enabled(false)
            .build(app)?;

            let github = MenuItemBuilder::with_id("open_github", "GitHub").build(app)?;

            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

            let menu = if dev_mode {
                let versions_submenu =
                    build_versions_submenu(&app.handle().clone(), &bundled_version)?;
                let show_logs = MenuItemBuilder::with_id("show_logs", "Show Logs").build(app)?;
                let separator = PredefinedMenuItem::separator(app)?;

                MenuBuilder::new(app)
                    .items(&[
                        &status,
                        &versions_submenu,
                        &show_logs,
                        &settings,
                        &separator,
                        &version_text,
                        &github,
                        &quit,
                    ])
                    .build()?
            } else {
                let separator = PredefinedMenuItem::separator(app)?;

                MenuBuilder::new(app)
                    .items(&[&settings, &separator, &version_text, &github, &quit])
                    .build()?
            };

            let tray_icon =
                tauri::image::Image::from_bytes(ICON_IDLE).expect("failed to load tray icon");

            let tray = TrayIconBuilder::new()
                .icon(tray_icon)
                .icon_as_template(true)
                .tooltip("Aztec Accelerator")
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "quit" => app.exit(0),
                    "show_logs" => {
                        open_in_browser(&log_dir());
                    }
                    "open_github" => {
                        open_in_browser(&"https://github.com/alejoamiras/aztec-accelerator");
                    }
                    "settings" => {
                        open_settings_window(app);
                    }
                    _ => {}
                })
                .build(app)?;

            // Tray icon animation loop — pulses outward during proving.
            // Both set_icon + set_icon_as_template must run in a single main-thread
            // turn to avoid a black flash between the two calls.
            let is_animating = Arc::new(AtomicBool::new(false));
            {
                let is_animating = is_animating.clone();
                let tray = tray.clone();
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let mut interval = tokio::time::interval(Duration::from_millis(50));
                    let mut frame_idx: usize = 0;
                    let mut was_animating = false;
                    loop {
                        interval.tick().await;
                        let animating = is_animating.load(Ordering::Relaxed);
                        if animating {
                            let tray = tray.clone();
                            let frame = frame_idx;
                            let _ = handle.run_on_main_thread(move || {
                                if let Ok(icon) =
                                    tauri::image::Image::from_bytes(ICON_PROVING[frame])
                                {
                                    let _ = tray.set_icon(Some(icon));
                                    let _ = tray.set_icon_as_template(true);
                                }
                            });
                            frame_idx = (frame_idx + 1) % ICON_PROVING.len();
                            was_animating = true;
                        } else if was_animating {
                            let tray = tray.clone();
                            let _ = handle.run_on_main_thread(move || {
                                if let Ok(icon) = tauri::image::Image::from_bytes(ICON_IDLE) {
                                    let _ = tray.set_icon(Some(icon));
                                    let _ = tray.set_icon_as_template(true);
                                }
                            });
                            frame_idx = 0;
                            was_animating = false;
                        }
                    }
                });
            }

            let status_clone = status.clone();
            let tray_clone = tray.clone();

            // Versions changed callback: rebuild the Versions submenu when versions change.
            // Only needed in dev mode (production menu has no Versions submenu).
            let app_handle = app.handle().clone();
            let bundled_for_cb = bundled_version.clone();
            let tray_for_versions = tray.clone();
            let on_versions_changed: aztec_accelerator::server::VersionsChangedCallback =
                Arc::new(move || {
                    if !dev_mode {
                        return;
                    }
                    match build_versions_submenu(&app_handle, &bundled_for_cb) {
                        Ok(new_submenu) => {
                            let status_rebuild = status.clone();
                            let show_logs_rebuild =
                                MenuItemBuilder::with_id("show_logs", "Show Logs")
                                    .build(&app_handle)
                                    .unwrap();
                            let settings_rebuild = MenuItemBuilder::with_id("settings", "Settings")
                                .build(&app_handle)
                                .unwrap();
                            let quit_rebuild = MenuItemBuilder::with_id("quit", "Quit")
                                .build(&app_handle)
                                .unwrap();

                            let app_version = env!("CARGO_PKG_VERSION");
                            let aztec_bb_version = env!("AZTEC_BB_VERSION");
                            let version_text_rebuild = MenuItemBuilder::with_id(
                                "version_info",
                                format!("v{app_version} · Aztec {aztec_bb_version}"),
                            )
                            .enabled(false)
                            .build(&app_handle)
                            .unwrap();
                            let github_rebuild = MenuItemBuilder::with_id("open_github", "GitHub")
                                .build(&app_handle)
                                .unwrap();
                            let separator_rebuild =
                                PredefinedMenuItem::separator(&app_handle).unwrap();

                            let new_menu = MenuBuilder::new(&app_handle)
                                .items(&[
                                    &status_rebuild,
                                    &new_submenu,
                                    &show_logs_rebuild,
                                    &settings_rebuild,
                                    &separator_rebuild,
                                    &version_text_rebuild,
                                    &github_rebuild,
                                    &quit_rebuild,
                                ])
                                .build()
                                .unwrap();
                            let _ = tray_for_versions.set_menu(Some(new_menu));
                            tracing::info!("Versions submenu updated");
                        }
                        Err(e) => {
                            tracing::warn!("Failed to rebuild versions submenu: {e}");
                        }
                    }
                });

            // Build the show_auth_popup callback that opens the authorization window
            let app_handle_for_auth = app.handle().clone();
            let auth_manager_for_timeout = auth_manager.clone();
            let show_auth_popup: aztec_accelerator::server::ShowAuthPopupCallback =
                Arc::new(move |origin: &str| {
                    show_auth_popup_window(&app_handle_for_auth, origin, &auth_manager_for_timeout);
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

            // Auto-start HTTPS if Safari Support is configured.
            // Uses a clone of the full AppState so the HTTPS server has auth + config.
            let https_port = try_start_https(&state);

            // Manage the shared state for Tauri commands (e.g. enable_safari_support)
            let mut state_with_https = state.clone();
            state_with_https.https_port = https_port;
            app.manage::<SharedAppState>(Arc::new(state_with_https));

            // Spawn the HTTP server on the Tokio runtime
            let mut http_state = state;
            http_state.https_port = https_port;
            tauri::async_runtime::spawn(async move {
                if let Err(e) = aztec_accelerator::server::start(http_state).await {
                    tracing::error!("Accelerator server error: {e}");
                }
            });

            // Background update check: 5s after launch, then every 12 hours
            let update_handle = app.handle().clone();
            let update_config = config_state.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                loop {
                    check_for_update(&update_handle, &update_config).await;
                    tokio::time::sleep(Duration::from_secs(12 * 3600)).await;
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Aztec Accelerator")
        .run(|_app, event| {
            // Tray-only app — keep running when Settings or auth popup windows are closed.
            // Only prevent automatic exit (code=None, triggered by last window closing).
            // Explicit app.exit(0) from the "Quit" menu sets code=Some(0) and must go through.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if should_prevent_exit(code) {
                    api.prevent_exit();
                }
            }
        });
}

/// On macOS, switch to Regular activation policy so the window appears in Dock/Cmd+Tab.
/// Registers a destroy listener on the window to switch back to Accessory when all windows close.
#[cfg(target_os = "macos")]
fn activate_for_window(app: &AppHandle, window: &tauri::WebviewWindow) {
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            if handle.webview_windows().is_empty() {
                let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn activate_for_window(_app: &AppHandle, _window: &tauri::WebviewWindow) {}

/// Open or focus the Settings window.
fn open_settings_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.set_focus();
        return;
    }
    if let Ok(window) =
        WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
            .title("Aztec Accelerator Settings")
            .inner_size(500.0, 520.0)
            .resizable(false)
            .center()
            .build()
    {
        activate_for_window(app, &window);
    }
}

/// Show the authorization popup for an unknown origin.
/// Spawns a 60s timeout that auto-denies if the user doesn't respond.
/// If the user closes the window without responding, the timeout will still
/// fire and resolve all pending requests for this origin with Deny.
fn show_auth_popup_window(app: &AppHandle, origin: &str, auth_manager: &Arc<AuthorizationManager>) {
    let label = format!("auth-{}", commands::sanitize_window_label(origin));
    if app.get_webview_window(&label).is_some() {
        return; // popup already open for this origin
    }

    let url = format!("authorize.html?origin={}", urlencoding::encode(origin));
    if let Ok(window) = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title("Authorize Site")
        .inner_size(400.0, 300.0)
        .resizable(false)
        .center()
        .always_on_top(true)
        .build()
    {
        activate_for_window(app, &window);
    }

    // Spawn 60s timeout — always resolve with Deny if still pending.
    // This handles both: (a) user ignoring the popup, and (b) user closing the
    // window without clicking Allow/Deny. In case (b), the window is gone but
    // the pending senders are still in the HashMap. resolve() is a no-op if the
    // origin was already resolved by respond_auth (senders already consumed).
    let app_handle = app.clone();
    let origin_owned = origin.to_string();
    let auth_manager = auth_manager.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        // Close window if still open
        if let Some(window) = app_handle.get_webview_window(&label) {
            let _ = window.close();
        }
        // Always try to resolve — no-op if already resolved by user click
        auth_manager.resolve(&origin_owned, AuthDecision::Deny);
        tracing::debug!(origin = %origin_owned, "Authorization timeout cleanup");
    });
}

/// Returns true if the exit should be prevented.
/// Window-close events have code=None and should be prevented (tray-only app).
/// Explicit exits (Quit menu, restart) have code=Some(_) and must go through.
fn should_prevent_exit(code: Option<i32>) -> bool {
    code.is_none()
}

// ── Auto-update ──────────────────────────────────────────────────────────

use tauri_plugin_updater::UpdaterExt;

/// Background update check. Runs 5s after launch, then every 12 hours.
/// Also called by `respond_update_prompt` when the user clicks "Update Now".
pub async fn check_for_update(app: &AppHandle, config_state: &ConfigState) {
    tracing::info!("Checking for updates...");
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!("Failed to build updater: {e}");
            return;
        }
    };

    let update = match updater.check().await {
        Ok(Some(update)) => update,
        Ok(None) => {
            tracing::info!("No update available");
            return;
        }
        Err(e) => {
            tracing::warn!("Update check failed: {e}");
            return;
        }
    };

    let new_version = update.version.clone();
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    tracing::info!(current = %current_version, new = %new_version, "Update available");

    let auto_update_pref = { config_state.read().auto_update };
    tracing::info!(?auto_update_pref, "Auto-update preference");

    match auto_update_pref {
        None => {
            tracing::info!("Showing update prompt (first time)");
            show_update_prompt_window(app, &current_version, &new_version);
        }
        Some(true) => {
            tracing::info!("Auto-update enabled, performing update");
            perform_update(app, update).await;
        }
        Some(false) => {
            tracing::info!(version = %new_version, "Update available (manual mode)");
        }
    }
}

/// Download, verify signature, install, and restart.
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
            },
        )
        .await
    {
        Ok(()) => {
            tracing::info!("Update installed, restarting");
            app.restart();
        }
        Err(e) => {
            tracing::error!("Update failed: {e}");
        }
    }
}

/// Show the one-time update prompt window.
fn show_update_prompt_window(app: &AppHandle, current_version: &str, new_version: &str) {
    if app.get_webview_window("update-prompt").is_some() {
        return;
    }

    let url = format!(
        "update-prompt.html?current={}&version={}",
        urlencoding::encode(current_version),
        urlencoding::encode(new_version)
    );
    if let Ok(window) = WebviewWindowBuilder::new(app, "update-prompt", WebviewUrl::App(url.into()))
        .title("Aztec Accelerator Update")
        .inner_size(420.0, 280.0)
        .resizable(false)
        .center()
        .build()
    {
        activate_for_window(app, &window);
    }
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
