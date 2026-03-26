//! Window management for Settings, Authorization popup, and Update prompt.

use aztec_accelerator::authorization::{AuthDecision, AuthorizationManager};
use aztec_accelerator::commands;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Focus a newly created window. We stay as Accessory (tray-only) rather than
/// switching to Regular activation policy, which would show the app in the Dock
/// and Cmd+Tab. Trade-off: if the window gets buried behind a fullscreen app,
/// the user must click "Settings" in the tray again to refocus it.
/// If we ever want Dock presence, switch to Regular here and back to Accessory
/// on window destroy — but ensure the bundle icon is set (release builds only).
fn focus_window(window: &tauri::WebviewWindow) {
    let _ = window.set_focus();
}

/// Open or focus the Settings window.
pub fn open_settings_window(app: &AppHandle) {
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
        focus_window(&window);
    }
}

/// Show the authorization popup for an unknown origin.
/// Spawns a 60s timeout that auto-denies if the user doesn't respond.
/// If the user closes the window without responding, the timeout will still
/// fire and resolve all pending requests for this origin with Deny.
pub fn show_auth_popup_window(
    app: &AppHandle,
    origin: &str,
    auth_manager: &Arc<AuthorizationManager>,
) {
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
        focus_window(&window);
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

/// Show the update prompt window.
pub fn show_update_prompt_window(app: &AppHandle, current_version: &str, new_version: &str) {
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
        focus_window(&window);
    }
}
