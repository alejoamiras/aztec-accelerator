use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use http::Method;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::authorization::AuthorizationManager;
use crate::{bb, config, versions};
use parking_lot::RwLock;
use tokio::sync::Semaphore;

mod bind;
pub use bind::bind_with_retry;
mod probe;
pub use probe::{healthy_aztec_on_port, healthy_aztec_version_on_port};
mod owner;
pub use owner::{classify_port_owner, may_bow_out, PortOwner};
mod auth;
mod host;
mod prove;

const PORT: u16 = 59833;
pub const HTTPS_PORT: u16 = 59834;

/// F-009: cap on concurrently in-flight + waiting authorized `/prove` requests (one holds the
/// prove permit; the rest wait to buffer their body). Bounds the total queue so a burst of slow
/// uploaders can't stack fresh per-request read timeouts and starve a legitimate request for
/// minutes — excess authorized requests are shed immediately with 429 instead of queueing.
pub(crate) const MAX_INFLIGHT_PROVE: usize = 8;

/// Fallback Aztec bb version reported by `/health` + used as the proving default when no version is
/// injected via `HeadlessState.bundled_version`. Core is `build.rs`-free, so this replaces the old
/// `env!("AZTEC_BB_VERSION")` (which only existed via src-tauri/build.rs). Consumers inject the real
/// version: the GUI from its build.rs env, the headless server from the copy-bb.ts hook (Phase 3).
pub const DEFAULT_BB_VERSION: &str = "unknown";

/// How long the server waits for the user's origin-authorization decision before timing out the
/// `/prove` request, AND how long the popup window waits before auto-denying. Two halves of one UX
/// contract — shared so the server-side timeout and the popup auto-deny can't drift (windows.rs
/// imports this).
pub const AUTH_DECISION_TIMEOUT: Duration = Duration::from_secs(60);

/// C9 (D18): absolute upper bound on how long a `/prove` request blocks waiting for a decision. The REAL
/// per-popup 60 s deadline is enforced by the popup's ACTIVATION-armed auto-deny timer (windows.rs) — so a
/// request queued behind a busy popup is NOT denied at 60 s-from-enqueue (the starvation bug). This
/// backstop only bounds the total queued wait — at most `MAX_PENDING_ORIGINS` popups each taking up to
/// `AUTH_DECISION_TIMEOUT` — so a queued request that somehow never surfaces can't block forever.
pub const AUTH_QUEUE_BACKSTOP: Duration = Duration::from_secs(
    // `MAX_PENDING_ORIGINS + 1` (not exactly ×MAX): the last-queued request only becomes active after the
    // MAX-1 ahead of it each drain a full 60 s, so a bare ×MAX would make its /prove backstop coincide with
    // the END of its own 60 s activation window. The +1 gives it a full window of margin (code-review).
    AUTH_DECISION_TIMEOUT.as_secs() * (crate::authorization::MAX_PENDING_ORIGINS as u64 + 1),
);

/// Status surfaced to the tray via the `on_status` callback during a `/prove` request.
/// `display_text()` MUST stay byte-identical to the legacy `"Status: …"` string literals — the
/// tray label and the `prove_success_path_and_status_sequence` characterization test both pin
/// them. `is_busy()` drives the tray spinner (true ⟺ work in flight: Downloading or Proving).
/// Replaces the prior stringly-typed `Fn(&str)` callback so the tray consumer matches on variants
/// instead of substring-sniffing the label text (Q10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerStatus {
    Idle,
    Downloading,
    Proving,
}

impl ServerStatus {
    /// Tray label text. Byte-identical to the pre-Q10 string literals (behavior-preserving).
    pub fn display_text(self) -> &'static str {
        match self {
            ServerStatus::Idle => "Status: Idle",
            ServerStatus::Downloading => "Status: Downloading bb...",
            ServerStatus::Proving => "Status: Proving...",
        }
    }

    /// Whether work is in flight (drives the tray spinner). True for exactly Downloading + Proving
    /// — matching the prior `text.contains("Proving") || text.contains("Downloading")` consumer.
    pub fn is_busy(self) -> bool {
        matches!(self, ServerStatus::Downloading | ServerStatus::Proving)
    }
}

pub type StatusCallback = Arc<dyn Fn(ServerStatus) + Send + Sync>;
pub type VersionsChangedCallback = Arc<dyn Fn() + Send + Sync>;

/// Callback to show the authorization popup window. Takes the origin + the opaque `request_id`
/// (SEC-06) the popup must echo back to `respond_auth` so decisions resolve by id, not origin.
/// q7e3-F-08: the popup receives the validated `&CanonicalOrigin` (plus the opaque request id) — the
/// desktop layer can no longer be handed a non-canonical origin string.
pub type ShowAuthPopupCallback =
    Arc<dyn Fn(&crate::authorization::CanonicalOrigin, &str) + Send + Sync>;

/// The server-side core state — everything the headless `accelerator-server` needs, with no GUI
/// coupling. Lives behind an `Arc` in [`AppState`] so cloning the state is cheap (fixes the main.rs
/// clone-stutter) (Q1).
#[derive(Clone)]
pub struct HeadlessState {
    /// Override for where an approved origin is persisted. `None` (production) means
    /// [`crate::config::config_path`]. `pub(crate)` so it CANNOT be set outside this crate — the
    /// only writers are core's own tests, which redirect the now-unconditional approve-and-save
    /// path away from the developer's real config file (post-impl codex: encode that as visibility,
    /// not as a comment).
    pub(crate) config_path: Option<std::path::PathBuf>,
    pub bundled_version: Option<String>,
    /// Injected app/release version for `/health.version`, decoupling it from `env!("CARGO_PKG_VERSION")`
    /// (which resolves to whichever crate compiles this module — wrong once `server.rs` lives in core).
    /// Always set — by the binary (its release-patched version) via [`HeadlessState::headless`], or to
    /// core's compile-time version by [`Default`]. (F-01)
    pub app_version: String,
    /// `true` once `start_https` has actually bound the HTTPS listener. Shared (Arc'd atomic) across
    /// the HTTP + HTTPS servers so `/health` advertises `https_port` from the REAL bind state — not
    /// the config flag, which would point the SDK at a dead port when the CA is untrusted at startup
    /// (HTTPS skipped, but `https_enabled` config stays on). (Q7)
    pub https_bound: Arc<AtomicBool>,
    /// Serializes the ENTIRE HTTPS bring-up: cert-set inspection/generation/trust-install, the
    /// pre-expiry rotation, the TLS load, and the bind. The launch gate and the Settings/onboarding
    /// enable path can both decide to start HTTPS at the same moment; without one owner they raced in
    /// three separate ways (post-impl codex): two listeners fighting for the port with the loser
    /// reporting failure while HTTPS was live; one path loading the cert set mid-`swap_into` and
    /// getting a MIXED leaf/key; and two paths concurrently WRITING cert sets, leaving the live set
    /// corrupt. An async mutex (not an atomic flag) so a waiter blocks exactly as long as the owner
    /// takes — cert generation can raise an OS trust dialog and NSS rotation shells out per store, so
    /// no fixed timeout can bound it correctly. Held only until the bind resolves; the serving loop
    /// runs unlocked (see the GUI's `start_https`).
    pub https_lifecycle: Arc<tokio::sync::Mutex<()>>,
    /// Fingerprint of the CA the RUNNING listener is actually serving (set when it binds). `https_bound`
    /// only says a listener exists — it cannot say *which* cert identity that listener holds, because a
    /// rotation swaps the files while the in-memory `TlsAcceptor` keeps the old leaf until relaunch. A
    /// re-enable that skips rebinding on `https_bound` alone could therefore commit "HTTPS enabled"
    /// while the listener serves a cert whose anchor was since removed (post-impl codex Medium).
    /// Compared against `certs::live_ca_fingerprint()` to detect exactly that.
    pub served_ca_fingerprint: Arc<RwLock<Option<String>>>,
    pub config: Option<Arc<RwLock<config::AcceleratorConfig>>>,
    pub auth_manager: Option<Arc<AuthorizationManager>>,
    /// Limits concurrent proving to 1 — bb already uses all cores. Always present (F-01).
    pub prove_semaphore: Arc<Semaphore>,
    /// F-009: bounds total in-flight + waiting authorized `/prove` requests (`MAX_INFLIGHT_PROVE`).
    /// Acquired (try, non-blocking) right after origin auth and held for the whole request, so a
    /// burst of slow uploaders is shed with 429 rather than stacking per-request read timeouts.
    pub prove_waiters: Arc<Semaphore>,
}

/// Full app state: the headless `core` plus the optional GUI callbacks. `Deref`s to `core`, so the
/// existing `state.<core_field>` reads are unchanged; the 3 GUI callbacks stay flat (each individually
/// optional — a headless build or a focused test sets only a subset) (Q1).
#[derive(Clone, Default)]
pub struct AppState {
    pub core: Arc<HeadlessState>,
    pub on_status: Option<StatusCallback>,
    pub on_versions_changed: Option<VersionsChangedCallback>,
    pub show_auth_popup: Option<ShowAuthPopupCallback>,
}

impl std::ops::Deref for AppState {
    type Target = HeadlessState;
    fn deref(&self) -> &HeadlessState {
        &self.core
    }
}

impl Default for HeadlessState {
    /// `prove_semaphore` + `app_version` are always present; `app_version` falls back to core's
    /// compile-time version (binaries inject their own via [`HeadlessState::headless`]). (F-01)
    fn default() -> Self {
        Self {
            config_path: None,
            bundled_version: None,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            https_bound: Arc::new(AtomicBool::new(false)),
            https_lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            served_ca_fingerprint: Arc::new(RwLock::new(None)),
            config: None,
            auth_manager: None,
            prove_semaphore: Arc::new(Semaphore::new(1)),
            prove_waiters: Arc::new(Semaphore::new(MAX_INFLIGHT_PROVE)),
        }
    }
}

impl HeadlessState {
    /// Construct headless server state. `app_version` is injected by the binary (its release-patched
    /// version); `config`/`auth_manager`/`bundled_version` stay optional (the headless binary runs with
    /// `config: None` when no origin gating is configured). (F-01)
    pub fn headless(
        app_version: impl Into<String>,
        bundled_version: Option<String>,
        config: Option<Arc<RwLock<config::AcceleratorConfig>>>,
        auth_manager: Option<Arc<AuthorizationManager>>,
    ) -> Self {
        Self {
            config_path: None,
            app_version: app_version.into(),
            bundled_version,
            https_bound: Arc::new(AtomicBool::new(false)),
            https_lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            served_ca_fingerprint: Arc::new(RwLock::new(None)),
            config,
            auth_manager,
            prove_semaphore: Arc::new(Semaphore::new(1)),
            prove_waiters: Arc::new(Semaphore::new(MAX_INFLIGHT_PROVE)),
        }
    }
}

impl AppState {
    /// Headless: core state with no GUI callbacks.
    pub fn headless(core: HeadlessState) -> Self {
        Self {
            core: Arc::new(core),
            on_status: None,
            on_versions_changed: None,
            show_auth_popup: None,
        }
    }

    /// Desktop: core state plus the 3 GUI callback slots (flat — no wrapper struct, keeps `core`
    /// GUI-agnostic). (F-01)
    pub fn desktop(
        core: HeadlessState,
        on_status: StatusCallback,
        on_versions_changed: VersionsChangedCallback,
        show_auth_popup: ShowAuthPopupCallback,
    ) -> Self {
        Self {
            core: Arc::new(core),
            on_status: Some(on_status),
            on_versions_changed: Some(on_versions_changed),
            show_auth_popup: Some(show_auth_popup),
        }
    }
}

pub async fn start(state: AppState) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // F-06 (round 3): the ONE place both binaries pass through on the way up, so the cache-size cap
    // gets a chance to bind even when the previous run left it over the limit — post-download
    // eviction cannot help a cache nothing is downloading into. Detached: never delay the listener.
    // Same fallback the post-download cleanup uses (`prove.rs`): the headless binary only sets
    // `bundled_version` when `AZTEC_BB_VERSION` is in its environment, so keying on `Some` would have
    // made the sweep a no-op on a default headless deployment. The `"unknown"` sentinel parses as a
    // version and is never in the cache, so it protects nothing that should be evicted.
    let bundled = state
        .bundled_version
        .clone()
        .unwrap_or_else(|| DEFAULT_BB_VERSION.to_string());

    let app = router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    let listener = bind_with_retry(addr).await?;
    tracing::info!("Accelerator server listening on {addr}");

    // ONLY after the bind succeeds. The in-use lease that protects a cached binary from eviction is
    // per-process, and that is sound only because exactly one instance ever evicts. Spawning this
    // before the bind broke that: a second instance would sweep the shared cache during its bind
    // retry — with its own empty registry, so blind to the running instance's leases — before finally
    // losing the port and exiting. This ordering is load-bearing, not stylistic (F-06 follow-up,
    // found by a codex review).
    tokio::spawn(async move {
        crate::versions::sweep_cache_on_start(&bundled).await;
        // F-08a: same slot, same reason. A prove workspace holds the private witness and is deleted
        // only by `TempDir`'s `Drop`, which a crash / Quit mid-proof / auto-update restart skips —
        // so residue accumulates indefinitely. Reaping is safe HERE and nowhere earlier: winning the
        // bind is what proves no other instance (desktop or headless — both reach proving through
        // this same `start`) is mid-proof in that shared directory.
        let reaped = crate::bb::reap_orphaned_prove_workspaces();
        if reaped > 0 {
            tracing::info!(
                count = reaped,
                "reaped abandoned prove workspaces at startup"
            );
        }
    });
    // F-03 sink B: from here until the serving loop ends, THIS process owns `:59833`. That is an
    // in-process fact no other process can forge, unlike the `/health` answer the floor tracker used
    // to rely on alone.
    BIND_OWNED.store(true, Ordering::SeqCst);
    let served = axum::serve(listener, app).await;
    // Cleared on the way out so the flag can never outlive the listener it describes: a stale `true`
    // after `serve` returns would let the floor advance for a build whose server is gone.
    BIND_OWNED.store(false, Ordering::SeqCst);
    served?;
    Ok(())
}

/// Set for exactly as long as this process is serving on [`PORT`] (F-03 sink B, audit
/// 2026-07-31-9c4cb0c).
static BIND_OWNED: AtomicBool = AtomicBool::new(false);

/// Does THIS process currently own the `:59833` listener?
///
/// The anti-rollback floor tracker asked `/health` "did my server come up?" — but any local process
/// can answer that, so the signal proved liveness of *something*, never of *us*. This is the
/// non-forgeable half.
pub fn we_own_the_bind() -> bool {
    BIND_OWNED.load(Ordering::SeqCst)
}

/// Whether the monotonic version floor may be advanced this run (F-03 sink B).
///
/// Both conditions are load-bearing and neither replaces the other:
/// - `we_own_bind` — non-forgeable, but proves only that we BOUND the port, not that we serve well;
/// - `probed == Some(want)` — forgeable on its own, but it is the only evidence that the server
///   actually answers, which is what stops a bound-but-wedged build from ratcheting the floor.
///
/// The audit recommended deleting the probe outright and keeping only an in-process signal. That
/// would trade a refuted security issue for a real availability one, so both are kept and ANDed.
pub fn should_commit_floor(we_own_bind: bool, probed: Option<&str>, want: &str) -> bool {
    we_own_bind && probed == Some(want)
}

/// Build the router for the HTTP listener (port [`PORT`]). Thin shim over [`router_for_port`].
pub fn router(state: AppState) -> Router {
    router_for_port(state, PORT)
}

/// Build the router, gating every request on a trusted loopback `Host`/`:authority` for
/// `expected_port` (SEC-01a — the DNS-rebinding keystone, see [`host`]). Each listener passes its
/// own port (HTTP [`PORT`], HTTPS [`HTTPS_PORT`]) so a `:59834` authority can't pass on the `:59833`
/// listener. The guard is the OUTERMOST layer → it runs before CORS and the routes.
pub fn router_for_port(state: AppState, expected_port: u16) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            http::header::CONTENT_TYPE,
            http::header::HeaderName::from_static("x-aztec-version"),
        ])
        .expose_headers([http::header::HeaderName::from_static("x-prove-duration-ms")]);

    Router::new()
        .route("/health", get(health))
        .route("/prove", post(prove::prove))
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024)) // 50MB — proving payloads can be large
        .layer(cors)
        .layer(SetResponseHeaderLayer::overriding(
            http::header::HeaderName::from_static("cross-origin-resource-policy"),
            HeaderValue::from_static("cross-origin"),
        ))
        // Outermost layer (added last → runs first): reject any non-loopback / wrong-port Host
        // before route or CORS logic. Behaviour-preserving for real loopback clients.
        .layer(axum::middleware::from_fn(move |req, next| {
            host::guard(expected_port, req, next)
        }))
        .with_state(state)
}

/// SEC-05: whether `/health` returns the detailed body (version / cached versions / `bb_available` /
/// `https_port`) or a minimal liveness body. Detailed only for an ABSENT Origin (local non-browser
/// callers: curl / Node / CI) or an APPROVED Origin (`is_approved` covers auto-approved localhost). A
/// present-but-unapproved cross-origin caller gets the minimal body — so a random website learns at
/// most "an accelerator exists", not its version or cached set. After SEC-01a every caller already
/// has a loopback Host, so the Origin (not the Host) is the right discriminant here.
fn health_is_detailed(state: &AppState, headers: &axum::http::HeaderMap) -> bool {
    let Some(raw) = headers
        .get(http::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
    else {
        return true; // no Origin → local, non-browser caller
    };
    let Some(origin) = crate::authorization::CanonicalOrigin::parse(raw) else {
        return false; // malformed Origin → treat as untrusted → minimal
    };
    match state.config.as_ref() {
        // Gated: detailed only for approved origins (incl. auto-approved localhost when enabled).
        Some(cfg) => {
            let cfg = cfg.read();
            AuthorizationManager::is_approved(
                &origin,
                &cfg.approved_origins,
                cfg.auto_approve_localhost,
            )
        }
        // No gating config (headless --allow-all) → no fingerprint concern → serve detailed.
        None => true,
    }
}

async fn health(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // SEC-05: starve cross-site fingerprinting — an unapproved cross-origin probe gets liveness only.
    if !health_is_detailed(&state, &headers) {
        return axum::Json(json!({ "status": "ok", "api_version": 1 }));
    }

    let bundled = state
        .bundled_version
        .as_deref()
        .unwrap_or(DEFAULT_BB_VERSION);

    let mut available = vec![bundled.to_string()];
    for v in versions::list_cached_versions() {
        if v != bundled {
            available.push(v);
        }
    }

    #[allow(unused_mut)]
    let mut body = json!({
        "status": "ok",
        "api_version": 1,
        "version": state.app_version.as_str(),
        "aztec_version": bundled,
        "available_versions": available,
        "bb_available": bb::find_bb(None).is_ok(),
    });

    // Advertise https_port only when the HTTPS listener actually bound (set by start_https after a
    // successful bind), NOT when the config merely requests HTTPS. Keying off the config flag
    // would point the SDK at a dead port on the untrusted-CA startup path (HTTPS skipped, config still
    // on). The shared Arc'd flag also reflects a runtime enable_https without a restart. (Q7)
    if state.https_bound.load(Ordering::Relaxed) {
        body["https_port"] = json!(HTTPS_PORT);
    }

    // Runtime diagnostics only in debug builds — avoid leaking user hardware info in production
    #[cfg(debug_assertions)]
    {
        body["runtime"] = json!({
            "available_parallelism": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        });
    }

    axum::Json(body)
}

/// Typed `/prove` + auth error (q7e3-F-03). Each variant maps to a fixed (status, error-code); the
/// `IntoResponse` impl renders the SAME `text/plain` JSON-string body the prior `(StatusCode,
/// json_error(..))` tuples produced (SDK `ky` keys on `text/plain` — pinned by
/// `prove_error_responses_stay_text_plain_json_string`). The host-guard's `invalid_host` reply is
/// deliberately NOT modeled here — it stays `axum::Json` (`application/json`, no `message`), pinned by
/// `invalid_host_reply_stays_application_json_without_message`.
#[derive(Debug)]
pub(crate) enum ProveError {
    InvalidVersion(String),
    /// codex audit #3: the requested version is well-formed but refused by the downgrade policy
    /// (older than bundled, a dev/unknown-channel build, build metadata, …). Distinct from
    /// `InvalidVersion` (malformed/traversal) so a legitimate integrator sees WHY their pin was denied.
    VersionNotAllowed {
        version: String,
        reason: &'static str,
    },
    PayloadTooLarge(String),
    /// F-009: the request body did not finish arriving within the read timeout while holding
    /// the single prove permit (slowloris / stalled upload). Distinct from PayloadTooLarge.
    BodyReadTimeout,
    ServiceUnavailable,
    /// The cached bb for the requested version is being evicted right now (F-06 lease). Same 503 as
    /// a shutdown — the SDK degrades to WASM on either — but a truthful body, because "shutting down"
    /// would send someone debugging this to entirely the wrong place.
    VersionEvicting,
    DownloadFailed {
        version: String,
        detail: String,
    },
    ProveFailed(String),
    InvalidOrigin,
    OriginDenied(String),
    TooManyRequests,
    /// F-009: the authorized-`/prove` waiter cap (`MAX_INFLIGHT_PROVE`) is full — shed with 429
    /// rather than queueing behind slow uploaders. Distinct from `TooManyRequests` (auth backlog).
    ProveQueueFull,
    AuthorizationTimeout,
    AuthorizationCancelled,
}

impl IntoResponse for ProveError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message): (StatusCode, &str, String) = match self {
            ProveError::InvalidVersion(v) => (
                StatusCode::BAD_REQUEST,
                "invalid_version",
                format!("Invalid x-aztec-version header (got '{v}')"),
            ),
            ProveError::VersionNotAllowed { version, reason } => (
                StatusCode::FORBIDDEN,
                "version_not_allowed",
                format!("Requested bb version '{version}' is not selectable: {reason}"),
            ),
            ProveError::PayloadTooLarge(e) => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                format!("Body too large or unreadable: {e}"),
            ),
            ProveError::BodyReadTimeout => (
                StatusCode::REQUEST_TIMEOUT,
                "body_read_timeout",
                "Timed out while reading request body".to_string(),
            ),
            ProveError::ServiceUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
                "Proving service shutting down".to_string(),
            ),
            ProveError::VersionEvicting => (
                StatusCode::SERVICE_UNAVAILABLE,
                "version_evicting",
                "The cached bb for this version is being evicted; retry".to_string(),
            ),
            ProveError::DownloadFailed { version, detail } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "download_failed",
                format!("Failed to download bb v{version}: {detail}"),
            ),
            ProveError::ProveFailed(e) => (StatusCode::INTERNAL_SERVER_ERROR, "prove_failed", e),
            ProveError::InvalidOrigin => (
                StatusCode::BAD_REQUEST,
                "invalid_origin",
                "Origin header is not a valid RFC 6454 origin".to_string(),
            ),
            ProveError::OriginDenied(origin) => (
                StatusCode::FORBIDDEN,
                "origin_denied",
                format!("Access denied for origin: {origin}"),
            ),
            ProveError::TooManyRequests => (
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_requests",
                "Too many pending authorization requests".to_string(),
            ),
            ProveError::ProveQueueFull => (
                StatusCode::TOO_MANY_REQUESTS,
                "prove_queue_full",
                "Too many concurrent proving requests; retry shortly".to_string(),
            ),
            ProveError::AuthorizationTimeout => (
                StatusCode::FORBIDDEN,
                "authorization_timeout",
                "Authorization request timed out".to_string(),
            ),
            ProveError::AuthorizationCancelled => (
                StatusCode::FORBIDDEN,
                "authorization_cancelled",
                "Authorization request was cancelled".to_string(),
            ),
        };
        (status, json_error(code, &message)).into_response()
    }
}

/// Typed `/prove` error body. Serialized to a JSON string and returned via `(StatusCode, String)`
/// so the Content-Type stays `text/plain` — NOT `axum::Json` (which would flip it to
/// `application/json` and change the SDK's `ky` error parsing). Field order (`error`, `message`)
/// matches the prior `json!` macro, so output is byte-identical. Pinned by
/// `prove_error_responses_stay_text_plain`.
#[derive(serde::Serialize)]
struct ProveErrorBody<'a> {
    error: &'a str,
    message: &'a str,
}

/// Build a consistent JSON error response body for the /prove endpoint.
fn json_error(error: &str, message: &str) -> String {
    serde_json::to_string(&ProveErrorBody { error, message }).unwrap()
}

#[cfg(test)]
mod tests;
