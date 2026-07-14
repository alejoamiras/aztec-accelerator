//! Window management for Settings, Authorization popup, and Update prompt.

use aztec_accelerator::authorization::{AuthDecision, AuthorizationManager};
use aztec_accelerator::commands;
use std::sync::Arc;
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};

/// True iff `url` is the app's OWN local asset origin. Tauri serves the bundled frontend from
/// `tauri://localhost` (Linux/macOS) or `http://tauri.localhost` (Windows). Every other navigation
/// target is off-origin. F-012 (codex HIGH-3): the CSP `connect-src` blocks fetch/XHR/WS exfil but NOT
/// a top-level navigation that smuggles data in the URL, and on Linux the `<meta>`-delivered CSP ignores
/// `frame-ancestors` — so this Rust guard is the real anti-navigation/anti-exfil control.
///
/// GATE-3 (codex MED): match ONLY the current platform's asset origin — accepting `http://tauri.localhost`
/// on Linux/macOS would permit a real loopback HTTP navigation (e.g. `http://tauri.localhost:59833/?data=…`
/// hitting a listening server) that is NOT the embedded-asset protocol. Also reject any embedded credentials
/// or explicit port; the asset origin never has either.
#[allow(dead_code)] // TEMP: on_navigation call removed while debugging the CI asset-load failure.
fn is_local_asset_url(url: &Url) -> bool {
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return false;
    }
    #[cfg(windows)]
    {
        url.scheme() == "http" && url.host_str() == Some("tauri.localhost")
    }
    #[cfg(not(windows))]
    {
        url.scheme() == "tauri" && url.host_str() == Some("localhost")
    }
}

/// Focus a newly created window. We stay as Accessory (tray-only) rather than
/// switching to Regular activation policy, which would show the app in the Dock
/// and Cmd+Tab. Trade-off: if the window gets buried behind a fullscreen app,
/// the user must click "Settings" in the tray again to refocus it.
/// If we ever want Dock presence, switch to Regular here and back to Accessory
/// on window destroy — but ensure the bundle icon is set (release builds only).
fn focus_window(window: &tauri::WebviewWindow) {
    let _ = window.set_focus();
}

/// Parameters for [`open_or_focus_window`]. `url`, `label` are owned/borrowed as the call site needs
/// (the auth/update labels + URLs are built per-call; settings' are static).
struct WindowConfig<'a> {
    label: &'a str,
    url: String,
    title: &'a str,
    width: f64,
    height: f64,
    always_on_top: bool,
    /// Focus an ALREADY-open window with this label? Settings does (user re-clicked "Settings");
    /// the auth/update popups don't (they just stay put). Preserves prior per-window behavior.
    focus_if_open: bool,
}

/// Open a window with the given config, or handle an already-open one. Returns `true` iff a window
/// with `config.label` was ALREADY open (focused first iff `focus_if_open`); `false` if none existed,
/// in which case a new one is built + focused. The bool lets callers (auth) do post-build work only
/// for a freshly-handled window. Dedups the get-or-build pattern across the 3 windows.
fn open_or_focus_window(app: &AppHandle, config: WindowConfig) -> bool {
    if let Some(window) = app.get_webview_window(config.label) {
        if config.focus_if_open {
            let _ = window.set_focus();
        }
        return true;
    }
    if let Ok(window) =
        WebviewWindowBuilder::new(app, config.label, WebviewUrl::App(config.url.into()))
            .title(config.title)
            .inner_size(config.width, config.height)
            .resizable(false)
            .center()
            .always_on_top(config.always_on_top)
            // F-012 (codex HIGH-3): confine the webview to its own local asset origin + deny new windows.
            // TEMP: removed to test whether these guards cancel the initial tauri:// asset load in CI
            // (probe proved the embed is complete, yet the webview can't load settings.html). Re-add with
            // a corrected matcher once the root cause is confirmed.
            // .on_navigation(is_local_asset_url)
            // .on_new_window(|_url, _features| NewWindowResponse::Deny)
            .build()
    {
        focus_window(&window);
    }
    false
}

/// Open or focus the Settings window.
pub fn open_settings_window(app: &AppHandle) {
    open_or_focus_window(
        app,
        WindowConfig {
            label: "settings",
            url: "settings.html".to_string(),
            title: "Aztec Accelerator Settings",
            width: 500.0,
            height: 520.0,
            always_on_top: false,
            focus_if_open: true,
        },
    );
}

/// Show the authorization popup for an unknown origin.
/// Spawns a 60s timeout that auto-denies if the user doesn't respond.
/// If the user closes the window without responding, the timeout will still
/// fire and resolve THIS request (by `request_id`) with Deny.
pub fn show_auth_popup_window(
    app: &AppHandle,
    origin: &str,
    request_id: &str,
    auth_manager: &Arc<AuthorizationManager>,
) {
    // SEC-06 post-impl (codex L3): label the window by `request_id`, NOT origin. Origin-keying made a
    // resolved request's stale 60s timeout (and respond_auth) close the *live* window of a newer
    // same-origin request that reused the label. Only the first pending request per origin shows a
    // popup (piggyback gate in server/auth.rs), so per-request labels never duplicate a popup.
    let label = format!("auth-{}", commands::sanitize_window_label(request_id));
    // SEC-06: carry the opaque request_id so the popup echoes it back to respond_auth.
    let url = format!(
        "authorize.html?origin={}&requestId={}",
        urlencoding::encode(origin),
        urlencoding::encode(request_id)
    );
    if open_or_focus_window(
        app,
        WindowConfig {
            label: &label,
            url,
            title: "Authorize Site",
            width: 400.0,
            height: 300.0,
            always_on_top: true,
            focus_if_open: false,
        },
    ) {
        return; // popup already open for this request — don't spawn a duplicate timeout
    }

    // Spawn 60s timeout — always resolve with Deny if still pending.
    // This handles both: (a) user ignoring the popup, and (b) user closing the
    // window without clicking Allow/Deny. In case (b), the window is gone but
    // the pending sender is still in the map. resolve() is a no-op if this
    // request was already resolved by respond_auth (sender already consumed).
    let app_handle = app.clone();
    let origin_owned = origin.to_string();
    let request_id_owned = request_id.to_string();
    let auth_manager = auth_manager.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(aztec_accelerator::server::AUTH_DECISION_TIMEOUT).await;
        // Close window if still open
        if let Some(window) = app_handle.get_webview_window(&label) {
            let _ = window.close();
        }
        // SEC-06: resolve by request_id — no-op if respond_auth already consumed it.
        auth_manager.resolve(&request_id_owned, AuthDecision::Deny);
        tracing::debug!(origin = %origin_owned, "Authorization timeout cleanup");
    });
}

/// Show the update prompt window.
///
/// Only called from the background update check, which is compiled out for
/// `webdriver` builds — so this is too, to keep those builds warning-clean.
#[cfg(not(feature = "webdriver"))]
pub fn show_update_prompt_window(app: &AppHandle, current_version: &str, new_version: &str) {
    let url = format!(
        "update-prompt.html?current={}&version={}",
        urlencoding::encode(current_version),
        urlencoding::encode(new_version)
    );
    open_or_focus_window(
        app,
        WindowConfig {
            label: "update-prompt",
            url,
            title: "Aztec Accelerator Update",
            width: 420.0,
            height: 280.0,
            always_on_top: false,
            focus_if_open: false,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::is_local_asset_url;
    use tauri::Url;

    #[test]
    fn navigation_guard_allows_only_the_local_asset_origin() {
        // The real initial loads for THIS platform (incl. query params) must be permitted.
        #[cfg(not(windows))]
        let ok_forms = [
            "tauri://localhost/authorize.html?origin=https%3A%2F%2Fx.com&requestId=abc",
            "tauri://localhost/settings.html",
        ];
        #[cfg(windows)]
        let ok_forms = [
            "http://tauri.localhost/authorize.html?origin=https%3A%2F%2Fx.com&requestId=abc",
            "http://tauri.localhost/settings.html",
        ];
        for ok in ok_forms {
            assert!(
                is_local_asset_url(&Url::parse(ok).unwrap()),
                "{ok} should be allowed"
            );
        }

        // The OTHER platform's origin must be REJECTED here (codex MED: `http://tauri.localhost` on
        // Linux/macOS is a real loopback HTTP navigation, not the embedded-asset protocol).
        #[cfg(not(windows))]
        let other_platform = "http://tauri.localhost/settings.html";
        #[cfg(windows)]
        let other_platform = "tauri://localhost/settings.html";
        assert!(
            !is_local_asset_url(&Url::parse(other_platform).unwrap()),
            "{other_platform} (other platform's origin) must be blocked"
        );

        // Ports and credentials are never part of the asset origin.
        #[cfg(not(windows))]
        let ported_credentialed = [
            "tauri://localhost:59833/settings.html",
            "tauri://user@localhost/settings.html",
            "tauri://user:pass@localhost/settings.html",
        ];
        #[cfg(windows)]
        let ported_credentialed = [
            "http://tauri.localhost:59833/settings.html",
            "http://user@tauri.localhost/settings.html",
            "http://user:pass@tauri.localhost/settings.html",
        ];
        for bad in ported_credentialed {
            assert!(
                !is_local_asset_url(&Url::parse(bad).unwrap()),
                "{bad} (port/creds) must be blocked"
            );
        }

        // Everything off-origin — look-alikes, the IPC host, non-http(s) schemes — must be blocked.
        for bad in [
            "https://evil.example/",
            "http://localhost/x",                   // wrong host
            "https://tauri.localhost/",             // wrong scheme
            "http://tauri.localhost.evil.example/", // suffix look-alike
            "tauri://evil/",
            "http://ipc.localhost/", // IPC host is not a navigation target
            "data:text/html,<script>alert(1)</script>",
            "file:///etc/passwd",
            "javascript:alert(1)",
        ] {
            assert!(
                !is_local_asset_url(&Url::parse(bad).unwrap()),
                "{bad} should be blocked"
            );
        }
    }
}
