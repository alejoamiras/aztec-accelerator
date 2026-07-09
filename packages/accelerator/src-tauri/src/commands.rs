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

/// Holds a pending update so `respond_update_prompt` can use it directly
/// instead of re-checking the network. Managed as Tauri state.
pub type PendingUpdate = Arc<parking_lot::Mutex<Option<tauri_plugin_updater::Update>>>;

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
    set_autostart_inner(&app, enabled)
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

/// Enable the encrypted (HTTPS) connection: generate certs, install browser trust, save config, start
/// HTTPS. Cross-platform via [`crate::trust`] — macOS raises the Keychain password dialog, Linux
/// installs into user NSS stores silently, Windows lands in a later phase (its trust backend errors
/// until then). Succeeds iff trust landed in ≥1 store (`install_ca_trust` errors otherwise), so on
/// Linux a missing `certutil` surfaces as an enable failure the wizard can show with a Retry (R3).
#[tauri::command]
pub async fn enable_https(
    config: tauri::State<'_, ConfigState>,
    shared_state: tauri::State<'_, SharedAppState>,
) -> Result<(), String> {
    enable_https_inner(&config, &shared_state)
}

/// The shared enable-HTTPS routine, callable outside a Tauri command (the onboarding wizard reuses
/// it). Generate certs → install browser trust → save config → start HTTPS. Errors iff trust landed
/// in zero stores (R3), so the wizard can render HTTPS as failed-with-Retry.
fn enable_https_inner(
    config: &ConfigState,
    shared_state: &crate::server::AppState,
) -> Result<(), String> {
    use crate::certs;

    // SEC-08 (post-impl codex M1): the startup path runs this same fail-closed migration before it
    // brings up HTTPS (main.rs). Without mirroring it here, a Settings off→on toggle would re-enable
    // HTTPS next to a readable legacy mint-any-cert key on upgraded installs — reopening exactly
    // the condition the startup gate closes. Fail closed: if the legacy key cannot be removed, refuse
    // to enable (surfaced to the Settings UI). HTTP is unaffected. (No-op on installs that never had
    // an on-disk CA key, i.e. every non-macOS install.)
    certs::migrate_legacy_ca_key().map_err(|e| {
        format!("Legacy CA key could not be removed; refusing to enable HTTPS: {e}")
    })?;

    certs::generate_and_save().map_err(|e| format!("Failed to generate certificates: {e}"))?;

    certs::install_ca_trust().map_err(|e| format!("Certificate trust was not granted: {e}"))?;

    mutate_config(config, |cfg| cfg.https_enabled = true)?;

    // Start HTTPS server with the full shared state (includes auth, config, popup callback)
    let tls_config =
        certs::load_rustls_config().map_err(|e| format!("Failed to load TLS config: {e}"))?;
    // The clone shares the Arc'd https_bound flag with the managed state, so start_https flipping it
    // after a successful bind is visible to /health — no https_port propagation needed. (Q7)
    crate::server::spawn_https(shared_state.clone(), tls_config);

    tracing::info!("HTTPS enabled");
    Ok(())
}

/// Shared autostart toggle (used by the Settings toggle + the onboarding wizard). Enables/disables
/// the OS autostart entry and the paired crash-recovery mechanism.
fn set_autostart_inner(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|e| e.to_string())?;
        crate::crash_recovery::enable_crash_recovery();
    } else {
        manager.disable().map_err(|e| e.to_string())?;
        crate::crash_recovery::disable_crash_recovery();
    }
    Ok(())
}

/// Disable the encrypted (HTTPS) connection: save config off. HTTPS stops on next restart. Trust
/// anchors are left in place (removing them is the separate [`remove_https_trust`] action, so a
/// re-enable doesn't re-prompt — D5/A4).
#[tauri::command]
pub fn disable_https(config: tauri::State<'_, ConfigState>) -> Result<(), String> {
    mutate_config(&config, |cfg| cfg.https_enabled = false)?;
    tracing::info!("HTTPS disabled via Settings (HTTPS stops on next restart)");
    Ok(())
}

/// Per-store browser-trust status for the local CA (drives the honest Settings/wizard status list).
#[tauri::command]
pub fn get_trust_status() -> crate::trust::TrustReport {
    crate::trust::trust_status(&crate::certs::live_ca_cert_path())
}

/// Explicitly remove the local CA from every browser trust store (the "Remove certificate trust"
/// Settings action — D5). Also flips HTTPS off so the app stops presenting a now-untrusted cert.
#[tauri::command]
pub fn remove_https_trust(config: tauri::State<'_, ConfigState>) -> Result<(), String> {
    let report = crate::trust::remove_ca_trust(&crate::certs::live_ca_cert_path());
    mutate_config(&config, |cfg| cfg.https_enabled = false)?;
    tracing::info!(
        removed = report.stores.len(),
        "Removed CA trust via Settings"
    );
    Ok(())
}

// ── First-run onboarding wizard ──

/// Prefill state for the onboarding wizard. `https_default` is ALWAYS `true` — the HTTPS toggle is
/// pre-checked for everyone, including upgraders who never had it, to move the whole installed base
/// onto the encrypted path (A9 / plan §2.1). Autostart + auto-update reflect current state so the
/// wizard shows an upgrader their real settings.
#[derive(serde::Serialize)]
pub struct OnboardingState {
    pub platform: String,
    pub https_default: bool,
    pub autostart_enabled: bool,
    pub auto_update: Option<bool>,
    pub trust_status: crate::trust::TrustReport,
}

#[tauri::command]
pub fn get_onboarding_state(
    app: tauri::AppHandle,
    config: tauri::State<'_, ConfigState>,
) -> OnboardingState {
    use tauri_plugin_autostart::ManagerExt;
    let auto_update = config.read().auto_update;
    OnboardingState {
        platform: std::env::consts::OS.to_string(),
        https_default: true,
        autostart_enabled: app.autolaunch().is_enabled().unwrap_or(false),
        auto_update,
        trust_status: crate::trust::trust_status(&crate::certs::live_ca_cert_path()),
    }
}

/// Per-action result of the wizard's "Start". Each action runs INDEPENDENTLY — a failure in one
/// (e.g. the cert install) does not abort the others. `Result<(),String>` serializes as
/// `{"Ok":null}` / `{"Err":"…"}` for the frontend to render per-row ✓/✗.
#[derive(serde::Serialize)]
pub struct OnboardingResult {
    pub https: Result<(), String>,
    pub autostart: Result<(), String>,
    pub auto_update: Result<(), String>,
    /// Whether the once-per-version onboarding marker was set (true iff every requested action ok).
    pub completed: bool,
}

/// Execute the wizard's choices. Each runs independently; the onboarding marker is set ONLY when all
/// requested actions succeed (marker discipline, R4). A failed HTTPS leaves the marker unset so the
/// wizard returns next launch — unless the user explicitly dismisses via [`dismiss_onboarding`].
#[tauri::command]
pub async fn complete_onboarding(
    app: tauri::AppHandle,
    config: tauri::State<'_, ConfigState>,
    shared_state: tauri::State<'_, SharedAppState>,
    https: bool,
    autostart: bool,
    auto_update: bool,
) -> Result<OnboardingResult, String> {
    let https_res = if https {
        enable_https_inner(&config, &shared_state)
    } else {
        Ok(())
    };
    let autostart_res = set_autostart_inner(&app, autostart);
    let auto_update_res = mutate_config(&config, |cfg| cfg.auto_update = Some(auto_update));

    let all_ok = https_res.is_ok() && autostart_res.is_ok() && auto_update_res.is_ok();
    let completed = all_ok
        && mutate_config(&config, |cfg| {
            cfg.onboarding_version = crate::config::ONBOARDING_VERSION
        })
        .is_ok();

    Ok(OnboardingResult {
        https: https_res,
        autostart: autostart_res,
        auto_update: auto_update_res,
        completed,
    })
}

/// Mark onboarding complete without (further) action — the explicit "Continue without HTTPS" (after a
/// failed cert install) and "Skip for now" paths (R4). The ONLY unconditional marker set.
#[tauri::command]
pub fn dismiss_onboarding(config: tauri::State<'_, ConfigState>) -> Result<(), String> {
    mutate_config(&config, |cfg| {
        cfg.onboarding_version = crate::config::ONBOARDING_VERSION
    })?;
    tracing::info!("Onboarding dismissed (marker set)");
    Ok(())
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
                        version = %update.version,
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
