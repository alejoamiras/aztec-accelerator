use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
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
use std::sync::RwLock;

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
}

pub async fn start(state: AppState) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("Accelerator server listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Start an HTTPS listener using the provided TLS config.
/// Runs independently from HTTP — errors are logged but never crash the app.
pub async fn start_https(
    state: AppState,
    tls_config: Arc<tokio_rustls::rustls::ServerConfig>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], HTTPS_PORT));
    let listener = match TcpListener::bind(addr).await {
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
        "version": env!("CARGO_PKG_VERSION"),
        "aztec_version": bundled,
        "available_versions": available,
        "bb_available": bb::find_bb(None).is_ok(),
    });

    if let Some(port) = state.https_port {
        body["https_port"] = json!(port);
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

/// Validate that a version string contains only safe characters for URL interpolation.
/// Allows digits, ASCII letters, dots, hyphens, and underscores.
fn is_valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 128
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

async fn prove(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    tracing::info!("Received /prove request");
    tracing::debug!(payload_bytes = body.len(), "Prove request payload size");

    // --- Origin authorization ---
    // CORS stays permissive (Any) so the SDK can always read responses.
    // Authorization is enforced here: unknown origins get a 403 JSON response.
    if let Some(ref auth_manager) = state.auth_manager {
        let origin = headers
            .get(http::header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        if let Some(origin) = origin {
            let approved = state.config.as_ref().is_some_and(|cfg| {
                let cfg = cfg.read().unwrap();
                AuthorizationManager::is_approved(&origin, &cfg.approved_origins)
            });

            if !approved {
                // If there is no popup callback (headless mode with ALLOWED_ORIGINS),
                // deny immediately instead of hanging for 60s waiting for a popup
                // that will never appear.
                if state.show_auth_popup.is_none() {
                    tracing::info!(origin = %origin, "Origin not approved (no popup available), denying");
                    return Err((
                        StatusCode::FORBIDDEN,
                        serde_json::to_string(&serde_json::json!({
                            "error": "origin_denied",
                            "message": format!("Access denied for origin: {origin}")
                        }))
                        .unwrap(),
                    ));
                }

                tracing::info!(origin = %origin, "Origin not approved, requesting authorization");
                let (rx, is_first) = auth_manager.request(&origin);

                if is_first {
                    if let Some(ref show_popup) = state.show_auth_popup {
                        show_popup(&origin);
                    }
                }

                // Wait for user decision (up to 60s)
                let decision = tokio::time::timeout(Duration::from_secs(60), rx)
                    .await
                    .map_err(|_| {
                        tracing::warn!(origin = %origin, "Authorization timed out");
                        auth_manager.resolve(&origin, AuthDecision::Deny);
                        (
                            StatusCode::FORBIDDEN,
                            serde_json::to_string(&serde_json::json!({
                                "error": "authorization_timeout",
                                "message": "Authorization request timed out"
                            }))
                            .unwrap(),
                        )
                    })?
                    .map_err(|_| {
                        (
                            StatusCode::FORBIDDEN,
                            serde_json::to_string(&serde_json::json!({
                                "error": "authorization_cancelled",
                                "message": "Authorization request was cancelled"
                            }))
                            .unwrap(),
                        )
                    })?;

                match decision {
                    AuthDecision::Allow { remember } => {
                        tracing::info!(origin = %origin, remember, "Origin authorized");
                        if remember {
                            if let Some(ref cfg_lock) = state.config {
                                let mut cfg = cfg_lock.write().unwrap();
                                if !cfg.approved_origins.contains(&origin) {
                                    cfg.approved_origins.push(origin.clone());
                                    let _ = config::save(&cfg);
                                }
                            }
                        }
                    }
                    AuthDecision::Deny => {
                        tracing::info!(origin = %origin, "Origin denied");
                        return Err((
                            StatusCode::FORBIDDEN,
                            serde_json::to_string(&serde_json::json!({
                                "error": "origin_denied",
                                "message": format!("Access denied for origin: {origin}")
                            }))
                            .unwrap(),
                        ));
                    }
                }
            }
        }
        // No Origin header → auto-approve (curl, same-origin, etc.)
    }
    // No auth_manager → auto-approve all (headless mode)

    // Extract requested Aztec version from header (if any)
    let requested_version = headers
        .get("x-aztec-version")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());

    // Validate version string to prevent URL manipulation in download URLs.
    // Only allows semver-like strings: digits, dots, hyphens, and ASCII letters.
    if let Some(ref v) = requested_version {
        if !is_valid_version(v) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "Invalid x-aztec-version header: version must match \
                     ^[0-9a-zA-Z._-]+$ (got '{v}')"
                ),
            ));
        }
        tracing::info!(version = %v, "Requested Aztec version");
    }

    if let Some(ref cb) = state.on_status {
        cb("Status: Proving...");
    }

    // Guard ensures status resets to Idle on any exit path (success, error, drop)
    let _guard = StatusGuard {
        cb: state.on_status.clone(),
    };

    // If a specific version is requested, ensure it's available (download if needed)
    let version_for_prove = if let Some(ref v) = requested_version {
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
                    // Cleanup old versions in the background
                    let bundled_owned = bundled.to_string();
                    let on_versions_changed = state.on_versions_changed.clone();
                    tokio::spawn(async move {
                        versions::cleanup_old_versions(&bundled_owned).await;
                        if let Some(cb) = on_versions_changed {
                            cb();
                        }
                    });
                    if let Some(ref cb) = state.on_versions_changed {
                        cb();
                    }
                }
                Err(e) => {
                    tracing::error!(version = %v, error = %e, "Failed to download bb");
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to download bb v{v}: {e}"),
                    ));
                }
            }

            if let Some(ref cb) = state.on_status {
                cb("Status: Proving...");
            }
        }
        Some(v.as_str())
    } else {
        None
    };

    // Only pass -t flag when speed isn't "full" — bb defaults to all cores already
    let threads = state.config.as_ref().and_then(|cfg| {
        let cfg = cfg.read().unwrap();
        if cfg.speed == "full" {
            None
        } else {
            Some(config::speed_to_threads(&cfg.speed))
        }
    });

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

    let proof = result.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
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
    use tower::util::ServiceExt;

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
        assert_eq!(json["status"], "ok");
        assert!(json.get("version").is_some());
        assert!(json.get("aztec_version").is_some());
        assert!(json.get("bb_available").is_some());
        assert!(json.get("available_versions").is_some());
        assert!(json["available_versions"].is_array());
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

    #[test]
    fn valid_version_strings_accepted() {
        assert!(is_valid_version("5.0.0"));
        assert!(is_valid_version("5.0.0-nightly.20260307"));
        assert!(is_valid_version("5.0.0-rc.1"));
        assert!(is_valid_version("5.0.0-devnet.20260307"));
        assert!(is_valid_version("1.2.3-alpha_beta"));
    }

    #[test]
    fn invalid_version_strings_rejected() {
        assert!(!is_valid_version(""));
        assert!(!is_valid_version("5.0.0; rm -rf /"));
        assert!(!is_valid_version("../../../etc/passwd"));
        assert!(!is_valid_version("5.0.0\n"));
        assert!(!is_valid_version("5.0.0 "));
        assert!(!is_valid_version("v5.0.0/../../malicious"));
        assert!(!is_valid_version(&"a".repeat(129)));
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
}
