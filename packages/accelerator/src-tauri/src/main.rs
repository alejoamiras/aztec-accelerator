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

/// q7e3-F-01: the launch-time HTTPS gate as a pure value, lifted out of `try_start_https` so the
/// reset-vs-skip asymmetry (the audit's most-fragile point) is unit-testable with zero mocks.
#[derive(Debug, PartialEq, Eq)]
enum LaunchHttpsGate {
    /// HTTPS is off — do nothing.
    Disabled,
    /// Enabled but certs are missing/invalid — reset `https_enabled` so the user re-enables.
    MissingCertsReset,
    /// Certs present but the CA isn't trusted in the Keychain — skip WITHOUT reset (keep the opt-in).
    UntrustedSkip,
    /// Certs present and trusted — proceed to load TLS + spawn.
    Ready,
}

/// Classify the launch HTTPS gate. `certs_exist` and `ca_trusted` are thunks so the original
/// short-circuit holds exactly: `certs_exist` is not evaluated unless `https_enabled`, and
/// `ca_trusted` not unless `certs_exist` too. Trust is only ever VERIFIED at launch (the thunk wraps
/// `is_ca_trusted`), never installed — launch must never raise the macOS Keychain prompt.
fn classify_launch_https(
    https_enabled: bool,
    certs_exist: impl FnOnce() -> bool,
    ca_trusted: impl FnOnce() -> bool,
) -> LaunchHttpsGate {
    if !https_enabled {
        LaunchHttpsGate::Disabled
    } else if !certs_exist() {
        LaunchHttpsGate::MissingCertsReset
    } else if !ca_trusted() {
        LaunchHttpsGate::UntrustedSkip
    } else {
        LaunchHttpsGate::Ready
    }
}

/// The BLOCKING half of the launch-time HTTPS bring-up: classify the gate, rotate (Linux), load TLS.
///
/// **Must be called while holding the HTTPS lifecycle lock.** Every cert read and write in here —
/// `certs_exist`, `is_ca_trusted`, the rotation's `swap_into`, the TLS load — has to be serialized
/// against the enable and renewal paths, or launch can observe a MIXED new-leaf/old-key set mid-swap
/// and then reset `https_enabled` over an enable that just succeeded (post-impl codex Medium).
/// `None` ⇒ don't start a listener.
fn prepare_launch_https(
    state: &AppState,
) -> Option<std::sync::Arc<tokio_rustls::rustls::ServerConfig>> {
    let cfg = config::load();
    // q7e3-F-01: the pre-load gate is a tested pure classifier; the load-failure reset stays below
    // (it depends on load_rustls_config's Result, not on these three booleans).
    //
    // Linux (plan R3): trust is inherently per-browser/partial, and a bound-but-untrusted loopback
    // listener is harmless — browsers fast-fail the HTTPS probe and the SDK falls back to HTTP. So
    // serve whenever certs are valid, decoupled from trust (which the *wizard* still checks). macOS
    // and Windows keep the verify-trust gate (they'd otherwise present an untrusted cert with a real
    // dialog behind it).
    #[cfg(target_os = "linux")]
    let ca_trusted = || true;
    #[cfg(not(target_os = "linux"))]
    let ca_trusted = certs::is_ca_trusted;
    match classify_launch_https(cfg.https_enabled, certs::certs_exist, ca_trusted) {
        LaunchHttpsGate::Disabled => return None,
        LaunchHttpsGate::MissingCertsReset => {
            tracing::warn!("HTTPS enabled but certs missing/invalid — resetting config");
            reset_https_enabled(state);
            return None;
        }
        LaunchHttpsGate::UntrustedSkip => {
            tracing::warn!("CA not trusted in Keychain — skipping HTTPS");
            return None;
        }
        LaunchHttpsGate::Ready => {}
    }

    // Pre-expiry renewal (§7). Linux: SILENT rotation (user NSS needs no prompt) done BEFORE loading +
    // binding the TLS config, so the FRESH leaf is what we serve. Doing it after the bind left the
    // acceptor holding the OLD leaf for the whole session — a long-running tray app would eventually
    // serve an EXPIRED cert (post-impl codex Medium). macOS/Windows do NOT rotate here — the setup
    // closure surfaces a renewal *consent window* instead of a surprise background OS trust prompt.
    #[cfg(target_os = "linux")]
    if let Err(e) = certs::regenerate_leaf_if_expiring() {
        tracing::warn!("Background leaf renewal: {e}");
    }

    match certs::load_rustls_config() {
        Ok(c) => Some(c),
        Err(e) => {
            // A broken/mismatched cert set (e.g. a crash mid-rotation leaving a new leaf with the old
            // key) must NOT silently wedge HTTPS. Reset https_enabled so the user re-enables and a
            // fresh, matched, trusted set is generated, instead of HTTPS being dead every launch.
            tracing::warn!("Failed to load TLS config ({e}) — resetting https_enabled to recover");
            reset_https_enabled(state);
            None
        }
    }
}

/// Launch-time HTTPS bring-up, fully serialized against the enable and renewal paths.
///
/// Runs entirely off the setup thread. It **waits** for the HTTPS lifecycle lock rather than standing
/// down on a failed try-lock: renewal can own that lock without ever binding a listener, so standing
/// down would leave HTTPS unstarted for the whole session (post-impl codex Medium). After acquiring
/// it, re-check `https_bound` — an enable path may have completed the entire bring-up while we waited.
/// The blocking cert work then runs on a blocking thread so it never occupies an async worker.
async fn launch_https(state: AppState) {
    // Cheap config read — no lock needed to learn HTTPS is simply off.
    if !config::load().https_enabled {
        return;
    }

    let guard = aztec_accelerator::server::claim_https_lifecycle(&state).await;

    if state.https_bound.load(Ordering::Relaxed) {
        tracing::debug!("HTTPS already bound by another path — launch gate has nothing to do");
        return; // `guard` drops here
    }

    let prep_state = state.clone();
    let tls_config =
        match tauri::async_runtime::spawn_blocking(move || prepare_launch_https(&prep_state)).await
        {
            Ok(Some(tls)) => tls,
            Ok(None) => return, // gate said no (or reset) — `guard` drops
            Err(e) => {
                tracing::error!("HTTPS prepare task failed: {e}");
                return;
            }
        };

    // Await the bind while still HOLDING the guard, so no other path can start a competing bring-up
    // or mutate the cert set while our listener is coming up. `guard` drops at end of scope,
    // releasing the lock for enable/renewal/removal; the serving loop itself runs unlocked.
    let ready = aztec_accelerator::server::spawn_https(state, tls_config);
    match ready.await {
        Ok(true) => tracing::info!("HTTPS listener started at launch"),
        _ => tracing::warn!("HTTPS listener did not bind at launch — continuing HTTP-only"),
    }
    drop(guard);
}

/// Disable HTTPS in config (certs missing/invalid/untrusted) so the user can re-enable to
/// regenerate a fresh, trusted cert set.
fn reset_https_enabled(state: &AppState) {
    if let Some(ref cfg_lock) = state.config {
        // q7e3-F-13: shared core helper; swallow the save error (best-effort reset, unchanged policy).
        let _ = config::lock_mutate_save(cfg_lock, |cfg| {
            cfg.https_enabled = false;
            true
        });
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
        let new_version = update.version().to_string();

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
            // F-03 sink A: `/health` answering "ok" proves only that SOMETHING is listening — both
            // fields are public contract, so any local process can say it and evict the real app,
            // taking its HTTPS listener down with it (which is what opens F-01 on Windows). Ask the
            // OS who owns the socket as well.
            //
            // D-ITEM7 polarity: `may_bow_out` exits unless the owner is POSITIVELY foreign, so
            // `Ours` and `Unknown` both behave exactly as before. This path runs every minute on
            // Windows (the crash-recovery task's PT1M trigger), so treating a transient lookup
            // failure as "stay resident" would strand duplicate tray processes.
            if addr_in_use && cfg!(target_os = "windows") {
                // ONE connection answers both questions. Probing health and identifying the owner
                // separately is a deterministic bypass, not a race: a one-shot listener can accept
                // the health request, close its listening socket, answer healthy over the accepted
                // socket, and leave the second lookup with nothing to find — which is `Unknown`, and
                // `Unknown` exits (post-impl codex round 7). Blocking, so it runs off the async
                // runtime's worker.
                let (healthy, owner) = tokio::task::spawn_blocking(|| {
                    aztec_accelerator::server::probe_and_identify(59833)
                })
                .await
                .unwrap_or((false, aztec_accelerator::server::PortOwner::Unknown));
                if aztec_accelerator::server::may_bow_out(healthy, owner) {
                    tracing::warn!(
                        ?owner,
                        "Another healthy Aztec instance owns :59833 — this instance is redundant; exiting cleanly"
                    );
                    app_handle.exit(0);
                    return;
                }
                if healthy {
                    tracing::error!(
                        ?owner,
                        "A process answering /health owns :59833 but is NOT our image — staying resident rather than letting it evict us"
                    );
                }
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

/// Spawn the F-004 Layer B post-launch floor tracker. Once THIS build's OWN accelerator server has
/// answered `/health` — as a healthy Aztec reporting OUR exact version — 3 consecutive times, advance
/// the monotonic version floor to the running version. Two guards matter (audit H3):
///   - 3 consecutive HEALTHY probes (not merely "process started") means a build that boots but
///     immediately wedges its server never ratchets the floor;
///   - the reported `/health.version` must equal `CARGO_PKG_VERSION`, AND this process must actually
///     own the `:59833` bind. The version match alone does NOT prove we are observing our own server
///     — the comment here used to claim it did, which is what made F-03 sink B look safe. On
///     macOS/Linux a broken new build that LOST the bind still sees the healthy INCUMBENT's
///     `/health`, and any local process can serve those fields. Ownership of the bind is the
///     in-process fact that cannot be forged.
///
/// Runs once; gated off for webdriver builds.
#[cfg(not(feature = "webdriver"))]
fn spawn_floor_tracker() {
    tauri::async_runtime::spawn(async move {
        let want = env!("CARGO_PKG_VERSION");
        let mut consecutive = 0u32;
        // Bounded (~2 min) so a genuinely unhealthy build never commits the floor.
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_secs(3)).await;
            // F-03 sink B: the probe proves the server SERVES (a bound-but-wedged build must never
            // ratchet the floor), and `we_own_the_bind()` proves the server is OURS. Neither alone
            // is sufficient, and the probe alone was forgeable by any local process.
            let probed = aztec_accelerator::server::healthy_aztec_version_on_port().await;
            if aztec_accelerator::server::should_commit_floor(
                aztec_accelerator::server::we_own_the_bind(),
                probed.as_deref(),
                want,
            ) {
                consecutive += 1;
                if consecutive >= 3 {
                    aztec_accelerator::updater::commit_launch_floor();
                    return;
                }
            } else {
                consecutive = 0;
            }
        }
        tracing::warn!(
            version = want,
            "Launch never reached 3 consecutive healthy version-matched probes; version floor not advanced this run"
        );
    });
}

// ── Desktop bootstrap (q7e3-F-04) ────────────────────────────────────────
// `.setup()` was a ~150-line Long Method closure. These phase helpers carry the capture-heavy
// construction; the closure stays a thin, visibly-ordered sequencer because two orderings there are
// load-bearing: SEC-08 migrate-first before HTTPS, and `manage::<SharedAppState>` before the
// webdriver settings-window + HTTP spawn (commands/webdriver break at RUNTIME if reordered).

/// Build the tray menu + icon with the static menu-event handler.
fn build_tray(
    app: &tauri::App,
    dev_mode: bool,
    bundled_version: &str,
    status: &tauri::menu::MenuItem<tauri::Wry>,
) -> Result<tauri::tray::TrayIcon, Box<dyn std::error::Error>> {
    let menu = tray::build_tray_menu(&app.handle().clone(), dev_mode, bundled_version, status)?;
    tray::build_tray_icon(app, &menu, move |app, event| match event.id().as_ref() {
        "quit" => {
            // The repeating-trigger crash-recovery task relaunches anything not
            // running, so an intentional quit must delete it first or the app
            // returns within ~1 min. A crash skips this path → the task survives
            // → relaunch. Windows-only: mac/linux key on exit code (launchd
            // SuccessfulExit:false / systemd on-failure), so a clean quit is a
            // no-op there and the recovery entry must persist across quit.
            // codex #7: surface (log) a non-confirmed disarm — an unconfirmed disable here means the
            // Task Scheduler recovery entry may survive and relaunch the app within ~1 min of this quit.
            // Piece 2 (A5): the disarm is serialized behind autostart.lock inside the seam —
            // it cannot interleave with the marker reconcile's arm→remove span.
            #[cfg(target_os = "windows")]
            aztec_accelerator::autostart::quit_disarm();
            app.exit(0);
        }
        "show_logs" => open_in_browser(&log_dir()),
        "open_github" => {
            open_in_browser(&"https://github.com/alejoamiras/aztec-accelerator");
        }
        "settings" => windows::open_settings_window(app),
        _ => {}
    })
}

/// Wire the desktop `AppState`: the versions-changed tray rebuild, the auth popup, and the
/// status-text/tooltip/animation callback. **Consumes `status`** (it moves into the versions-changed
/// callback) — anything the caller needs afterwards must be cloned BEFORE this call, which turns the
/// old "clone before the move" comment into a compiler-enforced property.
#[allow(clippy::too_many_arguments)]
fn build_desktop_state(
    app: &tauri::App,
    dev_mode: bool,
    bundled_version: String,
    status: tauri::menu::MenuItem<tauri::Wry>,
    tray: &tauri::tray::TrayIcon,
    is_animating: &Arc<AtomicBool>,
    config_state: &ConfigState,
    auth_manager: &AuthState,
) -> AppState {
    let status_clone = status.clone();
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
    let show_auth_popup: aztec_accelerator::server::ShowAuthPopupCallback = Arc::new(
        move |origin: &aztec_accelerator::authorization::CanonicalOrigin, request_id: &str| {
            // Deref-coerces to &str at the window boundary; the origin is canonical by type.
            windows::show_auth_popup_window(
                &app_handle_for_auth,
                origin,
                request_id,
                &auth_manager_for_timeout,
            );
        },
    );

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
    AppState::desktop(core, on_status, on_versions_changed, show_auth_popup)
}

fn main() {
    // `--remove-ca-trust`: remove the local CA from every browser trust store, then exit WITHOUT
    // starting the GUI. Used by scripted cleanup and the Windows NSIS uninstaller (Phase 6). Runs
    // before anything else so it never spins up a tray/server.
    if std::env::args().any(|a| a == "--remove-ca-trust") {
        let report = aztec_accelerator::trust::remove_ca_trust(&certs::live_ca_cert_path());
        for s in &report.stores {
            println!(
                "{}: {}",
                s.store,
                if s.installed {
                    "still trusted"
                } else {
                    "removed / absent"
                }
            );
        }
        // Exit NON-ZERO if any store still trusts the anchor (or removal couldn't be confirmed) so a
        // scripted uninstall / NSIS hook can detect the failure instead of always seeing success
        // (post-impl codex High).
        if report.removal_incomplete() {
            eprintln!("error: CA trust removal was incomplete — see the per-store lines above");
            std::process::exit(1);
        }
        return;
    }

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
            commands::repair_autostart,
            commands::set_speed,
            commands::remove_approved_origin,
            commands::get_system_info,
            commands::get_verified_info,
            commands::get_pending_auth,
            commands::respond_auth,
            commands::enable_https,
            commands::disable_https,
            commands::remove_https_trust,
            commands::get_onboarding_state,
            commands::complete_onboarding,
            commands::renew_cert,
            commands::record_renewal_prompt,
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

            // Startup autostart reconciliation (piece-2 plan §4): marker reconcile FIRST (the
            // removal transaction — the only rearm path allowed while a window exists), then the
            // heal, then the intent-keyed rearm. Suppressed ⇒ BOTH heal and rearm are skipped
            // this launch: "no process heals, no process rearms while a marker is live".
            {
                if aztec_accelerator::autostart::startup_reconcile() {
                    // The heal now runs on Windows too — the marker above is what piece 1 gated
                    // it on. Webdriver builds still skip the unattended heal (S1: E2E must not
                    // mutate a runner's real login items; the command path stays live for L7).
                    #[cfg(not(feature = "webdriver"))]
                    match aztec_accelerator::autostart::heal_if_broken(app.handle()) {
                        aztec_accelerator::autostart::HealOutcome::Healed { from, to } => {
                            tracing::info!(%from, %to, "startup autostart heal applied");
                        }
                        aztec_accelerator::autostart::HealOutcome::Failed(e) => {
                            tracing::warn!("startup autostart heal failed: {e}");
                        }
                        aztec_accelerator::autostart::HealOutcome::Skipped(reason) => {
                            tracing::debug!("startup autostart heal skipped: {reason}");
                        }
                        aztec_accelerator::autostart::HealOutcome::NotNeeded => {}
                    }
                    aztec_accelerator::autostart::startup_rearm(app.handle());
                }
            }

            // ── Build tray ──
            let tray = build_tray(app, dev_mode, &bundled_version, &status)?;

            // ── Animation ──
            let is_animating = Arc::new(AtomicBool::new(false));
            tray::start_animation_loop(tray.clone(), app.handle().clone(), is_animating.clone());

            // ── Callbacks and AppState wiring ──
            // q7e3-F-04: build_desktop_state CONSUMES `status`, so anything needed below must be
            // cloned first — the old "clone before the move" comment, now compiler-enforced.
            let status_for_diagnostics = status.clone();
            let state = build_desktop_state(
                app,
                dev_mode,
                bundled_version,
                status,
                &tray,
                &is_animating,
                &config_state,
                &auth_manager,
            );

            // ── HTTPS startup ──
            // One-time migration: delete any legacy on-disk CA private key (older installs) — it was
            // a readable mint-any-cert primitive. SEC-08 fail-closed: if it CANNOT be removed, do NOT
            // bring up HTTPS — a live HTTPS server next to a readable mint-any-cert key + its
            // still-trusted anchor is the exposure we're closing. HTTP is unaffected. Idempotent.
            match certs::migrate_legacy_ca_key() {
                Ok(()) => {
                    // Run the HTTPS bring-up OFF the setup thread. It waits on the lifecycle lock and
                    // then does SYNCHRONOUS trust-store queries (`is_ca_trusted` shells out to
                    // `security`/`certutil`) with no timeout — on its own blocking thread, so the setup
                    // thread still reaches the HTTP-server spawn below regardless. HTTP is the critical
                    // path and must never be blocked by a slow/hung HTTPS trust query (post-impl codex
                    // Medium). HTTPS binds a different port, so there's no race with the HTTP listener.
                    let https_state = state.clone();
                    tauri::async_runtime::spawn(launch_https(https_state));
                }
                Err(e) => tracing::error!(error = %e,
                    "SECURITY: legacy ca.key could not be removed — HTTPS NOT started (HTTP unaffected)"),
            }

            // Manage the shared state for Tauri commands (e.g. enable_https). It shares the
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

            // ── First-run onboarding wizard ──
            // Shown once when the config's onboarding_version is behind — new installs AND existing
            // upgraders (whose config lacks the marker → 0). Gated off for webdriver builds, which
            // bootstrap the Settings window as their browsing context (the wizard E2E drives it
            // explicitly instead of relying on auto-show, so the existing specs stay unaffected).
            #[cfg(not(feature = "webdriver"))]
            if config_state.read().onboarding_version < config::ONBOARDING_VERSION {
                windows::show_onboarding_window(app.handle());
            }

            // ── Certificate renewal consent (macOS/Windows, §7) ──
            // When the leaf is within the pre-expiry window, offer renewal via a consent window rather
            // than a silent background OS trust prompt (Linux rotates silently in try_start_https).
            // Throttled by `last_rotation_prompt_at` so clicking "Later" suppresses the re-prompt for a
            // day even across quick restarts (it still reappears on later launches until expiry).
            #[cfg(all(any(target_os = "macos", target_os = "windows"), not(feature = "webdriver")))]
            {
                const RENEWAL_THROTTLE_SECS: i64 = 20 * 3600;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let cfg = config::load();
                let recently_prompted = cfg
                    .last_rotation_prompt_at
                    .is_some_and(|t| now.saturating_sub(t) < RENEWAL_THROTTLE_SECS);
                if cfg.https_enabled
                    && !recently_prompted
                    && certs::certs_exist()
                    && certs::leaf_is_expiring()
                {
                    windows::show_renewal_window(app.handle());
                }
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
                // F-004 Layer B: advance the monotonic version floor once this build proves it runs.
                spawn_floor_tracker();
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

    // q7e3-F-01 characterization (test-FIRST): the launch HTTPS gate's four outcomes + the
    // reset-vs-skip asymmetry + both short-circuits (panicking thunks prove the unevaluated checks).
    #[test]
    fn launch_gate_disabled_short_circuits_everything() {
        assert_eq!(
            classify_launch_https(
                false,
                || panic!("certs_exist must not be checked when https is off"),
                || panic!("trust must not be checked when https is off"),
            ),
            LaunchHttpsGate::Disabled
        );
    }

    #[test]
    fn launch_gate_missing_certs_resets_and_short_circuits_trust() {
        // certs missing → reset; is_ca_trusted MUST NOT be called (preserves the original short-circuit).
        assert_eq!(
            classify_launch_https(
                true,
                || false,
                || panic!("trust must not be checked when certs are missing")
            ),
            LaunchHttpsGate::MissingCertsReset
        );
    }

    #[test]
    fn launch_gate_untrusted_skips_without_reset() {
        // certs present but untrusted → SKIP, NOT reset (the asymmetry the audit flagged as fragile).
        assert_eq!(
            classify_launch_https(true, || true, || false),
            LaunchHttpsGate::UntrustedSkip
        );
    }

    #[test]
    fn launch_gate_ready_when_present_and_trusted() {
        assert_eq!(
            classify_launch_https(true, || true, || true),
            LaunchHttpsGate::Ready
        );
    }
}
