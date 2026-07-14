//! Auto-update logic shared between the Tauri app (main.rs) and commands.
//!
//! The background loop in main.rs calls `check_for_update()` periodically.
//! When the user clicks "Update Now" in the prompt, `respond_update_prompt`
//! calls `perform_update()` directly — no redundant network re-check.

use crate::commands::ConfigState;
use accelerator_core::{update_manifest, updater_state};
use semver::Version;
use std::sync::OnceLock;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

/// The pinned updater public key, read ONCE from the bundled `tauri.conf.json` — the exact same key
/// the plugin uses to verify artifact signatures. Reading it from the config (instead of duplicating
/// the string) guarantees Layer A verifies against the key the build actually trusts. Panics at first
/// use only if the config is malformed, which is a build-time invariant, not a runtime input.
fn updater_pubkey() -> &'static str {
    static PUBKEY: OnceLock<String> = OnceLock::new();
    PUBKEY.get_or_init(|| {
        const CONF: &str = include_str!("../tauri.conf.json");
        let conf: serde_json::Value =
            serde_json::from_str(CONF).expect("tauri.conf.json is valid JSON");
        conf["plugins"]["updater"]["pubkey"]
            .as_str()
            .expect("tauri.conf.json plugins.updater.pubkey is present")
            .to_string()
    })
}

/// Absolute path to the monotonic version-floor state file. Lives alongside the app's other private
/// state under `~/.aztec-accelerator/` (same base as `certs/`), deliberately NOT inside `config.json`
/// (whose load is fail-open and would silently erase the floor on any parse glitch).
fn updater_state_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".aztec-accelerator").join("updater-state.json"))
}

/// An update that has cleared BOTH F-004 layers: Layer A (the signed-manifest envelope binds the
/// advertised version to the exact signed artifact set — [`update_manifest::verify_manifest`]) and
/// Layer B (the candidate is strictly above `max(current, floor)` — [`updater_state`]). Its fields are
/// private and the ONLY constructor is [`verify_and_gate`], so a value of this type is a
/// proof-carrying token: [`perform_update`] accepts nothing else, and the frontend holds no updater
/// capability (see `capabilities/default.json`). Together those make it impossible to install an
/// artifact that has not cleared both layers.
pub struct VerifiedUpdate {
    update: tauri_plugin_updater::Update,
    /// The SemVer-parsed, envelope-bound version.
    version: Version,
    /// The signed artifact byte size (authoritative — from the signed envelope, so the size cap in
    /// [`perform_update`] cannot be defeated by a lying feed).
    signed_size: u64,
}

impl VerifiedUpdate {
    /// The verified SemVer version — for logging and the post-launch floor commit.
    pub fn version(&self) -> &Version {
        &self.version
    }
}

/// F-004 gate: verify the signed manifest (Layer A) and enforce the monotonic version floor
/// (Layer B). Returns a proof-carrying [`VerifiedUpdate`] iff BOTH pass; on any failure it logs a
/// `SECURITY:`-prefixed reason and returns `None` (fail closed — the app stays on its current build).
fn verify_and_gate(update: tauri_plugin_updater::Update) -> Option<VerifiedUpdate> {
    let current = match Version::parse(env!("CARGO_PKG_VERSION")) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(
                "SECURITY: own version {} is not valid SemVer ({e}); refusing update",
                env!("CARGO_PKG_VERSION")
            );
            return None;
        }
    };

    // Layer A — bind the advertised version to the signed artifact set. Closes the F-004 splice: a
    // feed advertising a high version while pointing url/signature at an old, still-validly-signed
    // artifact is rejected here, BEFORE any download.
    let verified = match update_manifest::verify_manifest(
        &update.raw_json,
        updater_pubkey(),
        &update.version,
        update.download_url.as_str(),
        &update.signature,
    ) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("SECURITY: update-manifest verification failed ({e}); refusing update");
            return None;
        }
    };

    // Layer B — the monotonic anti-rollback floor.
    let floor = match updater_state_path() {
        Some(p) => updater_state::load_floor(&p),
        None => {
            tracing::error!("SECURITY: cannot resolve the updater-state path; refusing update");
            return None;
        }
    };
    if updater_state::running_below_floor(&current, &floor) {
        tracing::error!(
            current = %current,
            "SECURITY: running build is BELOW the version floor (possible out-of-band rollback); refusing all updates"
        );
        return None;
    }
    if !updater_state::candidate_allowed(&verified.version, &current, &floor) {
        tracing::error!(
            candidate = %verified.version,
            current = %current,
            "SECURITY: update candidate is not strictly above max(current, floor); refusing (anti-rollback)"
        );
        return None;
    }

    Some(VerifiedUpdate {
        update,
        version: verified.version,
        signed_size: verified.size,
    })
}

/// Check for updates and act based on the user's auto_update preference. Any available update is put
/// through the F-004 [`verify_and_gate`] FIRST — an unverified or rolled-back candidate never reaches
/// the prompt or the auto-install path. Returns the [`VerifiedUpdate`] when one is available and the
/// user hasn't opted into auto-update (so the caller can show a prompt or store it for later use).
pub async fn check_for_update(
    app: &AppHandle,
    config_state: &ConfigState,
) -> Option<VerifiedUpdate> {
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

    tracing::info!(
        current = env!("CARGO_PKG_VERSION"),
        new = %update.version,
        "Update advertised (pre-verification)"
    );

    // F-004: verify the signed manifest + enforce the version floor BEFORE acting on the update.
    let verified = verify_and_gate(update)?;

    let auto_update_pref = { config_state.read().auto_update };
    tracing::info!(?auto_update_pref, "Auto-update preference");

    match auto_update_pref {
        Some(true) => {
            tracing::info!("Auto-update enabled, performing update");
            perform_update(app, verified).await;
            None
        }
        _ => {
            // None (never asked) or Some(false) (manual) — return the verified update
            // so the caller can show a prompt or add a tray menu item.
            Some(verified)
        }
    }
}

/// Hard ceiling on the auto-update artifact size (SEC-03). Real DMG/AppImage/NSIS artifacts are tens
/// of MB; 500 MB is generous headroom that still stops a multi-GB memory blow-up.
const MAX_UPDATE_BYTES: u64 = 500 * 1024 * 1024;

/// Download, verify Ed25519 signature, install, and restart the app. Accepts ONLY a
/// [`VerifiedUpdate`] — an artifact that has already cleared both F-004 layers.
pub async fn perform_update(app: &AppHandle, verified: VerifiedUpdate) {
    let VerifiedUpdate {
        update,
        version,
        signed_size,
    } = verified;
    tracing::info!(version = %version, signed_size, "Downloading verified update");

    // SEC-03: pre-flight size cap. The plugin buffers the WHOLE artifact into memory before it
    // verifies the artifact signature, and its progress callback cannot abort that loop — so a huge
    // blob is a memory-DoS. Reject up front when the size exceeds the ceiling, BEFORE `download()`.
    // Unlike the old feed-derived value, `signed_size` comes from the F-004 signed envelope (Layer A
    // checked outer==envelope), so a feed can no longer OMIT it or LIE about it to slip the cap — the
    // two ways the previous best-effort cap could be bypassed without the signing key are both closed.
    //
    // Residual (tracked #345): a *malicious* feed that declares a small (correctly signed) size but
    // serves a genuinely larger blob at that url still forces the plugin to buffer those bytes before
    // its artifact-signature check rejects them — an availability-only memory-DoS that needs an
    // upstream streaming abort cap the plugin does not expose (`download()` buffers into an unbounded
    // Vec with a non-aborting callback). Integrity is unaffected: minisign still rejects the tampered
    // bytes. The self-managed reqwest+minisign rewrite that could bound bytes-read was rejected in
    // audit R3 (it would make a hand-rolled verify the sole authenticity control). Hence deferred.
    if signed_size > MAX_UPDATE_BYTES {
        tracing::error!(
            size = signed_size,
            max = MAX_UPDATE_BYTES,
            "Update artifact exceeds the size cap; refusing to download"
        );
        return;
    }
    tracing::info!(size = signed_size, "Signed artifact size within cap");

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

    // Defense in depth: the downloaded byte count must equal the SIGNED size. The plugin's own
    // minisign check already rejects tampered bytes; this additionally rejects a length mismatch
    // before install. Crash-recovery is still armed here (disarm happens below), so a plain return
    // is safe.
    if bytes.len() as u64 != signed_size {
        tracing::error!(
            got = bytes.len(),
            expected = signed_size,
            "SECURITY: downloaded artifact size does not match the signed size; refusing to install"
        );
        return;
    }

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

    // q7e3-F-10: recovery is now disarmed (Windows) — the guard re-arms on EVERY exit path below. Drop
    // covers the install-failure return; the restart arm calls rearm_now() explicitly FIRST, because
    // app.restart() never returns (Drop would never fire there). The old per-arm `// must rearm`
    // comments are now structurally enforced by the guard.
    #[cfg(target_os = "windows")]
    let mut recovery_guard = CrashRecoveryGuard::new(|| rearm_crash_recovery_if_enabled(app));

    match update.install(bytes) {
        Ok(()) => {
            // IgnoreNew + the exit-0-if-healthy guard absorb any brief double-launch with the
            // restarted build.
            #[cfg(target_os = "windows")]
            recovery_guard.rearm_now();
            tracing::info!("Update installed, restarting");
            app.restart();
        }
        Err(e) => {
            tracing::error!("Update install failed: {e}");
            // The app keeps running; recovery_guard's Drop re-arms on return (Windows, only if armed).
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

/// q7e3-F-10: structural guard for the Windows crash-recovery disarm→rearm invariant — *every* path
/// that leaves the app running (or restarts it) must end with recovery re-armed. Previously enforced by
/// a `// must rearm` comment at each of three exit sites. `Drop` re-arms automatically on the
/// early-return paths (install failure, etc.); the restart path MUST call [`rearm_now`] explicitly
/// FIRST, because `app.restart()` never returns — so `Drop` would never fire and recovery would be left
/// off (autostart on, task disarmed). `rearm_now` is idempotent with `Drop` (a flag prevents a
/// double-rearm). Generic over the rearm action so the ordering invariant is unit-testable without a
/// Tauri `AppHandle`. Compiled on Windows (its only real use) and under `test` (so the invariant is
/// pinned on every platform's CI); never in the non-test build of other platforms.
///
/// [`rearm_now`]: CrashRecoveryGuard::rearm_now
#[cfg(any(target_os = "windows", test))]
struct CrashRecoveryGuard<F: FnMut()> {
    rearm: F,
    rearmed: bool,
}

#[cfg(any(target_os = "windows", test))]
impl<F: FnMut()> CrashRecoveryGuard<F> {
    fn new(rearm: F) -> Self {
        Self {
            rearm,
            rearmed: false,
        }
    }

    /// Re-arm now (idempotent). Call this BEFORE a no-return `app.restart()`.
    fn rearm_now(&mut self) {
        if !self.rearmed {
            (self.rearm)();
            self.rearmed = true;
        }
    }
}

#[cfg(any(target_os = "windows", test))]
impl<F: FnMut()> Drop for CrashRecoveryGuard<F> {
    /// Re-arms on scope exit unless [`rearm_now`](CrashRecoveryGuard::rearm_now) already did — covers
    /// every early-return path without a per-site comment.
    fn drop(&mut self) {
        self.rearm_now();
    }
}

#[cfg(test)]
mod tests {
    use super::{updater_pubkey, CrashRecoveryGuard};

    #[test]
    fn updater_pubkey_matches_config() {
        // The pinned key Layer A verifies against MUST be exactly the plugin's configured pubkey.
        // Read tauri.conf.json independently and assert equality (catches a future edit that changes
        // one but not the other).
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let expected = conf["plugins"]["updater"]["pubkey"].as_str().unwrap();
        assert_eq!(updater_pubkey(), expected);
        assert!(!updater_pubkey().is_empty());
    }

    // q7e3-F-10 characterization (test-FIRST): the crash-recovery guard's rearm-before-restart +
    // no-double-rearm invariant. `app.restart()` never returns, so the restart path must `rearm_now()`
    // explicitly and Drop must NOT then re-arm again; the install-failure path relies on Drop alone.
    #[test]
    fn crash_recovery_guard_rearms_on_drop() {
        let count = std::cell::Cell::new(0);
        {
            let _g = CrashRecoveryGuard::new(|| count.set(count.get() + 1));
        }
        assert_eq!(
            count.get(),
            1,
            "Drop must re-arm once on the early-return path"
        );
    }

    #[test]
    fn crash_recovery_guard_rearm_now_before_restart_does_not_double() {
        let count = std::cell::Cell::new(0);
        {
            let mut g = CrashRecoveryGuard::new(|| count.set(count.get() + 1));
            g.rearm_now();
            assert_eq!(
                count.get(),
                1,
                "rearm_now re-arms immediately, before the no-return app.restart()"
            );
        }
        assert_eq!(
            count.get(),
            1,
            "Drop must NOT re-arm again after rearm_now (no double-rearm)"
        );
    }

    #[test]
    fn crash_recovery_guard_rearm_now_is_idempotent() {
        let count = std::cell::Cell::new(0);
        let mut g = CrashRecoveryGuard::new(|| count.set(count.get() + 1));
        g.rearm_now();
        g.rearm_now();
        assert_eq!(count.get(), 1, "rearm_now is idempotent");
    }
}
