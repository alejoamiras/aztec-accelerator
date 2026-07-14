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
pub fn get_config(
    window: tauri::WebviewWindow,
    config: tauri::State<'_, ConfigState>,
) -> Result<AcceleratorConfig, String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    Ok(config.read().clone())
}

#[tauri::command]
pub fn get_autostart_enabled(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    use tauri_plugin_autostart::ManagerExt;
    Ok(app.autolaunch().is_enabled().unwrap_or(false))
}

#[tauri::command]
pub fn set_autostart(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    require_label(window.label(), SETTINGS_LABEL)?;
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
    window: tauri::WebviewWindow,
    config: tauri::State<'_, ConfigState>,
    speed: config::Speed,
) -> Result<(), String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    mutate_config(&config, |cfg| cfg.speed = speed)
}

#[tauri::command]
pub fn remove_approved_origin(
    window: tauri::WebviewWindow,
    config: tauri::State<'_, ConfigState>,
    origin: String,
) -> Result<(), String> {
    require_label(window.label(), SETTINGS_LABEL)?;
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
pub fn get_system_info(window: tauri::WebviewWindow) -> Result<SystemInfo, String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    Ok(SystemInfo {
        platform: std::env::consts::OS.to_string(),
        cpu_count: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
    })
}

/// DTO returned to the authorization popup when an origin is on the recognized list.
/// `description` is intentionally NOT exposed — keep endorsement copy out of the popup.
#[derive(serde::Serialize)]
pub struct VerifiedSiteDto {
    pub display_name: String,
}

#[tauri::command]
pub fn get_verified_info(
    window: tauri::WebviewWindow,
    origin: String,
    state: tauri::State<'_, VerifiedSitesState>,
) -> Result<Option<VerifiedSiteDto>, String> {
    // F-012 (D6): only an authorization popup renders the verified badge.
    require_auth_window(window.label())?;
    Ok(state.lookup(&origin).map(|s| VerifiedSiteDto {
        display_name: s.display_name.clone(),
    }))
}

#[tauri::command]
pub fn respond_auth(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    auth: tauri::State<'_, AuthState>,
    request_id: String,
    origin: String,
    allowed: bool,
    remember: bool,
) -> Result<(), String> {
    // F-012 (D6) + SEC-06 (strengthened): bind the calling window to THIS request_id. windows.rs labels
    // the popup `auth-{hash(request_id)}`; asserting the caller's label == that means a popup opened for
    // request A physically cannot resolve request B, even if it forged B's id in the payload. Tauri
    // resolves `window` from the native IPC message, so the label is unspoofable from JS.
    let label = format!("{AUTH_LABEL_PREFIX}{}", sanitize_window_label(&request_id));
    require_label(window.label(), &label)?;

    let decision = if allowed {
        AuthDecision::Allow { remember }
    } else {
        AuthDecision::Deny
    };
    // SEC-06: resolve by the opaque `request_id`, NOT the origin. A tampered/guessed payload with a
    // wrong id is a harmless no-op (it can't resolve a *different* concurrent request, and the old
    // origin-keyed resolve could); the real request then denies via its 60s timeout.
    auth.resolve(&request_id, decision);

    // Close the authorization popup window (labelled by `request_id`, not origin — see windows.rs).
    // `origin` is kept on the payload for diagnostics only.
    tracing::debug!(origin = %origin, %request_id, allowed, "respond_auth decision");
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.close();
    }
    Ok(())
}

/// Create a unique, collision-free window label from an arbitrary key (an origin or, for auth
/// popups, the opaque `request_id`). Uses a truncated SHA-256 hash to avoid collisions between
/// similar keys (e.g. `example.com` vs `example_com` would collide with naive character replacement)
/// and to keep the label charset window-system-safe.
///
/// F-012 (codex MED-6): 16 bytes = 128 bits of the digest. The label is a security binding
/// (`respond_auth` asserts the caller's window == `auth-{hash(request_id)}`), so the earlier 6-byte
/// (48-bit) truncation gave a needlessly small margin; 128 bits makes a collision a non-issue while
/// staying a valid Tauri window label (lowercase hex).
pub fn sanitize_window_label(key: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(key.as_bytes());
    hex::encode(&hash[..16])
}

/// Fixed window labels the caller-label guard checks against (must match `windows.rs`).
pub const SETTINGS_LABEL: &str = "settings";
pub const UPDATE_PROMPT_LABEL: &str = "update-prompt";
const AUTH_LABEL_PREFIX: &str = "auth-";

/// F-012 (D6) — the PRIMARY, framework-independent caller-label check behind the per-window capability
/// ACL. Even if a capability were ever mis-scoped, a command still refuses to act for the wrong window.
/// `actual` is the real invoking window's label, which Tauri resolves from the native IPC message — JS
/// cannot spoof it. On mismatch: log (generic) and return a generic error that leaks no window topology.
fn require_label(actual: &str, expected: &str) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        tracing::warn!(
            actual,
            "command invoked from an unexpected window; rejecting"
        );
        Err("This command is not available from this window.".to_string())
    }
}

/// True iff `label` is a well-formed authorization-popup label: `auth-` + exactly 32 lowercase hex chars
/// (the 128-bit [`sanitize_window_label`] digest). Used by `get_verified_info`, which — unlike
/// `respond_auth` — doesn't receive the `request_id`, so it can only assert the caller IS an auth popup.
fn is_auth_label(label: &str) -> bool {
    label
        .strip_prefix(AUTH_LABEL_PREFIX)
        .is_some_and(|h| h.len() == 32 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// Require the caller to be an authorization popup (any `auth-<hash>`). Generic error on mismatch.
fn require_auth_window(actual: &str) -> Result<(), String> {
    if is_auth_label(actual) {
        Ok(())
    } else {
        tracing::warn!(
            actual,
            "auth command invoked from a non-auth window; rejecting"
        );
        Err("This command is not available from this window.".to_string())
    }
}

/// Enable Safari Support: generate certs, install trust, save config, start HTTPS.
/// The macOS Keychain trust prompt (native password dialog) is triggered by `security add-trusted-cert`.
#[cfg(target_os = "macos")]
#[tauri::command]
pub async fn enable_safari_support(
    window: tauri::WebviewWindow,
    config: tauri::State<'_, ConfigState>,
    shared_state: tauri::State<'_, SharedAppState>,
) -> Result<(), String> {
    require_label(window.label(), SETTINGS_LABEL)?;
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
pub fn disable_safari_support(
    window: tauri::WebviewWindow,
    config: tauri::State<'_, ConfigState>,
) -> Result<(), String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    mutate_config(&config, |cfg| cfg.safari_support = false)?;
    tracing::info!("Safari Support disabled via Settings (HTTPS stops on next restart)");
    Ok(())
}

/// Stub for non-macOS platforms.
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub async fn enable_safari_support(window: tauri::WebviewWindow) -> Result<(), String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    Err("Safari Support is only available on macOS".to_string())
}

/// Stub for non-macOS platforms.
#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn disable_safari_support(window: tauri::WebviewWindow) -> Result<(), String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    Err("Safari Support is only available on macOS".to_string())
}

/// Toggle auto-update preference from Settings.
#[tauri::command]
pub fn set_auto_update(
    window: tauri::WebviewWindow,
    config: tauri::State<'_, ConfigState>,
    enabled: bool,
) -> Result<(), String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    mutate_config(&config, |cfg| cfg.auto_update = Some(enabled))?;
    tracing::info!(enabled, "Auto-update preference changed via Settings");
    Ok(())
}

/// Called from the update prompt.
/// - action="update": save auto_update preference, then download + install using stored Update
/// - action="later": dismiss, auto_update stays None (prompt returns next launch)
#[tauri::command]
pub fn respond_update_prompt(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    config: tauri::State<'_, ConfigState>,
    pending: tauri::State<'_, PendingUpdate>,
    action: String,
    auto_update: bool,
) -> Result<(), String> {
    require_label(window.label(), UPDATE_PROMPT_LABEL)?;
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

#[cfg(test)]
mod tests {
    use super::{is_auth_label, require_auth_window, require_label, sanitize_window_label};

    #[test]
    fn require_label_matches_exactly() {
        assert!(require_label("settings", "settings").is_ok());
        assert!(require_label("auth-abc", "settings").is_err());
        assert!(require_label("", "settings").is_err());
        assert!(require_label("settings ", "settings").is_err()); // no trimming
    }

    #[test]
    fn auth_label_is_prefix_plus_128bit_lowercase_hex() {
        // A real label is `auth-` + the 32-hex (16-byte) digest — the width sanitize_window_label emits.
        let real = format!("auth-{}", sanitize_window_label("some-request-id"));
        assert_eq!(sanitize_window_label("some-request-id").len(), 32); // 16 bytes -> 32 hex
        assert!(is_auth_label(&real));
        assert!(require_auth_window(&real).is_ok());

        // Reject: wrong prefix, uppercase hex, wrong length, non-hex, the settings label.
        for bad in [
            "settings",
            "auth-",
            "auth-XYZ",
            "auth-ABCDEF0123456789ABCDEF0123456789", // uppercase
            "auth-abc",                              // too short
            "auth-0123456789abcdef0123456789abcdefff", // too long (34)
            "notauth-0123456789abcdef0123456789abcdef",
        ] {
            assert!(!is_auth_label(bad), "{bad} must not be a valid auth label");
            assert!(require_auth_window(bad).is_err(), "{bad} must be rejected");
        }
    }

    #[test]
    fn sanitize_window_label_is_deterministic_and_collision_resistant() {
        assert_eq!(sanitize_window_label("a"), sanitize_window_label("a"));
        assert_ne!(
            sanitize_window_label("example.com"),
            sanitize_window_label("example_com")
        );
        assert!(sanitize_window_label("x")
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
    }
}
