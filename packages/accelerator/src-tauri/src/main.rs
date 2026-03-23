// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use aztec_accelerator::server::{AppState, HTTPS_PORT};
use aztec_accelerator::versions;
use aztec_accelerator::{certs, config, log_dir};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::menu::{
    CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::tray::TrayIconBuilder;
use tauri::AppHandle;
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
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

/// Show a native macOS dialog asking the user to confirm Safari Support setup.
/// Returns true if the user clicks "Continue", false otherwise.
#[cfg(target_os = "macos")]
fn show_safari_support_dialog() -> bool {
    let script = r#"display dialog "Safari blocks requests to localhost unless you install a local certificate.\n\nmacOS will ask for your password once to trust it." with title "Enable Safari Support" buttons {"Cancel", "Continue"} default button "Continue" cancel button "Cancel""#;
    std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Show a native macOS error dialog.
#[cfg(target_os = "macos")]
fn show_error_dialog(message: &str) {
    let script = format!(
        r#"display dialog "{message}" with title "Safari Support Error" buttons {{"OK"}} default button "OK" with icon stop"#,
    );
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output();
}

/// Try to start HTTPS server if Safari Support is configured and certs are valid.
/// Returns the HTTPS port if started, None otherwise.
fn try_start_https() -> Option<u16> {
    let cfg = config::load();
    if !cfg.safari_support {
        return None;
    }
    if !certs::certs_exist() {
        tracing::warn!("Safari Support enabled but certs missing — resetting config");
        let _ = config::save(&config::AcceleratorConfig {
            safari_support: false,
        });
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
            let state_for_https = AppState {
                https_port: Some(HTTPS_PORT),
                ..Default::default()
            };
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

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(move |app| {
            // Hide from Dock — tray-only app
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let bundled_version = env!("AZTEC_BB_VERSION").to_string();

            let status = MenuItemBuilder::with_id("status", "Status: Idle")
                .enabled(false)
                .build(app)?;

            let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
            if autostart_enabled {
                aztec_accelerator::crash_recovery::enable_crash_recovery();
            }
            let autostart = CheckMenuItemBuilder::with_id("toggle_autostart", "Start on Login")
                .checked(autostart_enabled)
                .build(app)?;

            // Safari Support toggle (macOS only)
            #[cfg(target_os = "macos")]
            let safari_support = {
                let cfg = config::load();
                CheckMenuItemBuilder::with_id("toggle_safari", "Safari Support")
                    .checked(cfg.safari_support)
                    .build(app)?
            };

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

                #[cfg(target_os = "macos")]
                let menu = MenuBuilder::new(app)
                    .items(&[
                        &status,
                        &versions_submenu,
                        &show_logs,
                        &autostart,
                        &safari_support,
                        &separator,
                        &version_text,
                        &github,
                        &quit,
                    ])
                    .build()?;

                #[cfg(not(target_os = "macos"))]
                let menu = MenuBuilder::new(app)
                    .items(&[
                        &status,
                        &versions_submenu,
                        &show_logs,
                        &autostart,
                        &separator,
                        &version_text,
                        &github,
                        &quit,
                    ])
                    .build()?;

                menu
            } else {
                let separator = PredefinedMenuItem::separator(app)?;

                #[cfg(target_os = "macos")]
                let menu = MenuBuilder::new(app)
                    .items(&[
                        &autostart,
                        &safari_support,
                        &separator,
                        &version_text,
                        &github,
                        &quit,
                    ])
                    .build()?;

                #[cfg(not(target_os = "macos"))]
                let menu = MenuBuilder::new(app)
                    .items(&[&autostart, &separator, &version_text, &github, &quit])
                    .build()?;

                menu
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
                    "toggle_autostart" => {
                        let manager = app.autolaunch();
                        let currently_enabled = manager.is_enabled().unwrap_or(false);
                        if currently_enabled {
                            let _ = manager.disable();
                            aztec_accelerator::crash_recovery::disable_crash_recovery();
                            let _ = autostart.set_checked(false);
                            tracing::info!("Auto-start on login disabled");
                        } else {
                            let _ = manager.enable();
                            aztec_accelerator::crash_recovery::enable_crash_recovery();
                            let _ = autostart.set_checked(true);
                            tracing::info!("Auto-start on login enabled");
                        }
                    }
                    #[cfg(target_os = "macos")]
                    "toggle_safari" => {
                        let safari_support = safari_support.clone();
                        let currently_enabled = config::load().safari_support;
                        if currently_enabled {
                            // Disable: save config, note restart needed
                            let _ = config::save(&config::AcceleratorConfig {
                                safari_support: false,
                            });
                            let _ = safari_support.set_checked(false);
                            tracing::info!("Safari Support disabled (HTTPS stops on next restart)");
                        } else {
                            // Enable: show dialog, generate certs, install trust, start HTTPS
                            let _ = safari_support.set_checked(false); // Don't check until success
                            std::thread::spawn(move || {
                                if !show_safari_support_dialog() {
                                    tracing::info!("Safari Support setup cancelled by user");
                                    return;
                                }

                                if let Err(e) = certs::generate_and_save() {
                                    tracing::error!("Failed to generate certs: {e}");
                                    show_error_dialog("Failed to generate certificates.");
                                    return;
                                }

                                if let Err(e) = certs::install_ca_trust() {
                                    tracing::error!("Failed to install CA trust: {e}");
                                    show_error_dialog(
                                        "Certificate trust was not granted. Safari Support was not enabled.",
                                    );
                                    return;
                                }

                                // Trust succeeded — save config and start HTTPS
                                let _ = config::save(&config::AcceleratorConfig {
                                    safari_support: true,
                                });
                                let _ = safari_support.set_checked(true);

                                // Start HTTPS server
                                match certs::load_rustls_config() {
                                    Ok(tls_config) => {
                                        let state = AppState {
                                            https_port: Some(HTTPS_PORT),
                                            ..Default::default()
                                        };
                                        tauri::async_runtime::spawn(async move {
                                            if let Err(e) =
                                                aztec_accelerator::server::start_https(
                                                    state, tls_config,
                                                )
                                                .await
                                            {
                                                tracing::error!("HTTPS server error: {e}");
                                            }
                                        });
                                        tracing::info!("Safari Support enabled, HTTPS server started");
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to load TLS config: {e}");
                                    }
                                }
                            });
                        }
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
                            // Rebuild the full menu with the updated versions submenu
                            let status_rebuild = status.clone();
                            let show_logs_rebuild =
                                MenuItemBuilder::with_id("show_logs", "Show Logs")
                                    .build(&app_handle)
                                    .unwrap();
                            let autostart_enabled =
                                app_handle.autolaunch().is_enabled().unwrap_or(false);
                            let autostart_rebuild =
                                CheckMenuItemBuilder::with_id("toggle_autostart", "Start on Login")
                                    .checked(autostart_enabled)
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
                                    &autostart_rebuild,
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

            // Auto-start HTTPS if Safari Support is configured
            let https_port = try_start_https();

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
                https_port,
            };

            // Spawn the HTTP server on the Tokio runtime
            tauri::async_runtime::spawn(async move {
                if let Err(e) = aztec_accelerator::server::start(state).await {
                    tracing::error!("Accelerator server error: {e}");
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Aztec Accelerator");
}
