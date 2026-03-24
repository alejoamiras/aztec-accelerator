use crate::authorization::{AuthDecision, AuthorizationManager};
use crate::config::{self, AcceleratorConfig};
use std::sync::{Arc, RwLock};
use tauri::Manager;

pub type ConfigState = Arc<RwLock<AcceleratorConfig>>;
pub type AuthState = Arc<AuthorizationManager>;
/// Shared AppState so HTTPS servers spawned later (e.g. enabling Safari) get the full
/// state including auth_manager, config, and show_auth_popup — not a bare Default.
pub type SharedAppState = Arc<crate::server::AppState>;

#[tauri::command]
pub fn get_config(config: tauri::State<'_, ConfigState>) -> AcceleratorConfig {
    config.read().unwrap().clone()
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
        manager.enable().map_err(|e| e.to_string())?;
        crate::crash_recovery::enable_crash_recovery();
    } else {
        manager.disable().map_err(|e| e.to_string())?;
        crate::crash_recovery::disable_crash_recovery();
    }
    Ok(())
}

#[tauri::command]
pub fn set_speed(config: tauri::State<'_, ConfigState>, speed: String) -> Result<(), String> {
    let valid = ["full", "high", "balanced", "light", "low"];
    if !valid.contains(&speed.as_str()) {
        return Err(format!(
            "Invalid speed: {speed}. Must be one of: full, high, balanced, light, low"
        ));
    }
    let mut cfg = config.write().unwrap();
    cfg.speed = speed;
    config::save(&cfg).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn remove_approved_origin(
    config: tauri::State<'_, ConfigState>,
    origin: String,
) -> Result<(), String> {
    let mut cfg = config.write().unwrap();
    cfg.approved_origins.retain(|o| o != &origin);
    config::save(&cfg).map_err(|e| e.to_string())?;
    Ok(())
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

#[tauri::command]
pub fn respond_auth(
    app: tauri::AppHandle,
    auth: tauri::State<'_, AuthState>,
    origin: String,
    allowed: bool,
    remember: bool,
) {
    let decision = if allowed {
        AuthDecision::Allow { remember }
    } else {
        AuthDecision::Deny
    };
    auth.resolve(&origin, decision);

    // Close the authorization popup window
    let label = format!("auth-{}", sanitize_window_label(&origin));
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.close();
    }
}

/// Create a unique, collision-free window label from an origin string.
/// Uses a truncated SHA-256 hash to avoid collisions between similar origins
/// (e.g. `example.com` vs `example_com` would collide with naive character replacement).
pub fn sanitize_window_label(origin: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(origin.as_bytes());
    hash.iter().take(6).map(|b| format!("{b:02x}")).collect()
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
    use crate::server::HTTPS_PORT;

    certs::generate_and_save().map_err(|e| format!("Failed to generate certificates: {e}"))?;

    certs::install_ca_trust().map_err(|e| format!("Certificate trust was not granted: {e}"))?;

    // Save config
    {
        let mut cfg = config.write().unwrap();
        cfg.safari_support = true;
        config::save(&cfg).map_err(|e| e.to_string())?;
    }

    // Start HTTPS server with the full shared state (includes auth, config, popup callback)
    let tls_config =
        certs::load_rustls_config().map_err(|e| format!("Failed to load TLS config: {e}"))?;
    let mut state = (**shared_state).clone();
    state.https_port = Some(HTTPS_PORT);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::server::start_https(state, tls_config).await {
            tracing::error!("HTTPS server error: {e}");
        }
    });

    tracing::info!("Safari Support enabled via Settings");
    Ok(())
}

/// Disable Safari Support: save config. HTTPS stops on next restart.
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn disable_safari_support(config: tauri::State<'_, ConfigState>) -> Result<(), String> {
    let mut cfg = config.write().unwrap();
    cfg.safari_support = false;
    config::save(&cfg).map_err(|e| e.to_string())?;
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
