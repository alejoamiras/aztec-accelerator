//! Auto-update logic shared between the Tauri app (main.rs) and commands.
//!
//! The background loop in main.rs calls `check_for_update()` periodically.
//! When the user clicks "Update Now" in the prompt, `respond_update_prompt`
//! calls `perform_update()` directly — no redundant network re-check.

use crate::commands::ConfigState;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

/// Check for updates and act based on the user's auto_update preference.
/// Returns the `Update` if one is available and the user hasn't opted into auto-update
/// (so the caller can show a prompt or store it for later use).
pub async fn check_for_update(
    app: &AppHandle,
    config_state: &ConfigState,
) -> Option<tauri_plugin_updater::Update> {
    tracing::info!("Checking for updates...");
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!("Failed to build updater: {e}");
            return None;
        }
    };

    let update = match updater.check().await {
        Ok(Some(update)) => update,
        Ok(None) => {
            tracing::info!("No update available");
            return None;
        }
        Err(e) => {
            tracing::warn!("Update check failed: {e}");
            return None;
        }
    };

    let new_version = update.version.clone();
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    tracing::info!(current = %current_version, new = %new_version, "Update available");

    let auto_update_pref = { config_state.read().auto_update };
    tracing::info!(?auto_update_pref, "Auto-update preference");

    match auto_update_pref {
        Some(true) => {
            tracing::info!("Auto-update enabled, performing update");
            perform_update(app, update).await;
            None
        }
        _ => {
            // None (never asked) or Some(false) (manual) — return the update
            // so the caller can show a prompt or add a tray menu item
            Some(update)
        }
    }
}

/// Hard ceiling on the auto-update artifact size (SEC-03). Real DMG/AppImage/NSIS artifacts are tens
/// of MB; 500 MB is generous headroom that still stops a multi-GB memory blow-up.
const MAX_UPDATE_BYTES: u64 = 500 * 1024 * 1024;

/// Pure size lookup over a `latest.json` value: the `size` of the `platforms.*` entry whose `url`
/// matches `download_url`. `None` if absent. Split out from [`advertised_update_size`] so it can be
/// unit-tested without constructing a plugin `Update` (which has private fields).
fn size_from_feed(raw_json: &serde_json::Value, download_url: &str) -> Option<u64> {
    raw_json
        .get("platforms")?
        .as_object()?
        .values()
        .find(|p| p.get("url").and_then(|u| u.as_str()) == Some(download_url))
        .and_then(|p| p.get("size").and_then(serde_json::Value::as_u64))
}

/// The advertised artifact size for THIS platform, read from the feed JSON (`latest.json`) by matching
/// the download URL. `None` if the feed omits `size` (older feeds). The feed is the same JSON the
/// plugin will signature-check, so a feed declaring a huge size is rejected before the plugin buffers
/// a single byte.
fn advertised_update_size(update: &tauri_plugin_updater::Update) -> Option<u64> {
    size_from_feed(&update.raw_json, update.download_url.as_str())
}

/// Download, verify Ed25519 signature, install, and restart the app.
pub async fn perform_update(app: &AppHandle, update: tauri_plugin_updater::Update) {
    tracing::info!(version = %update.version, "Downloading update");

    // SEC-03: pre-flight size cap. The plugin buffers the WHOLE artifact into memory before it
    // verifies the signature, and its progress callback cannot abort that loop — so a tampered feed
    // pointing at a multi-GB blob is a memory-DoS. Reject up front when the feed's advertised `size`
    // exceeds the ceiling, BEFORE `download()`. This keeps the plugin's verified download path intact
    // (no hand-rolled crypto). Availability-only: minisign still rejects tampered *bytes*; this just
    // stops the buffer blow-up.
    //
    // Residual when the feed OMITS `size` (codex post-impl M2, tracked): the `None` arm proceeds. Be
    // honest about why this is weaker than it looks — for the *availability* property the signature
    // check is NOT the control, because the plugin buffers BEFORE it verifies. So an attacker who can
    // only tamper the *manifest* (strip `size`, point the URL at a huge blob) re-opens the memory-DoS
    // WITHOUT needing the signing key. The clean fix is to make `size` mandatory and fail closed on
    // absence; it is deferred because the live prod `latest.json` is still size-less (PR-4 made the
    // release workflow emit + assert `size` for all future cuts, but in-flight/served feeds predate
    // it), so flipping now would brick auto-update until every served feed carries size. A
    // self-managed ranged/HEAD Content-Length probe was rejected in audit R3 (do not reshape the
    // verified download path). Flip to fail-closed once the served feed is confirmed to carry `size`
    // across all supported update paths. See the tracking issue.
    match advertised_update_size(&update) {
        Some(size) if size > MAX_UPDATE_BYTES => {
            tracing::error!(
                size,
                max = MAX_UPDATE_BYTES,
                "Update artifact exceeds the size cap; refusing to download"
            );
            return;
        }
        Some(size) => tracing::info!(size, "Update artifact size within cap"),
        None => {
            tracing::warn!("Update feed omits artifact size; size cap not enforced for this update")
        }
    }

    // Download first (separate from install) so crash-recovery stays armed through the whole
    // download/verify span — a mid-download crash is still recovered.
    let bytes = match update
        .download(
            |chunk_length, content_length| {
                tracing::info!(
                    chunk_length,
                    content_length = content_length.unwrap_or(0),
                    "Download progress"
                );
            },
            || tracing::info!("Download complete"),
        )
        .await
    {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("Update download failed: {e}");
            return;
        }
    };

    // Windows: disarm the always-armed repeating crash-recovery task right before install. A
    // tick during NSIS file mutation could spawn the exe mid-update (lock the file being
    // replaced / launch a half-written binary). If we CANNOT verify the task is gone, do NOT
    // install — the race would be live; skip this attempt (the app keeps running on the current
    // version, and the next check retries). disable returns true if never armed (autostart off).
    #[cfg(target_os = "windows")]
    if !crate::crash_recovery::disable_crash_recovery() {
        tracing::error!(
            "Aborting update install: could not disarm crash-recovery task (race risk)"
        );
        // The app keeps running on the current version, and disarm may have PARTIALLY succeeded
        // (/Delete worked but /Query couldn't confirm), so recovery could now be off. Restore it
        // before bailing out — every path that leaves the app running must end armed.
        rearm_crash_recovery_if_enabled(app);
        return;
    }

    match update.install(bytes) {
        Ok(()) => {
            // Re-arm BEFORE restarting: a failed relaunch must not leave recovery off while
            // autostart is on. IgnoreNew + the exit-0-if-healthy guard absorb any brief
            // double-launch with the restarted build.
            #[cfg(target_os = "windows")]
            rearm_crash_recovery_if_enabled(app);
            tracing::info!("Update installed, restarting");
            app.restart();
        }
        Err(e) => {
            tracing::error!("Update install failed: {e}");
            // The app keeps running, so crash-recovery must resume (only if it should be armed).
            #[cfg(target_os = "windows")]
            rearm_crash_recovery_if_enabled(app);
        }
    }
}

/// Re-arm the Windows crash-recovery task iff it should be armed (autostart on). Idempotent —
/// `enable_crash_recovery` overwrites any existing task.
#[cfg(target_os = "windows")]
fn rearm_crash_recovery_if_enabled(app: &AppHandle) {
    use tauri_plugin_autostart::ManagerExt;
    if app.autolaunch().is_enabled().unwrap_or(false) {
        crate::crash_recovery::enable_crash_recovery();
    }
}

#[cfg(test)]
mod tests {
    use super::size_from_feed;
    use serde_json::json;

    fn feed(aarch64_size: Option<u64>) -> serde_json::Value {
        let mut plat = json!({ "signature": "sig", "url": "https://x.test/app-aarch64.tar.gz" });
        if let Some(s) = aarch64_size {
            plat["size"] = json!(s);
        }
        json!({
            "version": "1.0.5",
            "platforms": {
                "darwin-aarch64": plat,
                "linux-x86_64": { "signature": "s2", "url": "https://x.test/app-linux", "size": 123 },
            }
        })
    }

    #[test]
    fn size_from_feed_matches_url() {
        let f = feed(Some(42_000_000));
        assert_eq!(
            size_from_feed(&f, "https://x.test/app-aarch64.tar.gz"),
            Some(42_000_000)
        );
        // A different platform's URL resolves to that platform's size.
        assert_eq!(size_from_feed(&f, "https://x.test/app-linux"), Some(123));
    }

    #[test]
    fn size_from_feed_none_when_absent_or_unmatched() {
        // Matched platform omits size → None (older feed; cap not enforced for it).
        assert_eq!(
            size_from_feed(&feed(None), "https://x.test/app-aarch64.tar.gz"),
            None
        );
        // URL matches no platform → None.
        assert_eq!(size_from_feed(&feed(Some(10)), "https://x.test/nope"), None);
        // No platforms object → None.
        assert_eq!(
            size_from_feed(&json!({ "version": "1" }), "https://x.test/x"),
            None
        );
    }
}
