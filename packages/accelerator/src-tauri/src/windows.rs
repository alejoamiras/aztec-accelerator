//! Window management for Settings, Authorization popup, and Update prompt.

use aztec_accelerator::authorization::{AuthDecision, AuthorizationManager};
use aztec_accelerator::commands;
use std::sync::Arc;
use tauri::webview::NewWindowResponse;
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
    /// Focus a NEWLY-built window? Settings/update/active-auth do; a QUEUED auth popup (C9 D18) does NOT —
    /// it is built not-topmost + unfocused and only gains focus when promoted to active.
    focus_on_create: bool,
}

/// Open a window with the given config, or handle an already-open one. Returns `Some(window)` for a
/// FRESHLY-built window (so the caller can do post-build work — attach a close listener, arm a timer),
/// or `None` if a window with `config.label` was already open (focused first iff `focus_if_open`).
/// Dedups the get-or-build pattern across the 3 windows.
fn open_or_focus_window(app: &AppHandle, config: WindowConfig) -> Option<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window(config.label) {
        if config.focus_if_open {
            let _ = window.set_focus();
        }
        return None;
    }
    match WebviewWindowBuilder::new(app, config.label, WebviewUrl::App(config.url.into()))
        .title(config.title)
        .inner_size(config.width, config.height)
        .resizable(false)
        .center()
        .always_on_top(config.always_on_top)
        // F-012 (codex HIGH-3): confine the webview to its own local asset origin. Block any attempt
        // to navigate off-origin (data-exfil / phishing) and deny opening new windows/webviews — the
        // popups never legitimately do either. (Confirmed NOT the cause of the CI asset-load failure —
        // that was `tauri dev` injecting a dev-server devUrl → empty embed; fixed by running the binary
        // directly. These guards allow the real tauri://localhost initial load.)
        .on_navigation(is_local_asset_url)
        .on_new_window(|_url, _features| NewWindowResponse::Deny)
        .build()
    {
        Ok(window) => {
            if config.focus_on_create {
                focus_window(&window);
            }
            Some(window)
        }
        Err(_) => None,
    }
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
            focus_on_create: true,
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
    // C9 (D18/D19): peek the arbiter. This popup is ACTIVE (owns the actionable + always-on-top slot)
    // only if the slot was free; otherwise it is QUEUED — built not-topmost + unfocused + WITHOUT a timer,
    // gaining focus + its 60 s clock only when promoted (`arm_active_popup`). Exactly one popup is
    // actionable at a time, and the arbiter (`resolve_active`) enforces that server-side too.
    let is_active = auth_manager
        .peek(request_id)
        .map(|(_, active)| active)
        .unwrap_or(false);
    // SEC-06: carry the opaque request_id so the popup echoes it back to respond_auth.
    let url = format!(
        "authorize.html?origin={}&requestId={}",
        urlencoding::encode(origin),
        urlencoding::encode(request_id)
    );
    let Some(window) = open_or_focus_window(
        app,
        WindowConfig {
            label: &label,
            url,
            title: "Authorize Site",
            width: 400.0,
            height: 300.0,
            always_on_top: is_active,
            focus_if_open: false,
            focus_on_create: is_active,
        },
    ) else {
        // A per-request auth label is NEVER "already open", so `None` here means the window FAILED TO
        // BUILD (code-review: resource exhaustion etc.). Don't leave the arbiter's `active` slot held with
        // no timer — that would stall the whole auth queue until the 600 s backstop. Resolve this request
        // Deny to release the slot and promote + raise the next queued popup.
        if let Some(promoted) = auth_manager.resolve(request_id, AuthDecision::Deny) {
            commands::arm_active_popup(app, auth_manager, &promoted);
        }
        return;
    };

    // C9 (D14): a user closing the popup WITHOUT deciding must resolve it (Deny) + promote the next queued
    // popup — closing is not a resolution event to the arbiter otherwise. Idempotent with the timer +
    // respond_auth (a second resolve of the same id is a no-op → returns None → no double-promote). The
    // arbiter helpers live in `commands` (the lib) so both this (bin) and `respond_auth` (lib) share them.
    commands::attach_close_deny_listener(app, auth_manager, &window, request_id);

    // C9 (D18): arm the 60 s auto-deny ONLY for the ACTIVE popup (activation-relative, not enqueue — a
    // queued popup is not starved of actionable time). A queued popup is armed when promoted.
    if is_active {
        commands::spawn_active_deny_timer(app, auth_manager, request_id);
    }
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
            focus_on_create: true,
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
