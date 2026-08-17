use crate::authorization::{AuthDecision, AuthorizationManager, ResolveOutcome};
use crate::config::{self, AcceleratorConfig};
use crate::verified_sites::VerifiedSitesRegistry;
use std::sync::Arc;
use tauri::Manager;

/// B4: core's shared [`config::ConfigStore`] (config lock + persist capability), also held by
/// `HeadlessState.config`. `Deref`s to the lock so existing reads are unchanged; [`mutate_config`] uses
/// `.cap` to gate the save.
pub use crate::config::ConfigStore;
pub type ConfigState = Arc<config::ConfigStore>;

/// B4 (codex): the single user-facing message for "the on-disk config is from a newer build, so this build
/// may not overwrite it." Shared by [`mutate_config`]'s refuse arm and the [`require_persistable`] preflight
/// so the Settings UI shows one consistent string whether a command bails early or fails at the save.
const NEWER_CONFIG_MSG: &str =
    "This config was written by a newer version of the app; changes can't be saved. Update the app to change settings.";

/// Lock the config, apply `f`, then persist IFF this build may overwrite the on-disk schema (B4) — the
/// single source of truth for the lock-mutate-save pattern. Propagates the save error.
fn mutate_config(
    config: &ConfigState,
    f: impl FnOnce(&mut AcceleratorConfig),
) -> Result<(), String> {
    match config.cap.as_ref() {
        // q7e3-F-13: delegate to core's shared lock_mutate_save; keep the always-save + propagate policy.
        Some(cap) => config::lock_mutate_save(&config.lock, cap, |cfg| {
            f(cfg);
            true
        })
        .map_err(|e| e.to_string()),
        // B4 (codex): a NEWER build wrote this config. REFUSE the change and report it — do NOT apply it
        // in-memory (that would silently diverge from disk, revert next launch, and be undone by the
        // fresh-disk-read on the HTTPS-launch path, so the command would falsely "succeed"). Surface a clear
        // error so the UI shows the failure instead of a false success. `f` is dropped unused.
        None => {
            let _ = f;
            tracing::warn!("Refusing a config change: on-disk config was written by a newer build");
            Err(NEWER_CONFIG_MSG.to_string())
        }
    }
}

/// B4 (codex): preflight persistence eligibility BEFORE a command performs config-dependent EXTERNAL side
/// effects (generating certs, mutating the OS trust store, rotating the cert identity). Without it, a
/// command run against a newer-schema config would install/remove trust and THEN fail the config commit —
/// leaving an orphaned CA in the user's trust store with nothing in config to reflect it.
///
/// Two checks: the STARTUP capability ([`is_persistable`], `false` ⇒ this build never had a writable config —
/// headless or a future schema seen at launch), AND a FRESH on-disk re-probe (`on_disk_is_overwritable`) that
/// catches a newer build appearing SINCE launch (the stale-cap case codex round 3 flagged — the startup cap
/// alone would false-positive). Best-effort early-out; the save path still re-checks under the cross-process
/// lock at write time, which is the actual gate.
fn require_persistable(config: &ConfigState) -> Result<(), String> {
    if config.is_persistable() && config::on_disk_is_overwritable() {
        Ok(())
    } else {
        tracing::warn!(
            "Refusing a trust/HTTPS action: on-disk config was written by a newer build"
        );
        Err(NEWER_CONFIG_MSG.to_string())
    }
}
pub type AuthState = Arc<AuthorizationManager>;
pub type VerifiedSitesState = Arc<VerifiedSitesRegistry>;
/// Shared AppState so HTTPS servers spawned later (e.g. enabling Safari) get the full
/// state including auth_manager, config, and show_auth_popup — not a bare Default.
pub type SharedAppState = Arc<crate::server::AppState>;

/// B2 (F8): the pending-update slot lives in its OWN module so its `inner` storage is unreachable from
/// the rest of `commands` — `take_or_reprompt` is then GENUINELY the only extractor, and it never
/// hands the pending version back as a value (private-field encapsulation is module-scoped in Rust;
/// a sibling function in `commands` could otherwise write `pending.inner.lock().take()` and defeat the
/// consent binding entirely). This is the type-level enforcement the F8 doc claims — not a convention.
mod pending_update {
    /// Anything held pending in a [`PendingUpdateSlot`] must expose its version string, so the slot can
    /// bind consumption to the version the user actually saw.
    pub trait PendingVersion {
        fn version_string(&self) -> String;
    }

    use tauri::Manager;

    /// Outcome of [`PendingUpdateSlot::take_or_reprompt`].
    pub enum TakeOutcome<T> {
        /// The displayed version matched the pending one; the item is CONSUMED and returned.
        Took(T),
        /// A different version is pending; the item was RETAINED and an opaque [`Reprompt`] capability is
        /// returned so the caller can re-point the prompt — WITHOUT ever seeing the pending version.
        Reprompt(Reprompt),
        /// Nothing is pending (expired/cleared).
        Empty,
    }

    /// An opaque, single-use capability to re-point the update prompt at the pending version after a
    /// display↔version mismatch. It holds the pending version PRIVATELY; the only thing `commands` can do
    /// with it is [`navigate`], which builds the URL and drives the webview INSIDE this module. The
    /// version is never handed to caller code as a value, by-reference, or otherwise — so it cannot be
    /// captured and fed back into a take to force installing a version the user did not see (the escape
    /// hatch codex flagged across rounds 2–4).
    ///
    /// [`navigate`]: Reprompt::navigate
    pub struct Reprompt {
        pending_version: String,
    }

    impl Reprompt {
        /// Re-point the open update prompt at the pending version by rewriting only its query string
        /// (preserving the live window's platform-specific scheme + path, so it is never hardcoded).
        /// Reloading re-renders the version AND fires `pageshow`, re-arming the click-steal guard for the
        /// fresh decision. Consumes `self`, so the capability is one-shot. Every failure path CLOSES the
        /// prompt: the command returns `Ok`, so `wireButton` leaves the clicked control disabled — a
        /// silent early-return would strand the user with a permanently-disabled window that future 12h
        /// checks dedup against. A successful navigate reloads the page, re-enabling the controls itself.
        pub fn navigate(
            self,
            app: &tauri::AppHandle,
            current_version: &str,
            displayed_version: &str,
        ) {
            tracing::warn!(
                displayed = %displayed_version,
                pending = %self.pending_version,
                "SECURITY: update-prompt version mismatch — refusing the stale-displayed install; re-prompting for the pending version"
            );
            let Some(window) = app.get_webview_window("update-prompt") else {
                return; // already gone — nothing to strand
            };
            let mut url = match window.url() {
                Ok(url) => url,
                Err(e) => {
                    tracing::warn!(error = %e, "Could not read the update prompt URL to re-point it; closing it");
                    let _ = window.close();
                    return;
                }
            };
            url.set_query(Some(&format!(
                "current={}&version={}",
                urlencoding::encode(current_version),
                urlencoding::encode(&self.pending_version)
            )));
            if let Err(e) = window.navigate(url) {
                tracing::warn!(error = %e, "Failed to re-point the update prompt after a version mismatch; closing it");
                let _ = window.close();
            }
        }

        /// The captured version — TEST-ONLY, so a unit test can assert the right version was captured
        /// without a production accessor that would reopen the re-feed escape hatch.
        #[cfg(test)]
        pub fn version(&self) -> &str {
            &self.pending_version
        }
    }

    /// Holds a pending, already-VERIFIED update so `respond_update_prompt` can install it directly
    /// instead of re-checking the network.
    ///
    /// B2 (F8 — consent binding): the inner slot is PRIVATE TO THIS MODULE and the ONLY extractor is
    /// [`take_or_reprompt`], which consumes the item ONLY IF the caller passes the version string the
    /// prompt is currently displaying. There is no unconditional `take`, and the pending version is
    /// NEVER exposed to `commands` — on a mismatch it is sealed inside an opaque [`Reprompt`] whose only
    /// operation is navigation. So a caller cannot capture the pending version and feed it back in to
    /// force an unbound take: the update prompt physically cannot install a version the user did not
    /// consent to. Storing a `VerifiedUpdate` (not a raw plugin `Update`) additionally means nothing
    /// that failed either F-004 layer can ever install.
    ///
    /// Generic over the payload purely so the consent logic is unit-testable with a dummy
    /// `PendingVersion` (`VerifiedUpdate` itself has a private, network-gated constructor).
    ///
    /// [`take_or_reprompt`]: PendingUpdateSlot::take_or_reprompt
    pub struct PendingUpdateSlot<T> {
        inner: parking_lot::Mutex<Option<T>>,
    }

    impl<T> Default for PendingUpdateSlot<T> {
        fn default() -> Self {
            Self {
                inner: parking_lot::Mutex::new(None),
            }
        }
    }

    impl<T: PendingVersion> PendingUpdateSlot<T> {
        /// Store a freshly-verified item, replacing any prior pending one.
        pub fn set(&self, item: T) {
            *self.inner.lock() = Some(item);
        }

        /// B2 (F8): consume the pending item ONLY IF its version equals `displayed`. On mismatch the item
        /// is left in place and an opaque [`Reprompt`] is returned — the consent-binding choke point. The
        /// pending version is sealed inside `Reprompt` (never returned as a value), so it cannot be
        /// captured and fed back in to force a take.
        pub fn take_or_reprompt(&self, displayed: &str) -> TakeOutcome<T> {
            let mut guard = self.inner.lock();
            match guard.as_ref().map(PendingVersion::version_string) {
                Some(v) if v == displayed => {
                    TakeOutcome::Took(guard.take().expect("slot was Some in the arm above"))
                }
                Some(pending_version) => TakeOutcome::Reprompt(Reprompt { pending_version }),
                None => TakeOutcome::Empty,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Dummy stand-in for `VerifiedUpdate` (whose only constructor is network-gated).
        struct Dummy(&'static str);
        impl PendingVersion for Dummy {
            fn version_string(&self) -> String {
                self.0.to_string()
            }
        }

        #[test]
        fn take_or_reprompt_takes_only_on_match_and_seals_version_on_mismatch() {
            let slot = PendingUpdateSlot::<Dummy>::default();

            // Empty slot → Empty.
            assert!(matches!(slot.take_or_reprompt("2.0.0"), TakeOutcome::Empty));

            slot.set(Dummy("2.0.0"));

            // Mismatch: an opaque Reprompt is returned (version sealed inside, only accessible via the
            // test-only accessor) and the item is NOT consumed. Reverting the `v == displayed` guard so
            // it consumes unconditionally turns this into `Took` and fails the "still present" check.
            match slot.take_or_reprompt("1.0.0") {
                TakeOutcome::Reprompt(cap) => assert_eq!(cap.version(), "2.0.0"),
                _ => panic!("a version mismatch must return a Reprompt, not consume the update"),
            }

            // Still present → an exact-version take now consumes it.
            assert!(matches!(
                slot.take_or_reprompt("2.0.0"),
                TakeOutcome::Took(_)
            ));
            // Consumed exactly once.
            assert!(matches!(slot.take_or_reprompt("2.0.0"), TakeOutcome::Empty));
        }
    }
}

use pending_update::{PendingUpdateSlot, PendingVersion, TakeOutcome};

impl PendingVersion for crate::updater::VerifiedUpdate {
    fn version_string(&self) -> String {
        self.version().to_string()
    }
}

/// The managed pending-update slot: a [`PendingUpdateSlot`] of network-verified updates.
pub type PendingUpdate = PendingUpdateSlot<crate::updater::VerifiedUpdate>;

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
) -> Result<crate::autostart::AutostartStatus, String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    // D3/D13: a structured status, not a bare bool — the switch shows INTENT, a separate row shows
    // health (plan §5). codex #7 preserved: an Unreadable artifact surfaces as Err rather than a
    // false "disabled" (the switch stays disabled on unknown state). Read-only — opening Settings
    // never writes OS state.
    crate::autostart::status(&app)
}

/// D16: the Fix button. A dedicated command, NOT `set_autostart(true)` — a Broken entry reads
/// `prior_enabled == true` (intent), so the enable transaction correctly SKIPS the artifact write
/// (C1: rewriting would strip macOS KeepAlive), and toggle-ON on a broken entry is a silent no-op.
/// Repair is the in-place patch instead. Returns the fresh status so the UI re-renders from truth.
#[tauri::command]
pub fn repair_autostart(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
) -> Result<crate::autostart::AutostartStatus, String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    use crate::autostart::HealOutcome;
    match crate::autostart::heal_if_broken(&app) {
        HealOutcome::Healed { .. } | HealOutcome::NotNeeded => crate::autostart::status(&app),
        HealOutcome::Skipped(reason) => Err(format!("repair skipped: {reason}")),
        HealOutcome::Failed(e) => Err(format!("repair failed: {e}")),
    }
}

#[tauri::command]
pub fn set_autostart(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<(), String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    set_autostart_inner(&app, enabled)
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
) -> Result<(), String> {
    // F-012 (D6) + SEC-06 (strengthened): bind the calling window to THIS request_id. windows.rs labels
    // the popup `auth-{hash(request_id)}`; asserting the caller's label == that means a popup opened for
    // request A physically cannot resolve request B, even if it forged B's id in the payload. Tauri
    // resolves `window` from the native IPC message, so the label is unspoofable from JS.
    let label = format!("{AUTH_LABEL_PREFIX}{}", sanitize_window_label(&request_id));
    require_label(window.label(), &label)?;

    // No `remember` parameter: an untrusted renderer sending `false` would have been asking for LESS
    // privilege, so removing it is not a hardening — it is removing a value that now has one meaning.
    // Allow is unconditionally persistent; the popup discloses that instead of offering a toggle.
    let decision = if allowed {
        AuthDecision::Allow
    } else {
        AuthDecision::Deny
    };
    // C9 (D19): SERVER-SIDE arbiter enforcement — only the popup that currently owns the ACTIVE slot may
    // resolve. A queued (non-actionable) popup's webview cannot decide even if coerced into calling
    // respond_auth; the frontend button-disable is a reflection of this, not the gate. SEC-06: resolution
    // is still by the opaque `request_id` (a wrong id can't resolve a different request).
    match auth.resolve_active(&request_id, decision) {
        ResolveOutcome::Resolved(promoted) => {
            // Close this popup (labelled by `request_id`; `origin` is diagnostics-only) and promote the
            // next queued popup into the active slot.
            tracing::debug!(origin = %origin, %request_id, allowed, "respond_auth decision");
            if let Some(window) = app.get_webview_window(&label) {
                let _ = window.close();
            }
            if let Some(next) = promoted {
                arm_active_popup(&app, auth.inner(), &next);
            }
            Ok(())
        }
        ResolveOutcome::NotActive => {
            tracing::warn!(%request_id, "respond_auth rejected: not the active authorization popup");
            Err("not the active authorization request".to_string())
        }
    }
}

/// DTO for [`get_pending_auth`] — the SERVER-authoritative origin the popup must render (C9 D8), plus
/// whether this popup currently owns the actionable slot (C9 D15), so a queued popup disables its buttons.
#[derive(serde::Serialize)]
pub struct PendingAuthDto {
    pub origin: String,
    pub active: bool,
}

#[tauri::command]
pub fn get_pending_auth(
    window: tauri::WebviewWindow,
    auth: tauri::State<'_, AuthState>,
    request_id: String,
) -> Result<Option<PendingAuthDto>, String> {
    // C9 (D8/D19): bind the caller to ITS OWN request via the SAME exact-label guard respond_auth uses, so
    // a popup can only peek the origin/active-state of the request it was opened for, never another's.
    let label = format!("{AUTH_LABEL_PREFIX}{}", sanitize_window_label(&request_id));
    require_label(window.label(), &label)?;
    Ok(auth
        .peek(&request_id)
        .map(|(origin, active)| PendingAuthDto {
            origin: origin.to_string(),
            active,
        }))
}

// ── C9 single-active-popup arbiter — window helpers (lib, so both `respond_auth` here and
//    `windows::show_auth_popup_window` in the bin share one implementation) ──────────────────────────

/// C9 (D14/D18/D19): raise a promoted popup into the ACTIVE slot — topmost + focused — and arm its
/// activation-relative 60 s auto-deny. Called on every promotion (respond_auth / deny timer / window
/// close). No-op on the window itself if it was already closed; the arbiter state advanced regardless.
pub fn arm_active_popup(app: &tauri::AppHandle, auth_manager: &AuthState, request_id: &str) {
    let label = format!("{AUTH_LABEL_PREFIX}{}", sanitize_window_label(request_id));
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.set_always_on_top(true);
        let _ = window.set_focus();
    }
    spawn_active_deny_timer(app, auth_manager, request_id);
}

/// Spawn the ACTIVE popup's 60 s auto-deny. On fire: resolve Deny (which promotes the next queued request,
/// if any), close the window, then arm the promoted one — the chain drains the queue one active 60 s
/// window at a time. `resolve` is a no-op if the request was already decided (respond_auth / user close).
pub fn spawn_active_deny_timer(app: &tauri::AppHandle, auth_manager: &AuthState, request_id: &str) {
    let app = app.clone();
    let auth_manager = auth_manager.clone();
    let request_id = request_id.to_string();
    let label = format!("{AUTH_LABEL_PREFIX}{}", sanitize_window_label(&request_id));
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(crate::server::AUTH_DECISION_TIMEOUT).await;
        let promoted = auth_manager.resolve(&request_id, AuthDecision::Deny);
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.close();
        }
        if let Some(promoted) = promoted {
            arm_active_popup(&app, &auth_manager, &promoted);
        }
    });
}

/// C9 (D14): resolve-as-Deny + promote-next when the user CLOSES a popup without deciding. Idempotent with
/// the timer + respond_auth (both resolve first, then close — so the `Destroyed` fired by their own close
/// is a harmless no-op that promotes nobody).
pub fn attach_close_deny_listener(
    app: &tauri::AppHandle,
    auth_manager: &AuthState,
    window: &tauri::WebviewWindow,
    request_id: &str,
) {
    let app = app.clone();
    let auth_manager = auth_manager.clone();
    let request_id = request_id.to_string();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            if let Some(promoted) = auth_manager.resolve(&request_id, AuthDecision::Deny) {
                arm_active_popup(&app, &auth_manager, &promoted);
            }
        }
    });
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
/// The first-run onboarding wizard window (its own capability file grants only its commands).
pub const ONBOARDING_LABEL: &str = "onboarding";
/// The certificate-renewal consent window (macOS/Windows §7).
pub const RENEWAL_LABEL: &str = "renewal";
const AUTH_LABEL_PREFIX: &str = "auth-";

/// F-012 (D6) — the PRIMARY, framework-independent caller-label check behind the per-window capability
/// ACL. Even if a capability were ever mis-scoped, a command still refuses to act for the wrong window.
/// `actual` is the real invoking window's label, which Tauri resolves from the native IPC message — JS
/// cannot spoof it. On mismatch: log (generic) and return a generic error that leaks no window topology.
pub fn require_label(actual: &str, expected: &str) -> Result<(), String> {
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

/// Enable the encrypted (HTTPS) connection: generate certs, install browser trust, save config, start
/// HTTPS. Cross-platform via [`crate::trust`] — macOS raises the Keychain password dialog, Linux
/// installs into user NSS stores silently, Windows lands in a later phase (its trust backend errors
/// until then). Succeeds iff trust landed in ≥1 store (`install_ca_trust` errors otherwise), so on
/// Linux a missing `certutil` surfaces as an enable failure the wizard can show with a Retry (R3).
#[tauri::command]
pub async fn enable_https(
    window: tauri::WebviewWindow,
    config: tauri::State<'_, ConfigState>,
    shared_state: tauri::State<'_, SharedAppState>,
) -> Result<(), String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    enable_https_inner(&config, &shared_state).await
}

/// The shared enable-HTTPS routine, callable outside a Tauri command (the onboarding wizard reuses
/// it). Generate certs → install browser trust → ensure the listener is LIVE → save config. Errors if
/// trust lands in zero stores (R3) or the HTTPS listener can't bind, so the wizard renders HTTPS as
/// failed-with-Retry. `async` because it AWAITS the real bind before persisting `https_enabled`.
async fn enable_https_inner(
    config: &ConfigState,
    shared_state: &crate::server::AppState,
) -> Result<(), String> {
    use crate::certs;
    use std::sync::atomic::Ordering;

    // B4 (codex): preflight — if a newer build wrote the config we can't persist `https_enabled = true`, so
    // refuse BEFORE generating certs / installing CA trust rather than mutating the OS trust store only to
    // fail the commit at the end. (Reached from both the Settings toggle and onboarding.)
    require_persistable(config)?;

    // Take the HTTPS bring-up lock FIRST — before ANY cert inspection or write. `certs_exist()` reads
    // the leaf+key and `generate_and_save()` writes a whole new set; running either while the launch
    // gate is mid-rotation could observe a MIXED new-leaf/old-key set and then write a third set over
    // it, corrupting the live one and failing both starts (post-impl codex Medium). Waiting here is
    // correct and unbounded-by-design: the owner may be showing an OS trust dialog or shelling out to
    // certutil per NSS store, so there is no honest fixed timeout. The guard releases on every exit
    // path below (including `?`), and moves into `spawn_https` when we get that far.
    let lifecycle = crate::server::claim_https_lifecycle(shared_state).await;
    // The owner we waited for may have completed the whole bring-up. Re-read the live bind state
    // rather than assuming what we saw before the wait.
    //
    let already_bound = shared_state.https_bound.load(Ordering::Relaxed);

    // SEC-08 (post-impl codex M1): the startup path runs this same fail-closed migration before it
    // brings up HTTPS (main.rs). Without mirroring it here, a Settings off→on toggle would re-enable
    // HTTPS next to a readable legacy mint-any-cert key on upgraded installs — reopening exactly
    // the condition the startup gate closes. Fail closed: if the legacy key cannot be removed, refuse
    // to enable (surfaced to the Settings UI). HTTP is unaffected. (No-op on installs that never had
    // an on-disk CA key, i.e. every non-macOS install.)
    certs::migrate_legacy_ca_key().map_err(|e| {
        format!("Legacy CA key could not be removed; refusing to enable HTTPS: {e}")
    })?;

    // Generate + install trust ONLY when needed. Already set up (valid certs + trusted anchor)? Skip
    // the (re-)install and its OS prompt (post-impl review) — but STILL ensure the listener is running
    // below. The old short-circuit RETURNED here without spawning, so after disable→restart→re-enable
    // (certs kept, trusted, but the launch gate didn't start HTTPS because config was off) HTTPS never
    // served until yet another restart (post-impl codex High).
    if !(certs::certs_exist() && certs::is_ca_trusted()) {
        certs::generate_and_save().map_err(|e| format!("Failed to generate certificates: {e}"))?;
        certs::install_ca_trust().map_err(|e| format!("Certificate trust was not granted: {e}"))?;
    } else {
        tracing::info!("Certs already present + trusted — skipping re-install");
    }

    // Is the RUNNING listener still serving the identity that's now on disk? Computed HERE, after the
    // cert work above, not before it: `generate_and_save()` may have just minted a new set, so a
    // fingerprint sampled earlier would compare against the pre-regeneration files and wrongly report
    // "still current" (post-impl codex High). `https_bound` alone can't answer this — the acceptor is
    // fixed for the life of the serving loop, so after any rotation the files no longer describe what
    // we serve, and committing "enabled" over that would present a cert whose anchor may be gone.
    let serving_stale_identity =
        already_bound && *shared_state.served_ca_fingerprint.read() != certs::live_ca_fingerprint();

    // Ensure the HTTPS listener is LIVE. If launch already bound it, don't double-spawn; otherwise
    // spawn and AWAIT the real bind, persisting `https_enabled = true` ONLY once the listener is up
    // (post-impl codex High: a swallowed bind failure used to report success while Safari/strict
    // users silently fell back / failed).
    // We hold the bring-up lock, so no other path can be binding right now: `already_bound` (sampled
    // after the wait) is authoritative. If HTTPS isn't up yet, WE bring it up and await the real bind.
    if !already_bound {
        // The guard drops (releasing the lock) if this load fails.
        let tls_config =
            certs::load_rustls_config().map_err(|e| format!("Failed to load TLS config: {e}"))?;
        // The clone shares the Arc'd https_bound flag with the managed state, so start_https flipping
        // it after a successful bind is visible to /health. (Q7) We keep holding `lifecycle` across
        // the bind AND the `https_enabled = true` commit below — releasing at the bind would let
        // "Remove certificate trust" slip in between, remove the anchor and save `false`, only for our
        // `true` to overwrite it, leaving "enabled" HTTPS with an untrusted CA (post-impl codex).
        let ready = crate::server::spawn_https(shared_state.clone(), tls_config);
        if !matches!(ready.await, Ok(true)) {
            return Err(
                "Certificates are ready, but the HTTPS server could not start \
                 (port 59834 may be in use). Please try again."
                    .to_string(),
            );
        }
    }

    // A listener serving a stale identity can't be fixed in-process — the acceptor is fixed for the
    // life of the loop and the port is held by us, so there is nothing to rebind. Persist the enable
    // (the certs + trust ARE set up) and tell the user a restart finishes it, rather than silently
    // reporting success while HTTPS presents an untrusted cert.
    if serving_stale_identity {
        mutate_config(config, |cfg| cfg.https_enabled = true)?;
        tracing::warn!("HTTPS is running with a previous certificate — restart required");
        drop(lifecycle);
        return Err(
            "HTTPS is set up, but the running server is still using a previous \
                    certificate. Restart Aztec Accelerator to finish enabling it."
                .to_string(),
        );
    }

    mutate_config(config, |cfg| cfg.https_enabled = true)?;
    tracing::info!("HTTPS enabled");
    // `lifecycle` drops HERE — after the config commit, so the whole transaction (certs → trust →
    // bind → `https_enabled = true`) is atomic with respect to removal/renewal/launch.
    drop(lifecycle);
    Ok(())
}

/// Shared autostart toggle (used by the outer `set_autostart` Settings command + the onboarding
/// wizard). Window-agnostic — the caller-label guard lives in `set_autostart`; `complete_onboarding`
/// calls this from the onboarding window. The F-010 path preflight, the C8 `enable_transaction`
/// rollback (now with exact-prior-artifact restore), and the D19 `autostart.lock` all live inside
/// `autostart::set_enabled` — the owned replacement for the removed plugin (plan D7).
fn set_autostart_inner(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    crate::autostart::set_enabled(app, enabled)
}

/// Disable the encrypted (HTTPS) connection: save config off. HTTPS stops on next restart. Trust
/// anchors are left in place (removing them is the separate [`remove_https_trust`] action, so a
/// re-enable doesn't re-prompt — D5/A4).
#[tauri::command]
pub async fn disable_https(
    window: tauri::WebviewWindow,
    config: tauri::State<'_, ConfigState>,
    shared_state: tauri::State<'_, SharedAppState>,
) -> Result<(), String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    // Every OTHER HTTPS state transition takes this lock; disable skipping it was the hole. Trigger:
    // disable while another window's enable is parked on an OS trust dialog — disable reports success,
    // then the older enable finishes and overwrites `https_enabled` back to true (post-impl codex).
    let _lifecycle = crate::server::claim_https_lifecycle(&shared_state).await;
    mutate_config(&config, |cfg| cfg.https_enabled = false)?;
    tracing::info!("HTTPS disabled via Settings (HTTPS stops on next restart)");
    Ok(())
}

/// Explicitly remove the local CA from every browser trust store (the "Remove certificate trust"
/// Settings action — D5). Also flips HTTPS off so the app stops presenting a now-untrusted cert.
#[tauri::command]
pub async fn remove_https_trust(
    window: tauri::WebviewWindow,
    config: tauri::State<'_, ConfigState>,
    shared_state: tauri::State<'_, SharedAppState>,
) -> Result<(), String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    // B4 (codex): preflight — refuse against a newer-schema config BEFORE removing trust, so we don't strip
    // the CA from the user's stores only to fail the `https_enabled = false` commit (which would leave HTTPS
    // "on" in the newer config with its anchor gone). The save path re-checks too; this avoids the side effect.
    require_persistable(&config)?;
    // Removal MUTATES trust state, so it joins the same serialization as launch/enable/renewal. The
    // Settings window offers this button and the HTTPS toggle at once (and enable can sit on a
    // Keychain dialog), so without the lock a removal could delete every anchor and report success
    // while a concurrent enable/renewal installed a new one — or interleave with enable's final
    // config commit and leave "enabled" HTTPS with an untrusted CA (post-impl codex Medium). Held
    // across BOTH the removal and the `https_enabled = false` write, mirroring enable's transaction.
    let _lifecycle = crate::server::claim_https_lifecycle(&shared_state).await;
    let report = crate::trust::remove_ca_trust(&crate::certs::live_ca_cert_path());
    // Stop serving the now-to-be-untrusted cert regardless of whether every store could be cleaned.
    mutate_config(&config, |cfg| cfg.https_enabled = false)?;
    // Surface a partial/failed removal instead of reporting success (post-impl codex High): the
    // Settings action shows the error, so the user knows trust may still be installed.
    if let Some(detail) = report.removal_failure_detail() {
        tracing::warn!(%detail, "CA trust removal incomplete");
        return Err(format!(
            "HTTPS was turned off, but the certificate trust could not be fully removed: {detail}"
        ));
    }
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
    /// HTTPS is pre-checked for everyone incl. upgraders (A9/§2.1). Start-on-Login + Auto-Update
    /// default to the recommended YES in the wizard UI (not reflected here — the wizard is a
    /// "recommended setup", and computing OS/config state for the toggle defaults would only let an
    /// upgrader's prior opt-out silently re-enable on Start).
    pub https_default: bool,
}

#[tauri::command]
pub fn get_onboarding_state(window: tauri::WebviewWindow) -> Result<OnboardingState, String> {
    require_label(window.label(), ONBOARDING_LABEL)?;
    // Intentionally cheap + side-effect-free: the per-OS certificate copy is chosen from `platform`;
    // no autostart/config/trust probing (the wizard defaults all toggles to YES).
    Ok(OnboardingState {
        platform: std::env::consts::OS.to_string(),
        https_default: true,
    })
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
/// wizard returns next launch.
///
/// This is the wizard's ONLY exit that sets the marker. There is deliberately no separate "skip"
/// command: unchecking the toggles and pressing the primary button expresses the same outcome more
/// deliberately, and it keeps the marker tied to an actual decision. Closing the window instead leaves
/// the marker unset, so the wizard reappears — the reversible escape hatch.
#[tauri::command]
pub async fn complete_onboarding(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    config: tauri::State<'_, ConfigState>,
    shared_state: tauri::State<'_, SharedAppState>,
    https: bool,
    autostart: bool,
    auto_update: bool,
) -> Result<OnboardingResult, String> {
    require_label(window.label(), ONBOARDING_LABEL)?;
    let https_res = if https {
        enable_https_inner(&config, &shared_state).await
    } else {
        // Explicit opt-out: persist HTTPS OFF (post-impl codex High — the old `Ok(())` no-op left
        // `https_enabled` at whatever it was, so unchecking HTTPS on "Run setup again" from an
        // already-on install silently kept it on). A fresh install is already `false`; this makes the
        // decline authoritative in every case.
        mutate_config(&config, |cfg| cfg.https_enabled = false)
    };
    // If the user REQUESTED HTTPS but it failed, don't leave a stale `true` from a prior enable —
    // reflect the real (off) state so Settings and the next launch agree.
    if https && https_res.is_err() {
        let _ = mutate_config(&config, |cfg| cfg.https_enabled = false);
    }
    let autostart_res = set_autostart_inner(&app, autostart);
    let auto_update_res = mutate_config(&config, |cfg| cfg.auto_update = Some(auto_update));

    let all_ok = https_res.is_ok() && autostart_res.is_ok() && auto_update_res.is_ok();
    let completed = all_ok
        && mutate_config(&config, |cfg| {
            cfg.onboarding_version = crate::config::ONBOARDING_VERSION
        })
        .is_ok();

    // F-012 convention: windows are closed from RUST, never by the page (no core:window JS grants).
    // On full success the wizard is done — close it; on partial failure it stays open for Retry.
    // A failed native close propagates as Err so wireButton re-enables the controls (merge-audit
    // Low): every completed action is idempotent, so the user just clicks Start again.
    if completed {
        window
            .close()
            .map_err(|e| format!("setup completed, but the window could not be closed: {e}"))?;
    }

    Ok(OnboardingResult {
        https: https_res,
        autostart: autostart_res,
        auto_update: auto_update_res,
        completed,
    })
}

// ── Certificate renewal (macOS/Windows renewal consent window — §7) ──

/// "Renew now" from the renewal consent window: rotate the cert identity (raises the OS trust dialog
/// with context, unlike a surprise background prompt). Records the prompt time for throttling.
/// `async` (like `enable_https`/`complete_onboarding`) so the blocking subprocess + modal OS dialog
/// don't freeze the webview event loop / the "Renewing…" spinner (post-impl review).
#[tauri::command]
pub async fn renew_cert(
    window: tauri::WebviewWindow,
    config: tauri::State<'_, ConfigState>,
    shared_state: tauri::State<'_, SharedAppState>,
) -> Result<(), String> {
    require_label(window.label(), RENEWAL_LABEL)?;
    // B4 (codex): preflight — renewal rotates the cert set (mutates trust) and then records the throttle;
    // if a newer build wrote the config we can't record it, so refuse BEFORE rotating rather than rotating
    // and silently failing the (currently ignored) `last_rotation_prompt_at` write.
    require_persistable(&config)?;

    // Claim the prove permit BEFORE rotating, not after.
    //
    // Rotating is only useful if we can then restart: the running listener serves the OLD leaf from an
    // in-memory TlsAcceptor that is never hot-reloaded, so a rotation without a restart leaves the new
    // leaf unused — and, because we would also record the prompt throttle, unprompted. Renewal then
    // reported success while changing nothing observable, and the old leaf could expire with the tray
    // app still open (post-impl codex High).
    //
    // Rotating first and *then* discovering we cannot restart is the bug; deciding first is the fix.
    // The permit is held across the (diverging) restart, which also stops a proof from starting in the
    // gap — Tauri's restart calls exit(0), skipping bb's `kill_on_drop` and the prove-workspace
    // TempDir destructors, so restarting mid-proof would orphan `bb` and leave the witness on disk.
    let Ok(_prove_permit) = shared_state.prove_semaphore.try_acquire() else {
        tracing::info!("Renewal requested while a proof is in flight — asking the user to retry");
        return Err(
            "A proof is running right now. Renewing restarts the app, so please try again in a \
             moment — your certificate is still valid."
                .to_string(),
        );
    };

    // Renewal MUTATES the cert set (`rotate_now` stages a new CA/leaf/key, trusts it, then swaps), so
    // it takes the same bring-up lock as launch + enable. The renewal and onboarding windows can be
    // open at once; without this a rotation could swap the files while another path reads or writes
    // them, recreating exactly the MIXED-set corruption the lock exists to prevent.
    let lifecycle = crate::server::claim_https_lifecycle(&shared_state).await;
    crate::certs::rotate_now().map_err(|e| format!("Certificate renewal failed: {e}"))?;
    drop(lifecycle);

    let _ = mutate_config(&config, |cfg| {
        cfg.last_rotation_prompt_at = Some(now_unix_secs());
    });

    tracing::info!("Certificate renewed via consent window — restarting to serve the new leaf");
    window.app_handle().restart();
}

/// Record that the renewal window was shown/declined (throttles re-prompting).
#[tauri::command]
pub fn record_renewal_prompt(
    window: tauri::WebviewWindow,
    config: tauri::State<'_, ConfigState>,
) -> Result<(), String> {
    require_label(window.label(), RENEWAL_LABEL)?;
    mutate_config(&config, |cfg| {
        cfg.last_rotation_prompt_at = Some(now_unix_secs());
    })?;
    // "Later" dismisses the window — closed from Rust (F-012); a failed close propagates so
    // wireButton re-enables the button (merge-audit Low).
    window
        .close()
        .map_err(|e| format!("recorded, but the window could not be closed: {e}"))?;
    Ok(())
}

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
/// - action="update": install the DISPLAYED update (if still pending), best-effort save the preference
/// - action="later": dismiss, auto_update stays None (prompt returns next launch)
#[tauri::command]
pub fn respond_update_prompt(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    config: tauri::State<'_, ConfigState>,
    pending: tauri::State<'_, PendingUpdate>,
    action: String,
    auto_update: bool,
    // B2 (F8): the version string the prompt is CURRENTLY displaying (from its URL param). The install
    // proceeds only if this still equals the pending update's version — see `take_or_reprompt`.
    displayed_version: String,
) -> Result<(), String> {
    require_label(window.label(), UPDATE_PROMPT_LABEL)?;
    match action.as_str() {
        "update" => {
            // B2 (F8): consume the stored update ONLY IF it is still the version the user is looking at.
            // No unconditional take — a background re-check that swapped the pending update since this
            // prompt rendered cannot be installed on the stale click. On mismatch we get an opaque
            // `Reprompt` capability that re-points the window at the real version WITHOUT ever exposing
            // that version to this code, so it can't be turned back into a forced take.
            match pending.take_or_reprompt(&displayed_version) {
                TakeOutcome::Took(update) => {
                    // The displayed version matched — this IS the consented install. Persist the
                    // auto-update preference as BEST-EFFORT FIRST, then spawn (codex B2 round-3): on
                    // Windows a successful `install()` exits the process, so spawning before the
                    // synchronous save could interrupt it — save first removes that ordering hazard. The
                    // earlier "save-then-restore-on-failure" was wrong twice over (`mutate_config` mutates
                    // in-memory before the disk write, and the restore could clobber a newer pending
                    // update), so on a save failure we now only warn: the install is exactly the version
                    // the user clicked, and an unsaved preference is simply re-asked next launch (never
                    // less safe). Mirrors the approved-origin persist policy.
                    let version = update.version().to_string();
                    if let Err(e) =
                        mutate_config(&config, |cfg| cfg.auto_update = Some(auto_update))
                    {
                        tracing::warn!(
                            error = %e, version = %version,
                            "Consented update is installing, but persisting the auto-update preference failed; it will be re-asked next launch"
                        );
                    } else {
                        tracing::info!(version = %version, auto_update, "User clicked Update Now, downloading the displayed update");
                    }
                    let handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        crate::updater::perform_update(&handle, update).await;
                        // If perform_update returns (error), close the prompt
                        close_update_prompt(&handle);
                    });
                }
                TakeOutcome::Reprompt(cap) => {
                    // Version mismatch: the opaque capability logs + re-points the prompt at the pending
                    // version inside `pending_update`; the version never reaches this code.
                    cap.navigate(&app, env!("CARGO_PKG_VERSION"), &displayed_version);
                }
                TakeOutcome::Empty => {
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

    // B2 (F8): the update-consent version-binding primitive is tested inside `mod pending_update`
    // (`take_or_reprompt_takes_only_on_match_and_lends_version_on_mismatch`), where the test can reach
    // the private slot storage. The command wiring (`respond_update_prompt`) can't be unit-tested
    // end-to-end because `VerifiedUpdate` has a private, network-gated constructor — but only the
    // `TakeOutcome::Took` arm persists the preference or spawns an install, so a mismatch (which
    // resolves to `Reprompted`, carrying no update) provably does neither.
}
