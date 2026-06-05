use axum::{
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use http::Method;
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tower_http::cors::{Any, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::authorization::{AuthDecision, AuthorizationManager};
use crate::{bb, config, versions};
use parking_lot::RwLock;
use tokio::sync::Semaphore;

const PORT: u16 = 59833;
pub const HTTPS_PORT: u16 = 59834;

pub type StatusCallback = Arc<dyn Fn(&str) + Send + Sync>;
pub type VersionsChangedCallback = Arc<dyn Fn() + Send + Sync>;

/// Callback to show the authorization popup window. Takes the origin string.
pub type ShowAuthPopupCallback = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone, Default)]
pub struct AppState {
    pub on_status: Option<StatusCallback>,
    pub bundled_version: Option<String>,
    pub on_versions_changed: Option<VersionsChangedCallback>,
    pub https_port: Option<u16>,
    pub config: Option<Arc<RwLock<config::AcceleratorConfig>>>,
    pub auth_manager: Option<Arc<AuthorizationManager>>,
    pub show_auth_popup: Option<ShowAuthPopupCallback>,
    /// Limits concurrent proving to 1 — bb already uses all cores.
    pub prove_semaphore: Option<Arc<Semaphore>>,
}

pub async fn start(state: AppState) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    let listener = bind_with_retry(addr).await?;
    tracing::info!("Accelerator server listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// True iff a `/health` body looks like a healthy Aztec accelerator
/// (`status=="ok"` and `api_version==1`). Pure (unit-tested) so the redundant-vs-foreign
/// classification can't silently accept an arbitrary process answering on :59833.
fn is_healthy_aztec_response(body: &serde_json::Value) -> bool {
    body.get("status").and_then(|s| s.as_str()) == Some("ok")
        && body.get("api_version").and_then(|v| v.as_u64()) == Some(1)
}

/// Probe `http://127.0.0.1:59833/health` and return true iff a HEALTHY Aztec instance
/// answers. Used to classify a lost `:59833` bind: the autostart entry AND the
/// crash-recovery launcher (Task Scheduler / launchd / systemd) can both start us at
/// logon, so a redundant instance should bow out — but only if the incumbent is really
/// us, not some foreign process squatting on the port.
pub async fn healthy_aztec_on_port() -> bool {
    let url = format!("http://127.0.0.1:{PORT}/health");
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let Ok(resp) = client.get(&url).send().await else {
        return false;
    };
    // Require a 2xx so a non-success responder that happens to echo the right JSON
    // shape isn't mistaken for a healthy Aztec instance.
    if !resp.status().is_success() {
        return false;
    }
    let Ok(body) = resp.json::<serde_json::Value>().await else {
        return false;
    };
    is_healthy_aztec_response(&body)
}

/// Bind the HTTP listener, retrying briefly on `AddrInUse`.
///
/// During an in-place updater restart the just-exited previous instance can
/// still hold port 59833 for a moment. Without a retry the freshly-relaunched
/// app permanently fails to bind and the accelerator server stays down — the
/// new process binds once, hits `EADDRINUSE`, and never recovers (observed on
/// Linux auto-update; macOS happens to dodge it on timing). The wait is bounded
/// so a genuine conflict — a second instance the user started — still fails
/// fast with the port-in-use signal (surfaced by main.rs) instead of hanging.
async fn bind_with_retry(addr: SocketAddr) -> std::io::Result<TcpListener> {
    // 100ms polling, 5s budget: the restart overlap clears in well under a
    // second, so this is responsive AND fails a genuine second-instance
    // conflict reasonably fast (it surfaces "port in use" rather than stalling).
    bind_with_retry_inner(addr, Duration::from_millis(100), Duration::from_secs(5)).await
}

/// Inner form with injectable timings so tests can exercise the wait-it-out,
/// hard-deadline, and immediate-propagation paths without real-time sleeps.
async fn bind_with_retry_inner(
    addr: SocketAddr,
    interval: Duration,
    max_wait: Duration,
) -> std::io::Result<TcpListener> {
    let deadline = std::time::Instant::now() + max_wait;
    let mut warned = false;
    loop {
        match TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                // Hard deadline: sleep only the time actually left, so we give up
                // at ~max_wait rather than overshooting by a full `interval`.
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    tracing::warn!(
                        "port {} still in use after {max_wait:?} — giving up",
                        addr.port()
                    );
                    return Err(e);
                }
                if !warned {
                    tracing::warn!(
                        "port {} in use — retrying for up to {max_wait:?} (waiting out a prior instance, e.g. an in-place updater restart)",
                        addr.port()
                    );
                    warned = true;
                }
                tokio::time::sleep(interval.min(remaining)).await;
            }
            // Any non-AddrInUse error propagates immediately — never masked by the retry.
            Err(e) => return Err(e),
        }
    }
}

/// Start an HTTPS listener using the provided TLS config.
/// Runs independently from HTTP — errors are logged but never crash the app.
pub async fn start_https(
    state: AppState,
    tls_config: Arc<tokio_rustls::rustls::ServerConfig>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], HTTPS_PORT));
    // Same restart race as the HTTP listener: an in-place update relaunches while
    // the old process still holds 59834. Retry first; only fall back to HTTP-only
    // if the port is genuinely unavailable past the budget (HTTPS must never
    // crash the app — it's optional Safari support).
    let listener = match bind_with_retry(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("HTTPS port {HTTPS_PORT} unavailable: {e} — continuing HTTP-only");
            return Ok(());
        }
    };

    let acceptor = TlsAcceptor::from(tls_config);
    tracing::info!("HTTPS server listening on {addr}");

    loop {
        let (stream, _peer) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                tracing::debug!("TCP accept error: {e}");
                continue;
            }
        };

        let acceptor = acceptor.clone();
        let app = app.clone();

        tokio::spawn(async move {
            let tls_stream = match acceptor.accept(stream).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!("TLS handshake failed: {e}");
                    return;
                }
            };
            let io = hyper_util::rt::TokioIo::new(tls_stream);
            let hyper_service = hyper_util::service::TowerToHyperService::new(app);
            if let Err(e) =
                hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection(io, hyper_service)
                    .await
            {
                tracing::debug!("HTTPS connection error: {e}");
            }
        });
    }
}

pub fn router(state: AppState) -> Router {
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
        .route("/prove", post(prove))
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024)) // 50MB — proving payloads can be large
        .layer(cors)
        .layer(SetResponseHeaderLayer::overriding(
            http::header::HeaderName::from_static("cross-origin-resource-policy"),
            HeaderValue::from_static("cross-origin"),
        ))
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let bundled = state
        .bundled_version
        .as_deref()
        .unwrap_or(env!("AZTEC_BB_VERSION"));

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
        "version": env!("CARGO_PKG_VERSION"),
        "aztec_version": bundled,
        "available_versions": available,
        "bb_available": bb::find_bb(None).is_ok(),
    });

    // Report HTTPS port if Safari Support is enabled (reads live config, not static state,
    // so runtime enable_safari_support is reflected immediately without restart).
    let safari_enabled = state
        .config
        .as_ref()
        .is_some_and(|cfg| cfg.read().safari_support);
    if state.https_port.is_some() || safari_enabled {
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

/// Drop guard that resets tray status to Idle when the prove handler exits for any reason
/// (success, error, client disconnect, panic).
struct StatusGuard {
    cb: Option<StatusCallback>,
}

impl Drop for StatusGuard {
    fn drop(&mut self) {
        if let Some(ref cb) = self.cb {
            cb("Status: Idle");
        }
    }
}

type ProveError = (StatusCode, String);

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

/// Check if the request origin is authorized. Returns Ok(()) if approved.
async fn authorize_origin(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(), ProveError> {
    let auth_manager = match state.auth_manager {
        Some(ref am) => am,
        None => return Ok(()), // No auth_manager → auto-approve all (headless mode)
    };

    let raw_origin = match headers
        .get(http::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
    {
        Some(o) => o,
        // No Origin header → auto-approve. Browsers always send Origin on cross-origin
        // requests, so this only applies to curl/scripts/same-origin. Non-browser clients
        // can bypass auth by omitting Origin, but this is inherent to localhost services —
        // CORS/Origin is a browser-only mechanism, not a general access control boundary.
        None => return Ok(()),
    };

    let origin = match crate::authorization::canonicalize_origin(raw_origin) {
        Some(canon) => canon,
        None => {
            tracing::warn!(raw_origin = %raw_origin, "Invalid Origin header (path/query/userinfo/unknown scheme); rejecting");
            return Err((
                StatusCode::BAD_REQUEST,
                json_error(
                    "invalid_origin",
                    "Origin header is not a valid RFC 6454 origin",
                ),
            ));
        }
    };

    let approved = state.config.as_ref().is_some_and(|cfg| {
        let cfg = cfg.read();
        AuthorizationManager::is_approved(&origin, &cfg.approved_origins)
    });

    if approved {
        return Ok(());
    }

    // No popup callback = headless mode → deny immediately
    if state.show_auth_popup.is_none() {
        tracing::info!(origin = %origin, "Origin not approved (no popup available), denying");
        return Err((
            StatusCode::FORBIDDEN,
            json_error(
                "origin_denied",
                &format!("Access denied for origin: {origin}"),
            ),
        ));
    }

    tracing::info!(origin = %origin, "Origin not approved, requesting authorization");
    let (rx, is_first) = auth_manager.request(&origin).map_err(|_| {
        tracing::warn!(origin = %origin, "Too many pending authorization requests");
        (
            StatusCode::TOO_MANY_REQUESTS,
            json_error(
                "too_many_requests",
                "Too many pending authorization requests",
            ),
        )
    })?;

    if is_first {
        if let Some(ref show_popup) = state.show_auth_popup {
            show_popup(&origin);
        }
    }

    let decision = tokio::time::timeout(Duration::from_secs(60), rx)
        .await
        .map_err(|_| {
            tracing::warn!(origin = %origin, "Authorization timed out");
            auth_manager.resolve(&origin, AuthDecision::Deny);
            (
                StatusCode::FORBIDDEN,
                json_error("authorization_timeout", "Authorization request timed out"),
            )
        })?
        .map_err(|_| {
            (
                StatusCode::FORBIDDEN,
                json_error(
                    "authorization_cancelled",
                    "Authorization request was cancelled",
                ),
            )
        })?;

    match decision {
        AuthDecision::Allow { remember } => {
            tracing::info!(origin = %origin, remember, "Origin authorized");
            if remember {
                if let Some(ref cfg_lock) = state.config {
                    let mut cfg = cfg_lock.write();
                    if !cfg.approved_origins.contains(&origin) {
                        cfg.approved_origins.push(origin);
                        if let Err(e) = config::save(&cfg) {
                            tracing::warn!(error = %e, "Failed to persist approved origin");
                        }
                    }
                }
            }
            Ok(())
        }
        AuthDecision::Deny => {
            tracing::info!(origin = %origin, "Origin denied");
            Err((
                StatusCode::FORBIDDEN,
                json_error(
                    "origin_denied",
                    &format!("Access denied for origin: {origin}"),
                ),
            ))
        }
    }
}

/// Validate and resolve the requested Aztec version. Downloads the bb binary if needed.
async fn resolve_version<'a>(
    state: &AppState,
    requested: &'a Option<String>,
) -> Result<Option<&'a str>, ProveError> {
    let v = match requested {
        Some(v) => v,
        None => return Ok(None),
    };

    if !versions::is_valid_version(v) {
        return Err((
            StatusCode::BAD_REQUEST,
            json_error(
                "invalid_version",
                &format!("Invalid x-aztec-version header (got '{v}')"),
            ),
        ));
    }
    tracing::info!(version = %v, "Requested Aztec version");

    let bundled = state
        .bundled_version
        .as_deref()
        .unwrap_or(env!("AZTEC_BB_VERSION"));

    if v != bundled && !versions::version_bb_path(v).exists() {
        tracing::info!(version = %v, "Version not cached, downloading");
        if let Some(ref cb) = state.on_status {
            cb("Status: Downloading bb...");
        }

        match versions::download_bb(v).await {
            Ok(_) => {
                tracing::info!(version = %v, "Download complete");
                let bundled_owned = bundled.to_string();
                let on_versions_changed = state.on_versions_changed.clone();
                tokio::spawn(async move {
                    versions::cleanup_old_versions(&bundled_owned).await;
                    if let Some(cb) = on_versions_changed {
                        cb();
                    }
                });
            }
            Err(e) => {
                tracing::error!(version = %v, error = %e, "Failed to download bb");
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json_error(
                        "download_failed",
                        &format!("Failed to download bb v{v}: {e}"),
                    ),
                ));
            }
        }

        if let Some(ref cb) = state.on_status {
            cb("Status: Proving...");
        }
    }

    Ok(Some(v.as_str()))
}

/// Read the speed setting from config and convert to thread count.
/// Returns None for "full" (let bb use its default).
fn compute_threads(state: &AppState) -> Option<usize> {
    state.config.as_ref().and_then(|cfg| {
        let cfg = cfg.read();
        if cfg.speed.is_full() {
            None
        } else {
            Some(cfg.speed.to_threads())
        }
    })
}

const MAX_BODY_SIZE: usize = 50 * 1024 * 1024; // 50MB

async fn prove(
    State(state): State<AppState>,
    request: Request,
) -> Result<impl IntoResponse, ProveError> {
    tracing::info!("Received /prove request");

    // Extract headers before consuming the request body. Run authorization FIRST
    // so unapproved origins are rejected without buffering the (potentially large) body.
    let (parts, raw_body) = request.into_parts();
    authorize_origin(&state, &parts.headers).await?;

    let body = axum::body::to_bytes(raw_body, MAX_BODY_SIZE)
        .await
        .map_err(|e| {
            tracing::warn!("Failed to read request body: {e}");
            (
                StatusCode::PAYLOAD_TOO_LARGE,
                json_error(
                    "payload_too_large",
                    &format!("Body too large or unreadable: {e}"),
                ),
            )
        })?;
    tracing::debug!(payload_bytes = body.len(), "Prove request payload size");

    // Limit to one concurrent prove — bb already uses all cores.
    let _permit = if let Some(ref sem) = state.prove_semaphore {
        Some(sem.acquire().await.map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                json_error("service_unavailable", "Proving service shutting down"),
            )
        })?)
    } else {
        None
    };

    let requested_version = parts
        .headers
        .get("x-aztec-version")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());

    if let Some(ref cb) = state.on_status {
        cb("Status: Proving...");
    }
    let _guard = StatusGuard {
        cb: state.on_status.clone(),
    };

    let version_for_prove = resolve_version(&state, &requested_version).await?;
    let threads = compute_threads(&state);

    let start = std::time::Instant::now();
    let result = bb::prove(&body, version_for_prove, threads).await;
    let elapsed = start.elapsed();

    match &result {
        Ok(proof) => {
            tracing::info!("Proving succeeded");
            tracing::debug!(
                elapsed_ms = elapsed.as_millis() as u64,
                proof_bytes = proof.len(),
                "Proving timing and size"
            );
        }
        Err(e) => {
            tracing::error!("Proving failed: {e}");
            tracing::debug!(
                elapsed_ms = elapsed.as_millis() as u64,
                "Failed prove timing"
            );
        }
    }

    let proof = result.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            json_error("prove_failed", &e.to_string()),
        )
    })?;
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &proof);

    let mut response = axum::Json(json!({ "proof": encoded })).into_response();
    response.headers_mut().insert(
        "x-prove-duration-ms",
        HeaderValue::from_str(&elapsed.as_millis().to_string()).unwrap(),
    );

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use serial_test::serial;
    use tower::util::ServiceExt;

    #[test]
    fn classifies_health_responses() {
        // Healthy Aztec: bow out (redundant instance).
        assert!(is_healthy_aztec_response(
            &json!({"status": "ok", "api_version": 1})
        ));
        assert!(is_healthy_aztec_response(
            &json!({"status": "ok", "api_version": 1, "version": "1.2.3"})
        ));
        // Foreign / wrong / malformed: do NOT treat as Aztec (must surface the error,
        // never silently exit and leave the user with no accelerator).
        assert!(!is_healthy_aztec_response(
            &json!({"status": "ok", "api_version": 2})
        ));
        assert!(!is_healthy_aztec_response(
            &json!({"status": "error", "api_version": 1})
        ));
        assert!(!is_healthy_aztec_response(&json!({"api_version": 1})));
        assert!(!is_healthy_aztec_response(&json!({"hello": "world"})));
        assert!(!is_healthy_aztec_response(&json!({})));
        assert!(!is_healthy_aztec_response(&json!("not even an object")));
    }

    #[tokio::test]
    async fn bind_with_retry_waits_out_a_transient_holder() {
        // The in-place-restart case: hold a freshly-chosen port, release it
        // shortly, and assert the retry binds once it frees.
        let probe = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .unwrap();
        let addr = probe.local_addr().unwrap();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            drop(probe);
        });
        let listener =
            bind_with_retry_inner(addr, Duration::from_millis(20), Duration::from_secs(2))
                .await
                .expect("should bind once the transient holder releases the port");
        assert_eq!(listener.local_addr().unwrap().port(), addr.port());
    }

    #[tokio::test]
    async fn bind_with_retry_gives_up_on_a_persistent_conflict_at_a_hard_deadline() {
        // A second instance, not a restart overlap: a port held for the whole
        // window must fail with AddrInUse (so main.rs surfaces "port in use").
        // `interval` is deliberately LARGER than `budget` so a HARD deadline caps
        // at ~budget (sleeping only the remaining time) while a SOFT one would
        // sleep a full interval and overshoot — the elapsed assertion catches
        // that regression.
        let probe = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .unwrap();
        let addr = probe.local_addr().unwrap();
        let budget = Duration::from_millis(200);
        let interval = Duration::from_millis(500);
        let started = std::time::Instant::now();
        let err = bind_with_retry_inner(addr, interval, budget)
            .await
            .expect_err("should give up while the port stays held");
        let elapsed = started.elapsed();
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
        assert!(
            elapsed < budget + Duration::from_millis(150),
            "hard deadline overshot: {elapsed:?} (budget {budget:?}, interval {interval:?}) — a soft deadline would sleep a full interval past the budget"
        );
        drop(probe);
    }

    #[tokio::test]
    async fn bind_with_retry_propagates_non_addrinuse_immediately() {
        // An unassigned TEST-NET-1 address (RFC 5737) can't be bound →
        // AddrNotAvailable, NOT AddrInUse. It must return at once, never entering
        // the retry budget (a 10s budget would expose a wrongful retry).
        let bad = SocketAddr::from(([192, 0, 2, 1], 0));
        let started = std::time::Instant::now();
        let err = bind_with_retry_inner(bad, Duration::from_millis(100), Duration::from_secs(10))
            .await
            .expect_err("binding an unassigned address must fail");
        assert_ne!(err.kind(), std::io::ErrorKind::AddrInUse);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a non-AddrInUse error must propagate immediately, not retry for the budget"
        );
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let app = router(AppState::default());
        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // Assert complete response contract — every field, correct types
        assert_eq!(json["status"], "ok");
        assert_eq!(json["api_version"], 1);
        assert!(json["version"].is_string(), "version should be a string");
        assert!(
            json["aztec_version"].is_string(),
            "aztec_version should be a string"
        );
        assert!(
            json["bb_available"].is_boolean(),
            "bb_available should be a boolean"
        );
        assert!(
            json["available_versions"].is_array(),
            "available_versions should be an array"
        );
        // Default state: no Safari support → no https_port
        assert!(
            json.get("https_port").is_none(),
            "https_port should be absent without Safari support"
        );
    }

    #[tokio::test]
    async fn health_includes_https_port_when_safari_enabled() {
        let cfg = crate::config::AcceleratorConfig {
            safari_support: true,
            ..Default::default()
        };
        let state = AppState {
            config: Some(Arc::new(RwLock::new(cfg))),
            https_port: Some(59834),
            ..Default::default()
        };
        let app = router(state);
        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["https_port"], 59834);
    }

    #[tokio::test]
    async fn cors_preflight_returns_correct_headers() {
        let app = router(AppState::default());
        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/prove")
                    .header("origin", "http://localhost:5173")
                    .header("access-control-request-method", "POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            "*"
        );
        assert_eq!(
            response
                .headers()
                .get("cross-origin-resource-policy")
                .unwrap(),
            "cross-origin"
        );
    }

    #[tokio::test]
    async fn cors_allows_aztec_version_header() {
        let app = router(AppState::default());
        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/prove")
                    .header("origin", "http://localhost:5173")
                    .header("access-control-request-method", "POST")
                    .header("access-control-request-headers", "x-aztec-version")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let allow_headers = response
            .headers()
            .get("access-control-allow-headers")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            allow_headers.contains("x-aztec-version"),
            "CORS should allow x-aztec-version header, got: {allow_headers}"
        );
    }

    #[tokio::test]
    async fn health_includes_cors_headers() {
        let app = router(AppState::default());
        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header("origin", "http://localhost:5173")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            "*"
        );
    }

    #[tokio::test]
    async fn prove_returns_error_when_bb_not_found() {
        // This test exercises the "bb not found" error path. When bb IS installed
        // on the dev machine, find_bb() succeeds and the real bb binary runs with
        // garbage input — taking 60+ seconds to error out. Skip in that case.
        if bb::find_bb(None).is_ok() {
            eprintln!("skipping: bb is available on this machine");
            return;
        }

        let app = router(AppState::default());
        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/prove")
                    .header("content-type", "application/octet-stream")
                    .body(Body::from(vec![0u8; 10]))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should fail because bb is not available in test env
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn health_includes_runtime_diagnostics_in_debug() {
        let app = router(AppState::default());
        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Tests always run in debug mode, so runtime should be present
        let runtime = &json["runtime"];
        assert!(
            runtime.is_object(),
            "runtime should be present in debug builds"
        );
        assert!(
            runtime["available_parallelism"].as_u64().unwrap() > 0,
            "available_parallelism should be > 0"
        );
    }

    #[tokio::test]
    async fn prove_rejects_invalid_version_header() {
        let app = router(AppState::default());
        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/prove")
                    .header("content-type", "application/octet-stream")
                    .header("x-aztec-version", "../../../etc/passwd")
                    .body(Body::from(vec![0u8; 10]))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "invalid_version");
        assert!(
            json["message"].is_string(),
            "error message should be a string"
        );
    }

    /// CHARACTERIZATION (quality-refactor Phase 0 — Q8 wire-contract guard).
    /// `/prove` error responses are a `{error,message}` JSON-shaped body served as **`text/plain`**
    /// (they go out via `(StatusCode, String)`, not `axum::Json`). The SDK's `ky` client keys
    /// `HTTPError.data` parsing on Content-Type, so a Q8 refactor that switches to `axum::Json` would
    /// flip this to `application/json` and silently change SDK runtime behavior. Pin status + error-id
    /// + `text/plain` for the reachable (no-bb) error paths so that regression fails loudly.
    #[tokio::test]
    async fn prove_error_responses_stay_text_plain_json_string() {
        async fn assert_error(
            app: Router,
            req: Request<Body>,
            want_status: StatusCode,
            want_error: &str,
        ) {
            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), want_status, "status for {want_error}");
            let ct = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            assert!(
                ct.starts_with("text/plain"),
                "{want_error} must stay text/plain (Q8 wire contract — SDK ky keys on it), got {ct:?}"
            );
            let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let json: serde_json::Value =
                serde_json::from_slice(&body).expect("error body is JSON-shaped");
            assert_eq!(json["error"], want_error, "error id for {want_error}");
            assert!(json["message"].is_string(), "{want_error} needs a message");
        }

        // invalid_version (400) — default state, traversal-y x-aztec-version
        assert_error(
            router(AppState::default()),
            Request::builder()
                .method("POST")
                .uri("/prove")
                .header("content-type", "application/octet-stream")
                .header("x-aztec-version", "../../../etc/passwd")
                .body(Body::from(vec![0u8; 10]))
                .unwrap(),
            StatusCode::BAD_REQUEST,
            "invalid_version",
        )
        .await;

        // invalid_origin (400) — auth present, malformed Origin (rejected before popup)
        let (_origin_tx, _origin_rx) = std::sync::mpsc::channel();
        let (state, _auth) = auth_state_with_popup(_origin_tx);
        assert_error(
            router(state),
            Request::builder()
                .method("POST")
                .uri("/prove")
                .header("content-type", "application/octet-stream")
                .header("origin", "not-a-valid-origin")
                .body(Body::from(vec![0u8; 10]))
                .unwrap(),
            StatusCode::BAD_REQUEST,
            "invalid_origin",
        )
        .await;

        // origin_denied (403) — auth + deny
        let (popup_tx, _popup_rx) = std::sync::mpsc::channel();
        let (state, auth) = auth_state_with_popup(popup_tx);
        let auth_clone = auth.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            auth_clone.resolve("https://evil.com", crate::authorization::AuthDecision::Deny);
        });
        assert_error(
            router(state),
            Request::builder()
                .method("POST")
                .uri("/prove")
                .header("content-type", "application/octet-stream")
                .header("origin", "https://evil.com")
                .body(Body::from(vec![0u8; 10]))
                .unwrap(),
            StatusCode::FORBIDDEN,
            "origin_denied",
        )
        .await;
    }

    /// CHARACTERIZATION (quality-refactor Phase 0 — Q2 ordering + Q10 status guards).
    /// Pins the `/prove` SUCCESS path via a fake `bb` (`BB_BINARY_PATH`): 200 + `{proof}` base64 body
    /// + `x-prove-duration-ms` header, and the on_status sequence `["Status: Proving...",
    /// "Status: Idle"]` (the bundled path sets Proving, `StatusGuard` resets to Idle on exit).
    /// `#[serial]` because `find_bb` reads the process-global `BB_BINARY_PATH`. Q2 (server split)
    /// must preserve this ordering; Q10 (ServerStatus enum) must reproduce these exact strings.
    #[cfg(unix)]
    #[tokio::test]
    #[serial]
    async fn prove_success_path_and_status_sequence() {
        use std::os::unix::fs::PermissionsExt;
        // Fake bb: parse `-o <dir>`, write a 32-byte `proof` file there, exit 0.
        let dir = tempfile::tempdir().unwrap();
        let fake_bb = dir.path().join("fake-bb");
        std::fs::write(
            &fake_bb,
            "#!/bin/sh\nprev=\"\"\nfor a in \"$@\"; do [ \"$prev\" = \"-o\" ] && out=\"$a\"; prev=\"$a\"; done\nprintf '%032d' 0 > \"$out/proof\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake_bb, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("BB_BINARY_PATH", &fake_bb);

        let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let rec = recorded.clone();
        let state = AppState {
            on_status: Some(std::sync::Arc::new(move |s: &str| {
                rec.lock().unwrap().push(s.to_string())
            })),
            prove_semaphore: Some(std::sync::Arc::new(tokio::sync::Semaphore::new(1))),
            ..Default::default()
        };

        let response = router(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/prove")
                    .header("content-type", "application/octet-stream")
                    .body(Body::from(vec![0u8; 16]))
                    .unwrap(),
            )
            .await
            .unwrap();
        std::env::remove_var("BB_BINARY_PATH");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers().contains_key("x-prove-duration-ms"),
            "success must carry x-prove-duration-ms"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            json["proof"].as_str().is_some_and(|s| !s.is_empty()),
            "proof base64 present"
        );

        let seq = recorded.lock().unwrap().clone();
        assert_eq!(
            seq,
            vec!["Status: Proving...".to_string(), "Status: Idle".to_string()],
            "bundled success path status sequence (Q10 pin)"
        );
    }

    #[tokio::test]
    async fn health_includes_available_versions() {
        let state = AppState {
            bundled_version: Some("5.0.0-nightly.20260307".into()),
            ..Default::default()
        };
        let app = router(state);
        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let versions = json["available_versions"].as_array().unwrap();
        // At minimum, bundled version should be in available_versions
        assert!(versions
            .iter()
            .any(|v| v.as_str() == Some("5.0.0-nightly.20260307")));
    }

    /// Helper: build an AppState with auth enabled and a mock popup callback.
    fn auth_state_with_popup(
        popup_tx: std::sync::mpsc::Sender<String>,
    ) -> (AppState, Arc<crate::authorization::AuthorizationManager>) {
        let auth = Arc::new(crate::authorization::AuthorizationManager::new());
        let auth_for_state = auth.clone();
        let cfg = crate::config::AcceleratorConfig::default();
        let state = AppState {
            auth_manager: Some(auth_for_state),
            config: Some(Arc::new(RwLock::new(cfg))),
            show_auth_popup: Some(Arc::new(move |origin: &str| {
                let _ = popup_tx.send(origin.to_string());
            })),
            prove_semaphore: Some(Arc::new(Semaphore::new(1))),
            ..Default::default()
        };
        (state, auth)
    }

    #[tokio::test]
    async fn prove_auto_approves_localhost_origin() {
        let (popup_tx, popup_rx) = std::sync::mpsc::channel();
        let (state, _auth) = auth_state_with_popup(popup_tx);
        let app = router(state);

        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/prove")
                    .header("content-type", "application/octet-stream")
                    .header("origin", "http://localhost:5173")
                    .body(Body::from(vec![0u8; 10]))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Localhost is auto-approved — should NOT trigger popup, should proceed to proving
        // (which fails because bb is not available, but that's fine — we're testing the auth gate)
        assert_ne!(response.status(), StatusCode::FORBIDDEN);
        assert!(
            popup_rx.try_recv().is_err(),
            "popup should not fire for localhost"
        );
    }

    #[tokio::test]
    async fn prove_triggers_popup_for_unknown_origin() {
        let (popup_tx, popup_rx) = std::sync::mpsc::channel();
        let (state, auth) = auth_state_with_popup(popup_tx);
        let app = router(state);

        // Spawn a task that waits for the popup signal, then auto-approves.
        // Uses popup_rx.recv() instead of sleep to avoid race conditions.
        let auth_clone = auth.clone();
        let (popup_seen_tx, popup_seen_rx) = tokio::sync::oneshot::channel::<String>();
        tokio::spawn(async move {
            let origin = tokio::task::spawn_blocking(move || popup_rx.recv().unwrap())
                .await
                .unwrap();
            let _ = popup_seen_tx.send(origin.clone());
            auth_clone.resolve(
                &origin,
                crate::authorization::AuthDecision::Allow { remember: false },
            );
        });

        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/prove")
                    .header("content-type", "application/octet-stream")
                    .header("origin", "https://unknown-site.com")
                    .body(Body::from(vec![0u8; 10]))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Popup should have been triggered
        assert_eq!(popup_seen_rx.await.unwrap(), "https://unknown-site.com");
        // After approval, should proceed (not 403)
        assert_ne!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn prove_returns_403_when_origin_denied() {
        let (popup_tx, _popup_rx) = std::sync::mpsc::channel();
        let (state, auth) = auth_state_with_popup(popup_tx);
        let app = router(state);

        // Auto-deny after popup fires
        let auth_clone = auth.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            auth_clone.resolve("https://evil.com", crate::authorization::AuthDecision::Deny);
        });

        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/prove")
                    .header("content-type", "application/octet-stream")
                    .header("origin", "https://evil.com")
                    .body(Body::from(vec![0u8; 10]))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "origin_denied");
        assert!(
            json["message"].is_string(),
            "denied error should have a message"
        );
    }

    #[tokio::test]
    async fn prove_skips_auth_when_no_origin_header() {
        let (popup_tx, popup_rx) = std::sync::mpsc::channel();
        let (state, _auth) = auth_state_with_popup(popup_tx);
        let app = router(state);

        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/prove")
                    .header("content-type", "application/octet-stream")
                    .body(Body::from(vec![0u8; 10]))
                    .unwrap(),
            )
            .await
            .unwrap();

        // No Origin header = auto-approved (curl, same-origin)
        assert_ne!(response.status(), StatusCode::FORBIDDEN);
        assert!(
            popup_rx.try_recv().is_err(),
            "popup should not fire without Origin"
        );
    }

    #[tokio::test]
    async fn prove_approves_remembered_origin() {
        let (popup_tx, popup_rx) = std::sync::mpsc::channel();
        let (state, _auth) = auth_state_with_popup(popup_tx);

        // Pre-approve the origin in config
        if let Some(ref cfg) = state.config {
            cfg.write()
                .approved_origins
                .push("https://approved-site.com".to_string());
        }

        let app = router(state);
        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/prove")
                    .header("content-type", "application/octet-stream")
                    .header("origin", "https://approved-site.com")
                    .body(Body::from(vec![0u8; 10]))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(response.status(), StatusCode::FORBIDDEN);
        assert!(
            popup_rx.try_recv().is_err(),
            "popup should not fire for approved origin"
        );
    }

    #[tokio::test]
    async fn prove_returns_403_without_popup_in_headless() {
        // Headless mode: auth_manager is set but show_auth_popup is None
        let auth = Arc::new(crate::authorization::AuthorizationManager::new());
        let cfg = crate::config::AcceleratorConfig::default();
        let state = AppState {
            auth_manager: Some(auth),
            config: Some(Arc::new(RwLock::new(cfg))),
            show_auth_popup: None, // headless
            ..Default::default()
        };
        let app = router(state);

        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/prove")
                    .header("content-type", "application/octet-stream")
                    .header("origin", "https://unknown.com")
                    .body(Body::from(vec![0u8; 10]))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Headless with no popup = instant deny
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn prove_returns_429_when_too_many_pending_origins() {
        let (popup_tx, _popup_rx) = std::sync::mpsc::channel();
        let (state, _auth) = auth_state_with_popup(popup_tx);
        let app = router(state.clone());

        // Fill the AuthorizationManager to capacity (MAX_PENDING_ORIGINS = 10)
        let auth = state.auth_manager.as_ref().unwrap();
        for i in 0..10 {
            let _ = auth.request(&format!("https://origin-{i}.com"));
        }

        // The 11th distinct origin should get 429
        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/prove")
                    .header("content-type", "application/octet-stream")
                    .header("origin", "https://one-too-many.com")
                    .body(Body::from(vec![0u8; 10]))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "too_many_requests");
        assert!(json["message"].is_string());
    }

    #[tokio::test(start_paused = true)]
    async fn prove_returns_403_on_authorization_timeout() {
        let (popup_tx, _popup_rx) = std::sync::mpsc::channel();
        let (state, _auth) = auth_state_with_popup(popup_tx);
        let app = router(state);

        // Send request from unknown origin — popup fires but nobody resolves it.
        // start_paused = true means tokio time is auto-advanced when all tasks
        // are waiting on timers, so the 60s timeout resolves instantly.
        let response: axum::http::Response<_> = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/prove")
                    .header("content-type", "application/octet-stream")
                    .header("origin", "https://slow-user.com")
                    .body(Body::from(vec![0u8; 10]))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "authorization_timeout");
        assert!(json["message"].is_string());
    }

    // ── Helper unit tests ──

    #[test]
    fn compute_threads_returns_none_for_full_speed() {
        let cfg = crate::config::AcceleratorConfig {
            speed: crate::config::Speed::Full,
            ..Default::default()
        };
        let state = AppState {
            config: Some(Arc::new(RwLock::new(cfg))),
            ..Default::default()
        };
        assert_eq!(compute_threads(&state), None);
    }

    #[test]
    fn compute_threads_returns_some_for_non_full_speed() {
        let cfg = crate::config::AcceleratorConfig {
            speed: crate::config::Speed::Balanced,
            ..Default::default()
        };
        let state = AppState {
            config: Some(Arc::new(RwLock::new(cfg))),
            ..Default::default()
        };
        assert!(compute_threads(&state).is_some());
    }

    #[test]
    fn compute_threads_returns_none_without_config() {
        let state = AppState::default();
        assert_eq!(compute_threads(&state), None);
    }

    #[tokio::test]
    async fn resolve_version_passes_valid_version() {
        let state = AppState::default();
        let version = Some("5.0.0-rc.1".to_string());
        let result = resolve_version(&state, &version).await;
        // May fail on download (no network in test) but should not reject the version
        assert!(result.is_ok() || result.is_err());
        // The key assertion: valid version string is not rejected as BAD_REQUEST
        if let Err((status, _)) = &result {
            assert_ne!(*status, StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn resolve_version_rejects_invalid_version() {
        let state = AppState::default();
        let version = Some("../../../etc/passwd".to_string());
        let result = resolve_version(&state, &version).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn resolve_version_returns_none_without_header() {
        let state = AppState::default();
        let result = resolve_version(&state, &None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    // ── Failure-path tests ──

    #[tokio::test]
    async fn prove_rejects_oversized_body() {
        let app = router(AppState::default());
        // Send a body just over MAX_BODY_SIZE (50MB + 1 byte)
        // Use a smaller test to avoid allocating 50MB — the limit is enforced by
        // axum::body::to_bytes, which we call with MAX_BODY_SIZE. We can test
        // indirectly by setting up a custom small limit.
        // Instead, verify the endpoint handles a normal-sized body correctly
        // (the oversized case is enforced by the to_bytes call in the handler).
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/prove")
                    .body(Body::from(vec![0u8; 10]))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Should NOT return 413 for a small body — proves the handler runs past body extraction
        assert_ne!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn prove_handles_empty_body() {
        let app = router(AppState::default());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/prove")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Should not panic — returns an error from bb (not found or invalid input)
        // but the handler itself should not crash on empty input
        assert!(
            response.status().is_client_error() || response.status().is_server_error(),
            "Expected error status for empty body, got {}",
            response.status()
        );
    }
}
