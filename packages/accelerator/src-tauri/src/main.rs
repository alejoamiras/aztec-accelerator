// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod tray;
mod windows;

use aztec_accelerator::authorization::AuthorizationManager;
use aztec_accelerator::commands::{AuthState, ConfigState, PendingUpdate, SharedAppState};
use aztec_accelerator::server::{AppState, HeadlessState, ServerStatus};
use aztec_accelerator::{certs, commands, config, log_dir, verified_sites};
use parking_lot::RwLock;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
// Only the background update loop uses Duration; that loop is gated off for webdriver builds.
#[cfg(not(feature = "webdriver"))]
use std::time::Duration;
use tauri::menu::MenuItemBuilder;
use tauri::Manager;
// AppHandle is only referenced by the (webdriver-gated) update-check fn.
#[cfg(not(feature = "webdriver"))]
use tauri::AppHandle;
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

/// Try to start the HTTPS server if Safari Support is configured and certs are valid + trusted.
/// Uses a clone of the full `AppState` so the HTTPS server has auth, config, and callbacks.
/// `start_https` flips the shared `https_bound` flag once the listener actually binds, so `/health`
/// advertises `https_port` from the real bind state rather than the config flag.
fn try_start_https(state: &AppState) {
    let cfg = config::load();
    if !cfg.safari_support {
        return;
    }
    if !certs::certs_exist() {
        tracing::warn!("Safari Support enabled but certs missing/invalid — resetting config");
        reset_safari_support(state);
        return;
    }

    // Verify the live CA is still trusted (macOS only).
    if !certs::is_ca_trusted() {
        tracing::warn!("CA not trusted in Keychain — skipping HTTPS");
        return;
    }

    let tls_config = match certs::load_rustls_config() {
        Ok(c) => c,
        Err(e) => {
            // A broken/mismatched cert set (e.g. a crash mid-rotation leaving a new leaf with the old
            // key) must NOT silently wedge HTTPS. Reset Safari Support so the user re-enables and a
            // fresh, matched, trusted set is generated, instead of HTTPS being dead every launch.
            tracing::warn!("Failed to load TLS config ({e}) — resetting Safari Support to recover");
            reset_safari_support(state);
            return;
        }
    };

    aztec_accelerator::server::spawn_https(state.clone(), tls_config);

    // Pre-expiry auto-renewal runs OFF the startup path (a background thread) so the macOS trust
    // prompt can never block/hang launch. The running server keeps its already-loaded config; a
    // rotated set takes effect on the next launch.
    std::thread::spawn(|| {
        if let Err(e) = certs::regenerate_leaf_if_expiring() {
            tracing::warn!("Background leaf renewal: {e}");
        }
    });
}

/// Disable Safari Support in config (certs missing/invalid/untrusted) so the user can re-enable to
/// regenerate a fresh, trusted cert set.
fn reset_safari_support(state: &AppState) {
    if let Some(ref cfg_lock) = state.config {
        let mut cfg = cfg_lock.write();
        cfg.safari_support = false;
        let _ = config::save(&cfg);
    }
}

// ── Auto-update ──────────────────────────────────────────────────────────

/// Whether the background update poller should run.
///
/// A non-production build must never poll the prod updater feed or pop the
/// update-prompt window:
/// - `webdriver` builds are handled at compile time (this fn + the spawn site
///   are `#[cfg(not(feature = "webdriver"))]`), so the poller cannot exist there.
/// - `debug_assertions` (a developer's `cargo tauri dev`, and the `_e2e.yml`
///   `cargo run` desktop app) are disabled by default — opt back in with
///   `AZTEC_ACCEL_FORCE_UPDATE_CHECK=1`.
/// - `AZTEC_ACCEL_NO_UPDATE=1` is a universal kill switch (logged, for audit).
///
/// The shipped release desktop binary (release profile, no `webdriver`, no env
/// overrides) returns `true` — auto-update behavior is unchanged.
#[cfg(not(feature = "webdriver"))]
fn should_poll_for_updates() -> bool {
    if std::env::var("AZTEC_ACCEL_NO_UPDATE").is_ok() {
        tracing::warn!("AZTEC_ACCEL_NO_UPDATE set — background update checks suppressed");
        return false;
    }
    if cfg!(debug_assertions) && std::env::var("AZTEC_ACCEL_FORCE_UPDATE_CHECK").is_err() {
        tracing::info!(
            "Debug build — background update checks disabled (set AZTEC_ACCEL_FORCE_UPDATE_CHECK=1 to enable)"
        );
        return false;
    }
    true
}

/// Background update check wrapper. Calls the shared updater module and
/// shows the prompt window if an update is available and the user hasn't chosen yet.
///
/// Not compiled for `webdriver` builds: the prompt window would steal the
/// active WebDriver browsing context mid-test (see
/// implementations-plan/ci-reliability-2026-05-29/diagnosis.md).
#[cfg(not(feature = "webdriver"))]
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

/// Spawn the HTTP accelerator server, classifying an `AddrInUse` bind failure structurally. A
/// redundant Windows instance (Task Scheduler logon trigger + autostart Run key both fire) bows out
/// with exit(0) when a healthy Aztec already owns :59833; any other failure surfaces in the tray and
/// stays resident. (F-03: extracted verbatim from the `.setup` closure.)
fn spawn_http_server(
    state: aztec_accelerator::server::AppState,
    status: tauri::menu::MenuItem<tauri::Wry>,
    tray: tauri::tray::TrayIcon<tauri::Wry>,
    app_handle: tauri::AppHandle,
) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = aztec_accelerator::server::start(state).await {
            // Classify AddrInUse STRUCTURALLY (by ErrorKind), not by display text — the OS string
            // differs per platform (Windows WSAEADDRINUSE reads "Only one usage of each socket
            // address…"), so a string match would miss it on Windows and skip the whole dual-launch
            // fix on its target platform. bind_with_retry returns the io::Error, boxed by `?`.
            let addr_in_use = e
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::AddrInUse);
            // A redundant instance loses the :59833 bind — the autostart entry AND the crash-recovery
            // launcher can both start us at logon. If a HEALTHY Aztec instance already owns the port,
            // bow out with exit(0) rather than ghosting a tray with no server (exit 0 so the
            // supervisor's restart-on-failure does NOT loop us). A foreign process / no answer is a
            // real error: surface it and stay resident. WINDOWS-ONLY for now (the dual-launch is a new
            // Windows issue); the `&&` short-circuits so /health is only probed on Windows.
            if addr_in_use
                && cfg!(target_os = "windows")
                && aztec_accelerator::server::healthy_aztec_on_port().await
            {
                tracing::warn!(
                    "Another healthy Aztec instance owns :59833 — this instance is redundant; exiting cleanly"
                );
                app_handle.exit(0);
                return;
            }
            tracing::error!("Accelerator server error: {e}");
            let msg = if addr_in_use {
                "Error: port 59833 in use"
            } else {
                "Error: server failed"
            };
            let _ = status.set_text(msg);
            let _ = tray.set_tooltip(Some(msg));
        }
    });
}

/// Spawn the background update poller (5s warm-up, then every 12h). (F-03: extracted from `.setup`.)
#[cfg(not(feature = "webdriver"))]
fn spawn_update_poller(app_handle: AppHandle, config: ConfigState) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            run_update_check(&app_handle, &config).await;
            tokio::time::sleep(Duration::from_secs(12 * 3600)).await;
        }
    });
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

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init());

    #[cfg(feature = "webdriver")]
    {
        builder = builder.plugin(tauri_plugin_webdriver::init());
        tracing::info!("WebDriver plugin registered (port 4445)");
    }

    builder
        .manage(config_state.clone())
        .manage(auth_manager.clone())
        .manage::<commands::VerifiedSitesState>(Arc::new(
            verified_sites::VerifiedSitesRegistry::load(),
        ))
        .manage::<PendingUpdate>(Arc::new(parking_lot::Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::get_autostart_enabled,
            commands::set_autostart,
            commands::set_speed,
            commands::remove_approved_origin,
            commands::get_system_info,
            commands::get_verified_info,
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
                    "quit" => {
                        // The repeating-trigger crash-recovery task relaunches anything not
                        // running, so an intentional quit must delete it first or the app
                        // returns within ~1 min. A crash skips this path → the task survives
                        // → relaunch. Windows-only: mac/linux key on exit code (launchd
                        // SuccessfulExit:false / systemd on-failure), so a clean quit is a
                        // no-op there and the recovery entry must persist across quit.
                        #[cfg(target_os = "windows")]
                        aztec_accelerator::crash_recovery::disable_crash_recovery();
                        app.exit(0);
                    }
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
                Arc::new(move |origin: &str, request_id: &str| {
                    windows::show_auth_popup_window(
                        &app_handle_for_auth,
                        origin,
                        request_id,
                        &auth_manager_for_timeout,
                    );
                });

            let is_animating_for_status = is_animating.clone();
            let on_status = Arc::new(move |status: ServerStatus| {
                let text = status.display_text();
                tracing::info!(text, "on_status callback fired");
                if let Err(e) = status_clone.set_text(text) {
                    tracing::error!("set_text failed: {e}");
                }
                if let Err(e) = tray_clone.set_tooltip(Some(text)) {
                    tracing::error!("set_tooltip failed: {e}");
                }
                is_animating_for_status.store(status.is_busy(), Ordering::Release);
            });
            let core = HeadlessState::headless(
                env!("CARGO_PKG_VERSION"),
                Some(bundled_version),
                Some(config_state.clone()),
                Some(auth_manager.clone()),
            );
            let state = AppState::desktop(core, on_status, on_versions_changed, show_auth_popup);

            // ── HTTPS startup ──
            // One-time migration: delete any legacy on-disk CA private key (older installs) — it was
            // a readable mint-any-cert primitive. SEC-08 fail-closed: if it CANNOT be removed, do NOT
            // bring up Safari HTTPS — a live HTTPS server next to a readable mint-any-cert key + its
            // still-trusted anchor is the exposure we're closing. HTTP is unaffected. Idempotent.
            match certs::migrate_legacy_ca_key() {
                Ok(()) => try_start_https(&state),
                Err(e) => tracing::error!(error = %e,
                    "SECURITY: legacy ca.key could not be removed — Safari HTTPS NOT started (HTTP unaffected)"),
            }

            // Manage the shared state for Tauri commands (e.g. enable_safari_support). It shares the
            // Arc'd https_bound flag with the HTTP server's state, so start_https flipping it after a
            // successful bind is visible to /health (no separate https_port propagation needed). (Q7)
            app.manage::<SharedAppState>(Arc::new(state.clone()));

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

            // ── WebDriver: open Settings window so WebDriver has a browsing context ──
            #[cfg(feature = "webdriver")]
            windows::open_settings_window(app.handle());

            // ── HTTP server ──
            spawn_http_server(
                state,
                status_for_diagnostics,
                tray_for_diagnostics,
                app.handle().clone(),
            );

            // ── Background update check ──
            // Compile-gated off for `webdriver` builds (the prompt window would steal WebDriver's
            // active context mid-test); runtime-gated off for dev/CI via `should_poll_for_updates`.
            #[cfg(not(feature = "webdriver"))]
            if should_poll_for_updates() {
                spawn_update_poller(app.handle().clone(), config_state.clone());
            }

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
