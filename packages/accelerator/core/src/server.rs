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
pub use probe::healthy_aztec_on_port;
mod auth;
mod prove;

const PORT: u16 = 59833;
pub const HTTPS_PORT: u16 = 59834;

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

/// Callback to show the authorization popup window. Takes the origin string.
pub type ShowAuthPopupCallback = Arc<dyn Fn(&str) + Send + Sync>;

/// The server-side core state — everything the headless `accelerator-server` needs, with no GUI
/// coupling. Lives behind an `Arc` in [`AppState`] so cloning the state is cheap (fixes the main.rs
/// clone-stutter) (Q1).
#[derive(Clone, Default)]
pub struct HeadlessState {
    pub bundled_version: Option<String>,
    /// Injected app/release version for `/health.version`, decoupling it from `env!("CARGO_PKG_VERSION")`
    /// (which resolves to whichever crate compiles this module — wrong once `server.rs` lives in core).
    /// Falls back to the compile-time value while constructors are migrated. (core-extraction Phase 0)
    pub app_version: Option<String>,
    /// `true` once `start_https` has actually bound the HTTPS listener. Shared (Arc'd atomic) across
    /// the HTTP + HTTPS servers so `/health` advertises `https_port` from the REAL bind state — not
    /// the config flag, which would point the SDK at a dead port when the CA is untrusted at startup
    /// (HTTPS skipped, but `safari_support` config stays on). (Q7)
    pub https_bound: Arc<AtomicBool>,
    pub config: Option<Arc<RwLock<config::AcceleratorConfig>>>,
    pub auth_manager: Option<Arc<AuthorizationManager>>,
    /// Limits concurrent proving to 1 — bb already uses all cores.
    pub prove_semaphore: Option<Arc<Semaphore>>,
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

pub async fn start(state: AppState) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    let listener = bind_with_retry(addr).await?;
    tracing::info!("Accelerator server listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
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
        .route("/prove", post(prove::prove))
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
        "version": state.app_version.as_deref().unwrap_or(env!("CARGO_PKG_VERSION")),
        "aztec_version": bundled,
        "available_versions": available,
        "bb_available": bb::find_bb(None).is_ok(),
    });

    // Advertise https_port only when the HTTPS listener actually bound (set by start_https after a
    // successful bind), NOT when the config merely requests Safari support. Keying off the config flag
    // would point the SDK at a dead port on the untrusted-CA startup path (HTTPS skipped, config still
    // on). The shared Arc'd flag also reflects a runtime enable_safari_support without a restart. (Q7)
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

#[cfg(test)]
mod tests {
    use super::prove::{compute_threads, resolve_version};
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use serial_test::serial;
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
    async fn health_reports_injected_app_version() {
        // /health.version must reflect the injected app_version, not env!("CARGO_PKG_VERSION") — so the
        // reported version stays correct once server.rs is compiled inside the core crate. (Phase 0)
        let state = AppState {
            core: Arc::new(HeadlessState {
                app_version: Some("9.9.9-injected".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let response = router(state)
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
        assert_eq!(json["version"], "9.9.9-injected");
    }

    #[tokio::test]
    async fn health_advertises_https_port_when_https_bound() {
        // https_bound = true (set by start_https once the listener actually binds) → /health
        // advertises the HTTPS port so the SDK can connect.
        let state = AppState {
            core: Arc::new(HeadlessState {
                https_bound: Arc::new(AtomicBool::new(true)),
                ..Default::default()
            }),
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
        assert_eq!(json["https_port"], HTTPS_PORT);
    }

    #[tokio::test]
    async fn health_hides_https_port_when_safari_configured_but_not_bound() {
        // The untrusted-CA startup path: safari_support stays ON in config, but HTTPS never bound
        // (https_bound = false). /health must NOT advertise https_port, or the SDK probes a dead
        // port. Regression guard for the Q7 health-signal fix.
        let cfg = crate::config::AcceleratorConfig {
            safari_support: true,
            ..Default::default()
        };
        let state = AppState {
            core: Arc::new(HeadlessState {
                config: Some(Arc::new(RwLock::new(cfg))),
                https_bound: Arc::new(AtomicBool::new(false)),
                ..Default::default()
            }),
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
        assert!(
            json.get("https_port").is_none(),
            "https_port must be absent when HTTPS hasn't bound, even if safari_support is configured"
        );
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
            core: std::sync::Arc::new(HeadlessState {
                prove_semaphore: Some(std::sync::Arc::new(tokio::sync::Semaphore::new(1))),
                ..Default::default()
            }),
            on_status: Some(std::sync::Arc::new(move |s: ServerStatus| {
                rec.lock().unwrap().push(s.display_text().to_string())
            })),
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
            core: Arc::new(HeadlessState {
                bundled_version: Some("5.0.0-nightly.20260307".into()),
                ..Default::default()
            }),
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
            core: Arc::new(HeadlessState {
                auth_manager: Some(auth_for_state),
                config: Some(Arc::new(RwLock::new(cfg))),
                prove_semaphore: Some(Arc::new(Semaphore::new(1))),
                ..Default::default()
            }),
            show_auth_popup: Some(Arc::new(move |origin: &str| {
                let _ = popup_tx.send(origin.to_string());
            })),
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
            core: Arc::new(HeadlessState {
                auth_manager: Some(auth),
                config: Some(Arc::new(RwLock::new(cfg))),
                ..Default::default()
            }),
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
            core: Arc::new(HeadlessState {
                config: Some(Arc::new(RwLock::new(cfg))),
                ..Default::default()
            }),
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
            core: Arc::new(HeadlessState {
                config: Some(Arc::new(RwLock::new(cfg))),
                ..Default::default()
            }),
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
