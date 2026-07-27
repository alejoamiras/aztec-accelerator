use crate::authorization::{AuthDecision, AuthorizationManager, ResolveOutcome};
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
    // codex #7: surface an I/O error rather than reporting `false` (disabled). Reading the launcher entry
    // can fail (permissions, malformed unit); pretending "off" would mislead the user into thinking
    // autostart is disabled when its true state is unknown.
    app.autolaunch()
        .is_enabled()
        .map_err(|e| format!("cannot read autostart state: {e}"))
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
        // it after a successful bind is visible to /health. (Q7) The guard moves into the spawned
        // task, which releases it as soon as the bind resolves.
        let ready = crate::server::spawn_https(shared_state.clone(), tls_config, lifecycle);
        if !matches!(ready.await, Ok(true)) {
            return Err(
                "Certificates are ready, but the HTTPS server could not start \
                 (port 59834 may be in use). Please try again."
                    .to_string(),
            );
        }
    }

    mutate_config(config, |cfg| cfg.https_enabled = true)?;
    tracing::info!("HTTPS enabled");
    Ok(())
}

/// Shared autostart toggle (used by the outer `set_autostart` Settings command + the onboarding
/// wizard). Window-agnostic — the caller-label guard lives in `set_autostart`; `complete_onboarding`
/// calls this from the onboarding window. Carries main's F-010 executable-path safety + the C8
/// crash-recovery `enable_transaction` rollback so a partial failure never leaves a half-enabled state.
fn set_autostart_inner(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        // F-010: refuse autostart if this executable's path could inject into any OS launcher serializer
        // (systemd unit / .desktop / plist / Run-key) — BEFORE the plugin serializes it. Fail closed.
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
        // C8 (D20): enable the launcher entry + arm crash recovery as ONE transaction; on a partial
        // failure roll back against a known baseline. An unknown current state is undecidable → fail closed.
        let prior_enabled = manager
            .is_enabled()
            .map_err(|e| format!("cannot determine current autostart state: {e}"))?;
        crate::crash_recovery::enable_transaction(
            prior_enabled,
            || manager.enable().map_err(|e| e.to_string()),
            crate::crash_recovery::enable_crash_recovery,
            || manager.disable().map_err(|e| e.to_string()),
            crate::crash_recovery::disable_crash_recovery,
        )?;
    } else {
        // Disable the launcher, THEN disarm crash recovery — surface a non-confirmed disarm rather than
        // leaving the app able to relaunch on next login while the UI shows autostart as off.
        manager.disable().map_err(|e| e.to_string())?;
        if !crate::crash_recovery::disable_crash_recovery() {
            return Err(
                "Autostart launcher disabled, but crash recovery could not be confirmed disarmed — \
                 the app may still relaunch on next login. Please retry."
                    .to_string(),
            );
        }
    }
    Ok(())
}

/// Disable the encrypted (HTTPS) connection: save config off. HTTPS stops on next restart. Trust
/// anchors are left in place (removing them is the separate [`remove_https_trust`] action, so a
/// re-enable doesn't re-prompt — D5/A4).
#[tauri::command]
pub fn disable_https(
    window: tauri::WebviewWindow,
    config: tauri::State<'_, ConfigState>,
) -> Result<(), String> {
    require_label(window.label(), SETTINGS_LABEL)?;
    mutate_config(&config, |cfg| cfg.https_enabled = false)?;
    tracing::info!("HTTPS disabled via Settings (HTTPS stops on next restart)");
    Ok(())
}

/// Explicitly remove the local CA from every browser trust store (the "Remove certificate trust"
/// Settings action — D5). Also flips HTTPS off so the app stops presenting a now-untrusted cert.
#[tauri::command]
pub fn remove_https_trust(
    window: tauri::WebviewWindow,
    config: tauri::State<'_, ConfigState>,
) -> Result<(), String> {
    require_label(window.label(), SETTINGS_LABEL)?;
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
/// wizard returns next launch — unless the user explicitly dismisses via [`dismiss_onboarding`].
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

/// Mark onboarding complete without (further) action — the explicit "Continue without HTTPS" (after a
/// failed cert install) and "Skip for now" paths (R4). The ONLY unconditional marker set.
#[tauri::command]
pub fn dismiss_onboarding(
    window: tauri::WebviewWindow,
    config: tauri::State<'_, ConfigState>,
) -> Result<(), String> {
    require_label(window.label(), ONBOARDING_LABEL)?;
    mutate_config(&config, |cfg| {
        cfg.onboarding_version = crate::config::ONBOARDING_VERSION
    })?;
    tracing::info!("Onboarding dismissed (marker set)");
    // F-012: closed from Rust (the page has no core:window grant); a failed close propagates so
    // wireButton re-enables Skip (merge-audit Low). The marker set above is idempotent on retry.
    window
        .close()
        .map_err(|e| format!("dismissed, but the window could not be closed: {e}"))?;
    Ok(())
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
    crate::certs::rotate_now().map_err(|e| format!("Certificate renewal failed: {e}"))?;
    let _ = mutate_config(&config, |cfg| {
        cfg.last_rotation_prompt_at = Some(now_unix_secs());
    });
    // The running HTTPS listener still serves the OLD leaf from an in-memory TlsAcceptor that isn't
    // hot-reloaded (see certs::rotate) — the freshly-rotated leaf only takes effect on the next launch.
    // A restart serves it immediately, BUT Tauri's restart calls exit(0), skipping bb's `kill_on_drop`
    // + the prove-workspace TempDir destructors: restarting mid-proof would orphan `bb` and leave the
    // witness on disk (post-impl codex High). So only restart when NO proof is in flight — try_acquire
    // the single prove permit: holding it across the (diverging) restart also prevents a proof from
    // starting in the gap. If a proof IS running, defer: the old leaf is still valid (renewal runs ~30
    // days before expiry), so the new one applies on the next natural launch.
    match shared_state.prove_semaphore.try_acquire() {
        Ok(_permit) => {
            tracing::info!(
                "Certificate renewed via consent window — restarting to serve the new leaf"
            );
            window.app_handle().restart();
        }
        Err(_) => {
            tracing::info!(
                "Certificate renewed while a proof is in flight — the new leaf applies on next launch"
            );
            window
                .close()
                .map_err(|e| format!("renewed, but the window could not be closed: {e}"))?;
            Ok(())
        }
    }
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
