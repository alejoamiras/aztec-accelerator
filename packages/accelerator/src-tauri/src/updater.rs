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

/// F-004 B2: acquire the cross-process "updater transaction" lock. Serialises check→install and the
/// post-launch floor commit across concurrent app instances, so two processes can neither race the
/// floor file nor install over each other. Best-effort and non-blocking: if another instance holds it,
/// return `None` and the caller bows out (the periodic poller / next launch retries) rather than
/// blocking the async runtime. The returned guard (the open, exclusively-locked file) releases the
/// lock on drop — and, on the no-return `app.restart()` path, the OS releases it at process exit.
/// `pub(crate)`: the autostart heal takes this NON-BLOCKING to bow out while an update transaction
/// is live (plan Fork B / D19 — the heal must never hold or wait on it; `autostart.lock` is the
/// mutation lock, this is only the "is an update running?" probe).
pub(crate) fn acquire_updater_lock() -> Option<std::fs::File> {
    use fs2::FileExt as _;
    let parent = updater_state_path()?.parent()?.to_path_buf();
    let _ = std::fs::create_dir_all(&parent);
    let lock_path = parent.join("updater.lock");
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(e) => {
            // F-04 variant C: this is NOT "another instance holds it" (that is the try_lock branch
            // below, which logs). The lock FILE is unopenable — permissions stripped, or replaced by a
            // directory — which silently disabled installs, floor commits AND the autostart heal with
            // no signal whatsoever. Say so loudly instead.
            tracing::error!(
                error = %e,
                path = %lock_path.display(),
                "SECURITY: cannot open the updater lock; auto-update and the autostart self-heal are \
                 disabled until this file is repaired or removed"
            );
            return None;
        }
    };
    match file.try_lock_exclusive() {
        Ok(()) => Some(file),
        Err(_) => {
            tracing::info!(
                "Another instance holds the updater lock; skipping this update transaction"
            );
            None
        }
    }
}

/// Record that THIS build launched successfully by advancing the monotonic version floor to the
/// running version (F-004 Layer B). Called once, after the app has proven it actually runs (see the
/// launch tracker in main.rs) — so a build that boots but immediately wedges never ratchets the floor
/// and can't lock itself in as the new minimum.
///
/// The updater lock is REQUIRED, not best-effort (audit H2): committing the floor without it can race a
/// concurrent installer — the installer re-checks the floor before `install()`, so a commit that lands
/// between that check and the install would let the installer write a version below the just-advanced
/// floor. If the lock is held by another instance's transaction, defer the commit (the next launch
/// retries) rather than commit unlocked.
pub fn commit_launch_floor() {
    let Some(path) = updater_state_path() else {
        tracing::warn!("cannot resolve updater-state path; skipping floor commit");
        return;
    };
    let current = match Version::parse(env!("CARGO_PKG_VERSION")) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("own version is not SemVer ({e}); skipping floor commit");
            return;
        }
    };
    let Some(_guard) = acquire_updater_lock() else {
        tracing::warn!(
            "could not acquire the updater lock; deferring the floor commit to avoid racing a concurrent install"
        );
        return;
    };
    match updater_state::commit_successful_launch(&path, &current) {
        Ok(()) => tracing::info!(version = %current, "Version floor committed for this launch"),
        Err(e) => tracing::warn!("Floor commit skipped: {e}"),
    }
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

/// F-004 Layer B, fail-closed. Returns `Ok(())` iff `candidate` may be installed given the persisted
/// state and the running version. Every arm — updater-state path resolution, state load, the
/// running-below-floor check, and candidate-allowed (which itself rejects a `Corrupt` state) — fails
/// CLOSED (Err) on any problem. Shared by the check-time gate ([`verify_and_gate`]) and the install-time
/// re-check ([`perform_update`]) so the two can never diverge (audit M5). `current` is passed in so the
/// caller parses it once and a parse failure is handled as fail-closed there.
fn layer_b_gate(candidate: &Version, current: &Version) -> Result<(), String> {
    let Some(path) = updater_state_path() else {
        return Err("cannot resolve the updater-state path".to_string());
    };
    let state = updater_state::load_state(&path, updater_state::now_unix());
    if updater_state::running_below_floor(current, &state) {
        // F-04: a DIAGNOSTIC, not a veto. Refusing every update here was the permanent, silent
        // lockout: reachable by a forged floor *or* by a user deliberately downgrading to an older
        // build, and unrecoverable because nothing in the tree repaired the file. The floor is still
        // enforced literally by `candidate_allowed` — blocking a candidate below it is the feature —
        // but `commit_successful_launch` now RESETS it to the running build, so the state repairs
        // itself within one launch. The candidate must still beat the running build regardless, which
        // is the anti-rollback rule that does not depend on this file at all.
        tracing::error!(
            running = %current,
            "SECURITY: the persisted version floor is ABOVE the running build (out-of-band downgrade, \
             user downgrade, or a forged state file); it will be reset to the running version by the \
             next successful-launch commit rather than disabling updates permanently"
        );
    }
    if !updater_state::candidate_allowed(candidate, current, &state) {
        return Err(format!(
            "candidate {candidate} is not strictly above max(current {current}, floor, pending)"
        ));
    }
    Ok(())
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

    // Layer B — the monotonic anti-rollback floor (shared fail-closed gate).
    if let Err(reason) = layer_b_gate(&verified.version, &current) {
        tracing::error!("SECURITY: {reason}; refusing update");
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

    // Residual (audit M6): `updater.check()` fetches, BUFFERS, and JSON-parses the whole feed body
    // BEFORE we ever see `raw_json` — so `verify_manifest`'s 64 KiB manifest-field cap does NOT bound
    // the feed response itself. A feed writer returning a multi-GB `notes`/`platforms` blob is an
    // availability-only memory-DoS at check() time. Closing it needs an upstream feed-response byte
    // limit before JSON parsing, which `tauri-plugin-updater` does not expose (same class as the
    // artifact-buffer residual #345). Integrity is unaffected: an oversized feed still cannot forge a
    // valid signed manifest. Documented here so it isn't mistaken for covered by the manifest cap.
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

    // B2: hold the cross-process updater lock across the whole download+install so no other instance
    // can race the floor or install concurrently. If another instance is mid-update, bow out (the
    // poller retries). Held until this fn returns / the process restarts.
    let _txn = match acquire_updater_lock() {
        Some(f) => f,
        None => return,
    };

    // D22 (piece-2): no new update while an update window is live. The marker outlives the
    // updater lock by design (the lock dies with the exiting process at install()), so this is a
    // distinct check, not a duplicate of the lock.
    #[cfg(target_os = "windows")]
    if let Some(paths) = crate::update_marker::MarkerPaths::default_paths() {
        if crate::update_marker::live_marker_exists(&paths, crate::update_marker::now_unix()) {
            tracing::warn!("an update window is still live; not starting another update");
            return;
        }
    }

    // Parse our own version once; a parse failure is fail-closed (can't safely gate → abort). Needed
    // both for the install-time re-check and for recording the pending version after install.
    let current = match Version::parse(env!("CARGO_PKG_VERSION")) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("SECURITY: own version is not SemVer ({e}); aborting install");
            return;
        }
    };

    // TOCTOU: the floor/pending may have advanced since check_for_update (another instance committed a
    // launch or recorded a pending install). Re-run the SAME fail-closed Layer B gate under the lock,
    // right before committing to the download — a Corrupt/raced state now aborts the install.
    if let Err(reason) = layer_b_gate(&version, &current) {
        tracing::error!(
            candidate = %version,
            "SECURITY: {reason} at install time (raced by another instance); aborting"
        );
        return;
    }

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

    // Arc-hunt r3 #2 + r4 #2: resolve the installer's destination BEFORE anything is mutated —
    // above `record_pending` as well as above the disarm. It is a read-only registry+env lookup,
    // so an unreadable/wrong-typed install-location value must not (a) leave crash recovery
    // disarmed with no marker to restore it, nor (b) leave `pending` recorded: F-004's floor then
    // rejects every version below that candidate, so a WITHDRAWN release would block the fixed
    // (lower but still newer) one forever.
    #[cfg(target_os = "windows")]
    let expected_install = match nsis_install_destination() {
        Ok(dest) => canonicalize_expected(dest),
        Err(e) => {
            tracing::error!(
                "cannot determine the install destination ({e}); aborting before any state change"
            );
            return;
        }
    };

    // Piece-2 rev-3 ordering: record the install INTENT first (it needs only the updater lock and
    // must precede install(); a later disarm failure leaving `pending` recorded is the documented
    // exact-version-retry semantics) — so the fallible floor write can never strand a live marker.
    // H1 / codex #5 rationale unchanged: fail closed if the intent cannot be recorded.
    match updater_state_path() {
        Some(path) => {
            if let Err(e) = updater_state::record_pending(&path, &current, &version) {
                tracing::error!(
                    candidate = %version,
                    "SECURITY: failed to record the install intent before install ({e}); aborting to avoid a downgrade window"
                );
                return;
            }
        }
        None => {
            tracing::error!(
                "SECURITY: cannot resolve the updater-state path; aborting install (no rollback floor)"
            );
            return;
        }
    }

    // B3 (F6): CONFIRM the in-flight bb tree is dead BEFORE installing. On Windows install() spawns NSIS
    // then process::exit's, so bb must be gone before the installer touches files; kill/TerminateJobObject
    // are asynchronous, so we WAIT and ABORT the update if bb can't be confirmed dead (codex H2 / the
    // ratified "unconfirmed reap ⇒ abort update"). Placed after the download and before the marker
    // transaction below, so an abort is a clean `return` (only the updater lock is held — no marker or
    // recovery state to unwind).
    //
    // codex r2 H2a: `begin_quiesce()` FIRST, and hold it across confirm + install. It makes every new
    // `prove` fail to register, so — set under the same lock the terminator takes the pgid under (Unix) /
    // re-checked after job assignment (Windows) — no bb can start in the window between "confirmed dead" and
    // "installed". `_quiesce` drops on every abort `return` below, re-opening proving; a SUCCESSFUL install
    // exits/restarts the process, so it never needs to.
    let _quiesce = crate::bb::begin_quiesce();
    if let Err(e) = crate::bb::terminate_and_confirm(std::time::Duration::from_secs(5)).await {
        tracing::error!(error = %e, "Aborting update install: could not confirm the in-flight bb was terminated");
        return;
    }

    // ── Windows: the update-window critical section (piece-2 plan §4) ──
    // autostart.lock spans intent-read → disarm → marker+handoff create, and NOTHING else: the
    // held lock freezes owned intent mutations, which is what keeps the snapshot honest inside it
    // (final pass #1 — a pre-lock snapshot could go stale against a mid-download toggle).
    // Lock nesting matches the heal: updater.lock (outer, held) → autostart.lock (inner).
    #[cfg(target_os = "windows")]
    let (marker_paths, mut recovery_guard) = {
        let Some(paths) = crate::update_marker::MarkerPaths::default_paths() else {
            tracing::error!("cannot resolve the update-marker path; aborting install");
            return;
        };
        let section_lock = match crate::autostart::acquire_autostart_lock() {
            Ok(l) => l,
            Err(e) => {
                // Nothing mutated yet — plain abort; the poller retries.
                tracing::warn!("cannot enter the update critical section ({e}); aborting install");
                return;
            }
        };

        // Current intent, read INSIDE the lock — both the marker's stored snapshot and the
        // guard's rearm input. Err ⇒ assume enabled (codex r3 #5: about to disarm, err toward
        // restoring; a spurious rearm is an idempotent write).
        let was_recovery_enabled = crate::autostart::intent_enabled(app).unwrap_or_else(|e| {
            tracing::warn!(
                "pre-install: autostart state unreadable ({e}); assuming enabled for re-arm safety"
            );
            true
        });

        // Disarm the always-armed repeating crash-recovery task right before install. A tick
        // during NSIS file mutation could spawn a half-written binary. Cannot confirm ⇒ do NOT
        // install.
        if !crate::crash_recovery::disable_crash_recovery() {
            tracing::error!(
                "Aborting update install: could not disarm crash-recovery task (race risk)"
            );
            rearm_crash_recovery_if_enabled(was_recovery_enabled);
            drop(section_lock);
            return;
        }

        // q7e3-F-10: the guard re-arms on every exit path below unless explicitly defused after
        // the marker cleanup has reconciled recovery itself (defusing prevents a stale-snapshot
        // Drop-rearm into a live window).
        let mut guard =
            CrashRecoveryGuard::new(move || rearm_crash_recovery_if_enabled(was_recovery_enabled));

        // The marker (D18): compare-and-create under the held lock; a live foreign window is
        // never deleted. The expected install path is where the INSTALLER will put the new exe —
        // NOT current_exe (arc-hunt r2 F2: a stray copy driving an update would record its own
        // path, and the relaunched N would suppress reconciliation until the deadline). For the
        // installed instance the two coincide.
        let expected = expected_install.clone();
        let payload = crate::update_marker::MarkerPayload::new(
            &version,
            &expected,
            was_recovery_enabled,
            crate::update_marker::now_unix(),
        );
        match crate::update_marker::create_new(&paths, &payload, crate::update_marker::now_unix()) {
            Ok(()) => {
                // Observable proof the transaction was armed (D24: version/nonce only, no paths).
                // The Windows smoke asserts this line so "no transaction files afterwards" cannot
                // pass by never having created one (arc-hunt r3 #4).
                tracing::info!(candidate = %version, "update window marker armed");
            }
            Err(crate::update_marker::CreateErr::Live) => {
                // Raced by a FOREIGN window between the top-of-fn check and here. Never delete
                // it — and never REARM into it either: "no process rearms while a marker is
                // live" applies to us too. Recovery stays disarmed; the foreign window's own
                // reconcile (next launch) reconciles to intent (post-impl audit #1).
                tracing::warn!(
                    "another update window appeared; aborting install, recovery stays disarmed"
                );
                guard.defuse();
                return;
            }
            Err(crate::update_marker::CreateErr::NotPublished(e)) => {
                // Nothing of ours exists — plain abort; the guard's Drop rearms from the
                // snapshot, which is FRESH here (the lock has been held since the intent read),
                // so an unreadable current intent can never strand recovery on this path
                // (verification pass, sibling path a).
                tracing::error!("cannot create the update marker ({e}); aborting install");
                return;
            }
            Err(crate::update_marker::CreateErr::PublishedMaybe(e)) => {
                // The exclusive create WON, so any survivor is OURS — possibly complete (sync
                // can fail after a full write). Run the checked, intent-sensitive cleanup under
                // the still-held lock, then defuse: a Drop-rearm into a possibly-live marker of
                // ours is exactly what rev 3 forbids (post-impl audit #1).
                tracing::error!("marker write failed after creation ({e}); aborting install");
                crate::update_marker::post_create_failure_cleanup(
                    &paths,
                    &crate::autostart::intent_enabled_now,
                    // r5 #1: the GATED arm — cleanup re-arms recovery, and a copy-initiated
                    // update whose marker write failed must not capture the task on the way out.
                    &crate::autostart::gated_enable_crash_recovery,
                    &crate::crash_recovery::disable_crash_recovery,
                );
                guard.defuse();
                return;
            }
        }
        if let Err(e) = crate::update_marker::write_handoff(&paths, &payload.txn) {
            // Post-create failure (final pass #2): intent-sensitive cleanup under the still-held
            // lock, then defuse — the cleanup reconciled recovery itself.
            tracing::error!("cannot write the update handoff ({e}); aborting install");
            crate::update_marker::post_create_failure_cleanup(
                &paths,
                &crate::autostart::intent_enabled_now,
                // r5 #1: gated — see the sibling cleanup above.
                &crate::autostart::gated_enable_crash_recovery,
                &crate::crash_recovery::disable_crash_recovery,
            );
            guard.defuse();
            drop(section_lock);
            return;
        }
        drop(section_lock); // dropped BEFORE install(): NSIS runs outside any lock we hold.
        (paths, guard)
    };

    match update.install(bytes) {
        Ok(()) => {
            // Windows never reaches here — install() dispatched the installer and exited the process.
            // macOS/Linux: the intent is already recorded above; the restarted build commits its floor
            // and clears the intent on a healthy launch. IgnoreNew + the exit-0-if-healthy guard absorb
            // any brief double-launch with the restarted build.
            #[cfg(target_os = "windows")]
            recovery_guard.rearm_now();
            tracing::info!("Update installed, restarting");
            app.restart();
        }
        Err(e) => {
            // Intent stays recorded — a returned Err is not proof that nothing was mutated (codex #5),
            // and candidate_allowed lets this exact version be retried.
            tracing::error!("Update install failed: {e}");
            // Piece-2: the writer (this process) can never satisfy its own marker's removal rule,
            // so clean up NOW, under a re-acquired lock, with intent-sensitive ordering (plan §4:
            // ON = remove-checked-then-arm; OFF = confirm-disarm-then-remove), then defuse the
            // guard — its snapshot is stale and cleanup reconciled recovery itself. If the lock
            // cannot be re-acquired, leave everything: the marker suppresses until the next
            // launch's reconcile (or expiry), and the DEFUSED guard cannot rearm into that window.
            #[cfg(target_os = "windows")]
            {
                match crate::autostart::acquire_autostart_lock() {
                    Ok(_lock) => {
                        crate::update_marker::post_create_failure_cleanup(
                            &marker_paths,
                            &crate::autostart::intent_enabled_now,
                            // r5 #1: gated — see the sibling cleanups above.
                            &crate::autostart::gated_enable_crash_recovery,
                            &crate::crash_recovery::disable_crash_recovery,
                        );
                    }
                    Err(le) => {
                        tracing::warn!(
                            "cannot re-enter the update critical section after a failed install ({le}); leaving the window suppressed"
                        );
                    }
                }
                recovery_guard.defuse();
            }
        }
    }
}

/// Re-arm the Windows crash-recovery task iff it was armed before this update disarmed it. The
/// decision uses `was_enabled` — the autostart state captured ONCE before the disarm (codex r3 #5) —
/// NOT a fresh read, so a transient read error can neither silently skip a needed re-arm (r2 #5) nor
/// spuriously arm recovery while autostart is off (r3 #5). Idempotent: `enable_crash_recovery`
/// overwrites any existing task.
#[cfg(target_os = "windows")]
fn rearm_crash_recovery_if_enabled(was_enabled: bool) {
    if was_enabled {
        // Arc-hunt r2 F1: the guard rearm is an IMPLICIT path — a stray copy whose update
        // aborted must not re-point the recovery task at itself on the way out. (Windows: no
        // AppImage indirection, so current_exe is the ownership reference.)
        match std::env::current_exe() {
            Ok(exe) if crate::autostart::implicit_arm_gate(&exe) => {}
            Ok(_) => return,
            Err(e) => {
                tracing::warn!("own path unresolvable ({e}); post-update rearm skipped");
                return;
            }
        }
        // C8 (D12): log-and-continue — a post-update rearm hiccup must not abort, but is never swallowed.
        if let Err(e) = crate::crash_recovery::enable_crash_recovery() {
            tracing::warn!("post-update crash-recovery rearm failed: {e}");
        }
    }
}

/// Arc-hunt r2 F2: the destination the NSIS installer will actually use — MANUPRODUCTKEY's
/// default value (`Software\<publisher>\<productName>`, written by every prior install and read
/// back by RestorePreviousInstallLocation) or the `%LOCALAPPDATA%` default. `Err` means the
/// registry answered something other than "not found": abort the update rather than publish a
/// marker with a guessed path. Pre-#427 installs wrote the OLD manufacturer namespace
/// ("Software\aztec\…"); those are dev-only default-dir installs, and NotFound → default
/// resolves to exactly where they live.
#[cfg(target_os = "windows")]
fn nsis_install_destination() -> Result<std::path::PathBuf, String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
    let reg_dir = match hkcu
        .open_subkey_with_flags(r"Software\Aztec Accelerator\Aztec Accelerator", KEY_READ)
    {
        Ok(k) => match k.get_value::<String, _>("") {
            Ok(v) => Some(v),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("install-location value read failed: {e}")),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("install-location key open failed: {e}")),
    };
    let lad = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    crate::update_marker::expected_install_path_from(reg_dir, lad)
        .ok_or_else(|| "cannot determine install destination (no LOCALAPPDATA)".to_string())
}

/// Best-effort canonicalization that survives a not-yet-existing file: canonicalize the whole
/// path, else the parent dir + file name, else keep it raw (the marker reader's proven-absence
/// rule handles the rest).
#[cfg(target_os = "windows")]
fn canonicalize_expected(p: std::path::PathBuf) -> std::path::PathBuf {
    if let Ok(c) = p.canonicalize() {
        return c;
    }
    if let (Some(dir), Some(name)) = (p.parent(), p.file_name()) {
        if let Ok(cd) = dir.canonicalize() {
            return cd.join(name);
        }
    }
    p
}

/// Delete a resolved legacy exe with a bounded retry. The old process may still hold a handle for a beat
/// after the update relaunches us, so a single `remove_file` can lose the race (exactly the flaw in
/// Tauri's installer delete this backstops). Retry ~5s (25 × 200ms, matching the predecessor/server
/// tolerances elsewhere); `NotFound` is success; on exhaustion log and keep running — the next startup
/// retries, and we NEVER rewrite autostart here. Compiled under `test` too so the effect is unit-testable
/// off-Windows.
#[cfg(any(target_os = "windows", test))]
fn prune_legacy_file(legacy: &std::path::Path) {
    match std::fs::symlink_metadata(legacy) {
        Ok(md) if md.is_dir() => return, // only ever a file/symlink — never a directory
        Ok(_) => {}
        Err(_) => return, // absent ⇒ nothing to do
    }
    for attempt in 1..=25u32 {
        match std::fs::remove_file(legacy) {
            Ok(()) => {
                tracing::info!("pruned surviving legacy exe after rename-update");
                return;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) if attempt == 25 => {
                tracing::error!(
                    "could not prune legacy exe after 25 attempts (retry next startup): {e}"
                );
                return;
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(200)),
        }
    }
}

/// Backstop for Tauri's racy NSIS `Delete $OldMainBinaryName` on a rename-crossing update
/// (1.x `aztec-accelerator.exe` → 2.0 `AztecAccelerator.exe`). Runs on every startup of the
/// OFFICIALLY-INSTALLED app, AFTER the update-marker reconcile and BEFORE the autostart heal — a deleted
/// legacy target makes any stale autostart pointer `Broken`, which the existing heal then repairs. No-op
/// unless our canonical path is the canonical NSIS destination (see `legacy_cleanup_candidate`), so a
/// portable/copied binary can never delete a user's unrelated `aztec-accelerator.exe`.
#[cfg(target_os = "windows")]
pub fn cleanup_legacy_binary_after_rename() {
    let Ok(current) = std::env::current_exe() else {
        return;
    };
    let dest = match nsis_install_destination() {
        Ok(d) => d,
        Err(e) => {
            tracing::debug!("legacy-exe prune skipped (no install destination): {e}");
            return;
        }
    };
    let current_c = canonicalize_expected(current);
    let dest_c = canonicalize_expected(dest);
    if let Some(legacy) = crate::update_marker::legacy_cleanup_candidate(&current_c, &dest_c) {
        prune_legacy_file(&legacy);
    }
}

#[cfg(test)]
mod legacy_prune_tests {
    use super::prune_legacy_file;

    #[test]
    fn prune_removes_only_the_target_is_idempotent_and_spares_directories() {
        let dir = std::env::temp_dir().join(format!("aa-legacy-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let legacy = dir.join("aztec-accelerator.exe");
        let keep = dir.join("AztecAccelerator.exe");
        std::fs::write(&legacy, b"old").unwrap();
        std::fs::write(&keep, b"new").unwrap();

        // Deletes the exact target, leaves the similarly-named current exe untouched.
        prune_legacy_file(&legacy);
        assert!(!legacy.exists(), "legacy exe should be pruned");
        assert!(keep.exists(), "the current exe must never be pruned");

        // Idempotent when already absent.
        prune_legacy_file(&legacy);
        assert!(!legacy.exists());

        // Never removes a directory sitting at the path.
        let as_dir = dir.join("some.dir");
        std::fs::create_dir_all(&as_dir).unwrap();
        prune_legacy_file(&as_dir);
        assert!(
            as_dir.is_dir(),
            "a directory at the path must be left alone"
        );

        let _ = std::fs::remove_dir_all(&dir);
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

    /// DEFUSE: mark handled WITHOUT rearming. Used after the marker cleanup transaction has
    /// already reconciled recovery to CURRENT intent (piece-2 plan §4) — a Drop-rearm after that
    /// would act on the STALE pre-disarm snapshot, and a rearm into a still-live window is exactly
    /// what the marker forbids.
    fn defuse(&mut self) {
        self.rearmed = true;
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
    fn crash_recovery_guard_defuse_prevents_drop_rearm() {
        // Piece 2: after the marker cleanup transaction has reconciled recovery to CURRENT
        // intent, the guard's snapshot is stale — defuse() must make Drop a no-op, or a stale
        // rearm could fire into a live update window.
        let count = std::cell::Cell::new(0);
        {
            let mut guard = CrashRecoveryGuard::new(|| count.set(count.get() + 1));
            guard.defuse();
        }
        assert_eq!(count.get(), 0, "a defused guard must never rearm");
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
