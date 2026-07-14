use crate::authorization::{AuthDecision, AuthorizationManager};
use crate::config::{self, AcceleratorConfig};
use crate::verified_sites::VerifiedSitesRegistry;
use parking_lot::RwLock;
use std::sync::Arc;
use tauri::Manager;

pub type ConfigState = Arc<RwLock<AcceleratorConfig>>;

/// Lock the config, apply `f`, then persist — the single source of truth for the lock-mutate-save
/// pattern (replaces copy-pasted `config.write()` + `config::save` blocks). Propagates the save error.
fn mutate_config(
    config: &ConfigState,
    f: impl FnOnce(&mut AcceleratorConfig),
) -> Result<(), String> {
    // q7e3-F-13: delegate to core's shared lock_mutate_save; mutate_config keeps its always-save +
    // propagate policy (the closure always returns true).
    config::lock_mutate_save(config, |cfg| {
        f(cfg);
        true
    })
    .map_err(|e| e.to_string())
}
pub type AuthState = Arc<AuthorizationManager>;
pub type VerifiedSitesState = Arc<VerifiedSitesRegistry>;
/// Shared AppState so HTTPS servers spawned later (e.g. enabling Safari) get the full
/// state including auth_manager, config, and show_auth_popup — not a bare Default.
pub type SharedAppState = Arc<crate::server::AppState>;

/// Holds a pending, already-VERIFIED update so `respond_update_prompt` can use it directly instead of
/// re-checking the network. Storing a `VerifiedUpdate` (not a raw plugin `Update`) means the prompt
/// path physically cannot install anything that has not cleared both F-004 layers. Managed as Tauri
/// state.
pub type PendingUpdate = Arc<parking_lot::Mutex<Option<crate::updater::VerifiedUpdate>>>;

#[tauri::command]
pub fn get_config(config: tauri::State<'_, ConfigState>) -> AcceleratorConfig {
    config.read().clone()
}

#[tauri::command]
pub fn get_autostart_enabled(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        // F-010: refuse autostart entirely if this executable's path could inject into ANY OS launcher
        // serializer (systemd unit / .desktop / plist / Run-key) — BEFORE invoking the plugin (which would
        // otherwise serialize the unsafe path itself). Fail closed: leave a clean disabled state + surface
        // the refusal to the UI. (The webview cannot bypass this by calling the raw plugin enable — the
        // `autostart:allow-enable` capability grant is removed; only this gated command can enable.)
        let exe =
            std::env::current_exe().map_err(|e| format!("cannot resolve executable path: {e}"))?;
        if !crate::crash_recovery::autostart_path_is_safe(&exe) {
            let _ = manager.disable();
            crate::crash_recovery::disable_crash_recovery();
            return Err(
                "Executable path is unsafe for autostart (control/newline/non-UTF-8); refusing to enable."
                    .to_string(),
            );
        }
        manager.enable().map_err(|e| e.to_string())?;
        crate::crash_recovery::enable_crash_recovery();
    } else {
        manager.disable().map_err(|e| e.to_string())?;
        crate::crash_recovery::disable_crash_recovery();
    }
    Ok(())
}

#[tauri::command]
pub fn set_speed(
    config: tauri::State<'_, ConfigState>,
    speed: config::Speed,
) -> Result<(), String> {
    mutate_config(&config, |cfg| cfg.speed = speed)
}

#[tauri::command]
pub fn remove_approved_origin(
    config: tauri::State<'_, ConfigState>,
    origin: String,
) -> Result<(), String> {
    mutate_config(&config, |cfg| {
        cfg.approved_origins
            .retain(|o| o.as_str() != origin.as_str())
    })
}

#[derive(serde::Serialize)]
pub struct SystemInfo {
    pub platform: String,
    pub cpu_count: usize,
}

#[tauri::command]
pub fn get_system_info() -> SystemInfo {
    SystemInfo {
        platform: std::env::consts::OS.to_string(),
        cpu_count: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
    }
}

/// DTO returned to the authorization popup when an origin is on the recognized list.
/// `description` is intentionally NOT exposed — keep endorsement copy out of the popup.
#[derive(serde::Serialize)]
pub struct VerifiedSiteDto {
    pub display_name: String,
}

#[tauri::command]
pub fn get_verified_info(
    origin: String,
    state: tauri::State<'_, VerifiedSitesState>,
) -> Option<VerifiedSiteDto> {
    state.lookup(&origin).map(|s| VerifiedSiteDto {
        display_name: s.display_name.clone(),
    })
}

#[tauri::command]
pub fn respond_auth(
    app: tauri::AppHandle,
    auth: tauri::State<'_, AuthState>,
    request_id: String,
    origin: String,
    allowed: bool,
    remember: bool,
) {
    let decision = if allowed {
        AuthDecision::Allow { remember }
    } else {
        AuthDecision::Deny
    };
    // SEC-06: resolve by the opaque `request_id`, NOT the origin. A tampered/guessed payload with a
    // wrong id is a harmless no-op (it can't resolve a *different* concurrent request, and the old
    // origin-keyed resolve could); the real request then denies via its 60s timeout.
    auth.resolve(&request_id, decision);

    // Close the authorization popup window. SEC-06 post-impl (codex L3): the window is labelled by
    // `request_id`, NOT origin — origin-keying let a resolved request's stale 60s timeout close the
    // *live* window of a newer same-origin request (and respond_auth close the wrong one). `origin` is
    // kept on the payload for diagnostics only.
    tracing::debug!(origin = %origin, %request_id, allowed, "respond_auth decision");
    let label = format!("auth-{}", sanitize_window_label(&request_id));
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.close();
    }
}

/// Create a unique, collision-free window label from an arbitrary key (an origin or, for auth
/// popups, the opaque `request_id`). Uses a truncated SHA-256 hash to avoid collisions between
/// similar keys (e.g. `example.com` vs `example_com` would collide with naive character replacement)
/// and to keep the label charset window-system-safe.
pub fn sanitize_window_label(key: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(key.as_bytes());
    hex::encode(&hash[..6])
}

/// Enable Safari Support: generate certs, install trust, save config, start HTTPS.
/// The macOS Keychain trust prompt (native password dialog) is triggered by `security add-trusted-cert`.
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn enable_safari_support(
    config: tauri::State<'_, ConfigState>,
    shared_state: tauri::State<'_, SharedAppState>,
) -> Result<(), String> {
    use crate::certs;

    // SEC-08 (post-impl codex M1): the startup path runs this same fail-closed migration before it
    // brings up HTTPS (main.rs). Without mirroring it here, a Settings off→on toggle would re-enable
    // Safari HTTPS next to a readable legacy mint-any-cert key on upgraded installs — reopening exactly
    // the condition the startup gate closes. Fail closed: if the legacy key cannot be removed, refuse
    // to enable (surfaced to the Settings UI). HTTP is unaffected.
    certs::migrate_legacy_ca_key().map_err(|e| {
        format!("Legacy CA key could not be removed; refusing to enable Safari HTTPS: {e}")
    })?;

    certs::generate_and_save().map_err(|e| format!("Failed to generate certificates: {e}"))?;

    certs::install_ca_trust().map_err(|e| format!("Certificate trust was not granted: {e}"))?;

    // Save config
    {
        mutate_config(&config, |cfg| cfg.safari_support = true)?;
    }

    // Start HTTPS server with the full shared state (includes auth, config, popup callback)
    let tls_config =
        certs::load_rustls_config().map_err(|e| format!("Failed to load TLS config: {e}"))?;
    // The clone shares the Arc'd https_bound flag with the managed state, so start_https flipping it
    // after a successful bind is visible to /health — no https_port propagation needed. (Q7)
    crate::server::spawn_https((**shared_state).clone(), tls_config);

    tracing::info!("Safari Support enabled via Settings");
    Ok(())
}

/// Disable Safari Support: save config. HTTPS stops on next restart.
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn disable_safari_support(config: tauri::State<'_, ConfigState>) -> Result<(), String> {
    mutate_config(&config, |cfg| cfg.safari_support = false)?;
    tracing::info!("Safari Support disabled via Settings (HTTPS stops on next restart)");
    Ok(())
}

/// Stub for non-macOS platforms.
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub async fn enable_safari_support() -> Result<(), String> {
    Err("Safari Support is only available on macOS".to_string())
}

/// Stub for non-macOS platforms.
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn disable_safari_support() -> Result<(), String> {
    Err("Safari Support is only available on macOS".to_string())
}

/// Toggle auto-update preference from Settings.
#[tauri::command]
pub fn set_auto_update(config: tauri::State<'_, ConfigState>, enabled: bool) -> Result<(), String> {
    mutate_config(&config, |cfg| cfg.auto_update = Some(enabled))?;
    tracing::info!(enabled, "Auto-update preference changed via Settings");
    Ok(())
}

/// Called from the update prompt.
/// - action="update": save auto_update preference, then download + install using stored Update
/// - action="later": dismiss, auto_update stays None (prompt returns next launch)
#[tauri::command]
pub fn respond_update_prompt(
    app: tauri::AppHandle,
    config: tauri::State<'_, ConfigState>,
    pending: tauri::State<'_, PendingUpdate>,
    action: String,
    auto_update: bool,
) -> Result<(), String> {
    match action.as_str() {
        "update" => {
            // Save auto-update preference from the checkbox
            {
                // Q9 / Ask B (ship the fix): propagate the save error instead of swallowing it, so a
                // failed auto-update-preference write surfaces to the user rather than the update
                // silently proceeding on a stale preference. Rare (disk-write failure); the pending
                // update is left untaken on the early return, so a retry re-saves and updates.
                mutate_config(&config, |cfg| cfg.auto_update = Some(auto_update))?;
            }

            // Take the stored Update object — no redundant network re-check
            let update = pending.lock().take();
            match update {
                Some(update) => {
                    tracing::info!(
                        version = %update.version(),
                        auto_update,
                        "User clicked Update Now, downloading stored update"
                    );
                    let handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        crate::updater::perform_update(&handle, update).await;
                        // If perform_update returns (error), close the prompt
                        close_update_prompt(&handle);
                    });
                }
                None => {
                    tracing::warn!("No pending update found — may have expired. Closing prompt.");
                    close_update_prompt(&app);
                }
            }
        }
        "later" => {
            close_update_prompt(&app);
            tracing::info!("User clicked Remind Me Later");
        }
        _ => {
            close_update_prompt(&app);
        }
    }
    Ok(())
}

fn close_update_prompt(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("update-prompt") {
        let _ = window.close();
    }
}
