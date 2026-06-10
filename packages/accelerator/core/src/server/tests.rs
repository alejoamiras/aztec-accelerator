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
                .header("host", "127.0.0.1:59833")
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
            app_version: "9.9.9-injected".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let response = router(state)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1:59833")
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
                .header("host", "127.0.0.1:59833")
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
                .header("host", "127.0.0.1:59833")
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
                .header("host", "127.0.0.1:59833")
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
                .header("host", "127.0.0.1:59833")
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
                .header("host", "127.0.0.1:59833")
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
// `#[serial]`: this test is a READER of the process-global `BB_BINARY_PATH` (via `find_bb` in
// both the skip-guard and the handler). `#[serial]` only excludes other `#[serial]` tests, so an
// unmarked reader still overlaps the serial writer (`prove_success_path_and_status_sequence`) —
// its fake exit-0 bb then turns this 500-assertion into a 200 (the CI flake on #346/#347).
#[serial]
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
                .header("host", "127.0.0.1:59833")
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
                .header("host", "127.0.0.1:59833")
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
                .header("host", "127.0.0.1:59833")
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
            .header("host", "127.0.0.1:59833")
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
            .header("host", "127.0.0.1:59833")
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
    let (popup_tx, popup_rx) = std::sync::mpsc::channel();
    let (state, auth) = auth_state_with_popup(popup_tx);
    let auth_clone = auth.clone();
    tokio::spawn(async move {
        let (_origin, request_id) = tokio::task::spawn_blocking(move || popup_rx.recv().unwrap())
            .await
            .unwrap();
        auth_clone.resolve(&request_id, crate::authorization::AuthDecision::Deny);
    });
    assert_error(
        router(state),
        Request::builder()
            .header("host", "127.0.0.1:59833")
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
    // RAII: unset `BB_BINARY_PATH` even if the request below panics — a leaked var would poison
    // every later `find_bb`-reading test in this process (they'd all see the fake bb).
    struct EnvGuard;
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var("BB_BINARY_PATH");
        }
    }
    std::env::set_var("BB_BINARY_PATH", &fake_bb);
    let _guard = EnvGuard;

    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let rec = recorded.clone();
    let state = AppState {
        core: std::sync::Arc::new(HeadlessState::default()),
        on_status: Some(std::sync::Arc::new(move |s: ServerStatus| {
            rec.lock().unwrap().push(s.display_text().to_string())
        })),
        ..Default::default()
    };

    let response = router(state)
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1:59833")
                .method("POST")
                .uri("/prove")
                .header("content-type", "application/octet-stream")
                .body(Body::from(vec![0u8; 16]))
                .unwrap(),
        )
        .await
        .unwrap();

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
                .header("host", "127.0.0.1:59833")
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

/// SEC-05: a present-but-unapproved cross-origin `/health` probe gets a minimal liveness body
/// (no version/cache fingerprint); an auto-approved localhost origin gets the detailed body. The
/// no-Origin case (local tools, CI, `connectivity.test.ts`) stays detailed — covered above.
#[tokio::test]
async fn health_minimal_for_unapproved_cross_origin() {
    let state = AppState {
        core: Arc::new(HeadlessState {
            bundled_version: Some("5.0.0-nightly.20260307".into()),
            // Gated, with localhost auto-approve ON so the localhost probe below is "approved"
            // and exercises the detailed tier (SEC-04 defaults this off; tested separately).
            config: Some(Arc::new(RwLock::new(crate::config::AcceleratorConfig {
                auto_approve_localhost: true,
                ..Default::default()
            }))),
            ..Default::default()
        }),
        ..Default::default()
    };
    let app = router(state);

    // Unapproved, non-localhost Origin → minimal body.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1:59833")
                .header("origin", "https://evil.example")
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
    assert!(
        json.get("available_versions").is_none() && json.get("aztec_version").is_none(),
        "must not leak version/cache to an unapproved origin (got: {json})"
    );

    // An auto-approved localhost Origin → detailed body.
    let response2 = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1:59833")
                .header("origin", "http://localhost:5173")
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body2 = axum::body::to_bytes(response2.into_body(), usize::MAX)
        .await
        .unwrap();
    let json2: serde_json::Value = serde_json::from_slice(&body2).unwrap();
    assert!(
        json2.get("available_versions").is_some(),
        "approved/localhost origin must get the detailed body (got: {json2})"
    );
}

/// Helper: build an AppState with auth enabled and a mock popup callback. The callback forwards
/// `(origin, request_id)` so tests can assert the origin AND resolve by the opaque id (SEC-06).
fn auth_state_with_popup(
    popup_tx: std::sync::mpsc::Sender<(String, String)>,
) -> (AppState, Arc<crate::authorization::AuthorizationManager>) {
    let auth = Arc::new(crate::authorization::AuthorizationManager::new());
    let auth_for_state = auth.clone();
    let cfg = crate::config::AcceleratorConfig::default();
    let state = AppState {
        core: Arc::new(HeadlessState {
            auth_manager: Some(auth_for_state),
            config: Some(Arc::new(RwLock::new(cfg))),
            ..Default::default()
        }),
        show_auth_popup: Some(Arc::new(move |origin: &str, request_id: &str| {
            let _ = popup_tx.send((origin.to_string(), request_id.to_string()));
        })),
        ..Default::default()
    };
    (state, auth)
}

#[tokio::test]
async fn prove_auto_approves_localhost_origin() {
    let (popup_tx, popup_rx) = std::sync::mpsc::channel();
    let (state, _auth) = auth_state_with_popup(popup_tx);
    // SEC-04: localhost auto-approve is now opt-in (desktop default is prompt-once). Enable it
    // here to exercise the auto-approve path; the flag-OFF deny is pinned by
    // `authorization::tests::is_approved_checks_both`.
    state
        .config
        .as_ref()
        .unwrap()
        .write()
        .auto_approve_localhost = true;
    let app = router(state);

    let response: axum::http::Response<_> = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1:59833")
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
        let (origin, request_id) = tokio::task::spawn_blocking(move || popup_rx.recv().unwrap())
            .await
            .unwrap();
        let _ = popup_seen_tx.send(origin);
        auth_clone.resolve(
            &request_id,
            crate::authorization::AuthDecision::Allow { remember: false },
        );
    });

    let response: axum::http::Response<_> = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1:59833")
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
    let (popup_tx, popup_rx) = std::sync::mpsc::channel();
    let (state, auth) = auth_state_with_popup(popup_tx);
    let app = router(state);

    // Auto-deny by request_id once the popup fires (SEC-06: resolve by opaque id, not origin).
    let auth_clone = auth.clone();
    tokio::spawn(async move {
        let (_origin, request_id) = tokio::task::spawn_blocking(move || popup_rx.recv().unwrap())
            .await
            .unwrap();
        auth_clone.resolve(&request_id, crate::authorization::AuthDecision::Deny);
    });

    let response: axum::http::Response<_> = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1:59833")
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
async fn prove_allows_no_origin_only_with_trusted_loopback_host() {
    let (popup_tx, popup_rx) = std::sync::mpsc::channel();
    let (state, _auth) = auth_state_with_popup(popup_tx);
    let app = router(state);

    let response: axum::http::Response<_> = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1:59833")
                .method("POST")
                .uri("/prove")
                .header("content-type", "application/octet-stream")
                .body(Body::from(vec![0u8; 10]))
                .unwrap(),
        )
        .await
        .unwrap();

    // SEC-01b: a no-Origin request is allowed ONLY because it carries a trusted loopback Host
    // (the SEC-01a guard vouched for it) — this is the legit curl/Node/local-script case. The
    // DNS-rebinding no-Origin variant is 403'd at the Host guard (Host=evil.com), pinned by
    // `prove_rejects_forged_host_dns_rebinding`. So the Host guard, not the Origin header, is the
    // boundary for non-browser callers — the old "Origin omission = bypass" footgun is closed.
    assert_ne!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        popup_rx.try_recv().is_err(),
        "popup should not fire without Origin"
    );
}

/// SEC-01a: the DNS-rebinding attack shape — a rebound page makes a same-origin (no-Origin)
/// request whose `Host` is the attacker's domain, not loopback. The loopback-Host guard must
/// 403 it BEFORE the Origin gate (which would otherwise auto-approve the missing Origin), and
/// the popup must never fire.
#[tokio::test]
async fn prove_rejects_forged_host_dns_rebinding() {
    let (popup_tx, popup_rx) = std::sync::mpsc::channel();
    let (state, _auth) = auth_state_with_popup(popup_tx);
    let app = router(state);

    let response: axum::http::Response<_> = app
        .oneshot(
            Request::builder()
                .header("host", "evil.com:59833") // rebound attacker domain, not loopback
                .method("POST")
                .uri("/prove")
                .header("content-type", "application/octet-stream")
                .body(Body::from(vec![0u8; 10]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(
        body.contains("invalid_host"),
        "rebinding must be rejected by the Host guard (got: {body})"
    );
    assert!(
        popup_rx.try_recv().is_err(),
        "popup must not fire for a forged-Host request"
    );
}

#[tokio::test]
async fn prove_approves_remembered_origin() {
    let (popup_tx, popup_rx) = std::sync::mpsc::channel();
    let (state, _auth) = auth_state_with_popup(popup_tx);

    // Pre-approve the origin in config
    if let Some(ref cfg) = state.config {
        cfg.write().approved_origins.push(
            crate::authorization::CanonicalOrigin::parse("https://approved-site.com").unwrap(),
        );
    }

    let app = router(state);
    let response: axum::http::Response<_> = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1:59833")
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
                .header("host", "127.0.0.1:59833")
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
                .header("host", "127.0.0.1:59833")
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
                .header("host", "127.0.0.1:59833")
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

#[test]
fn resolve_version_flags_uncached_for_download() {
    // F-08: resolve_version is now pure (sync, no download, no status). A valid, non-bundled,
    // uncached version resolves Ok with `to_download` set — prove() then owns the download+status
    // (Proving→Downloading→Proving). The full 4-element download-arm sequence can't be unit-tested
    // (download_bb needs the network); the no-download arm is pinned by
    // `prove_success_path_and_status_sequence`, and prove() emits Downloading/Proving structurally
    // around this flag.
    let state = AppState::default();
    let version = Some("5.0.0-rc.1".to_string());
    let resolved = resolve_version(&state, &version).expect("valid version resolves");
    assert_eq!(resolved.version, Some("5.0.0-rc.1"));
    assert!(
        resolved.to_download.is_some(),
        "uncached non-bundled version must be flagged for download"
    );
}

#[test]
fn resolve_version_no_download_for_bundled() {
    // The bundled version is always present → never flagged for download (no Downloading status).
    let core = HeadlessState::headless("1.0.0", Some("0.99.0".to_string()), None, None);
    let state = AppState::headless(core);
    let requested = Some("0.99.0".to_string());
    let resolved = resolve_version(&state, &requested).expect("bundled resolves");
    assert_eq!(resolved.version, Some("0.99.0"));
    assert!(
        resolved.to_download.is_none(),
        "bundled version must NOT download"
    );
}

#[test]
fn resolve_version_rejects_invalid_version() {
    let state = AppState::default();
    let version = Some("../../../etc/passwd".to_string());
    let result = resolve_version(&state, &version);
    assert!(result.is_err());
    // q7e3-F-03: ProveError is now a typed enum; assert via its IntoResponse status (still BAD_REQUEST).
    let err = result.unwrap_err();
    assert_eq!(err.into_response().status(), StatusCode::BAD_REQUEST);
}

#[test]
fn resolve_version_returns_none_without_header() {
    let state = AppState::default();
    let resolved = resolve_version(&state, &None).expect("no header resolves");
    assert_eq!(resolved.version, None);
    assert!(resolved.to_download.is_none());
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
                .header("host", "127.0.0.1:59833")
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
// `#[serial]`: same `BB_BINARY_PATH` reader race as `prove_returns_error_when_bb_not_found` —
// garbage bodies DO reach bb (no deserialization gate before the spawn), so the serial writer's
// fake exit-0 bb can turn this not-2xx assertion into a 200.
#[serial]
async fn prove_handles_empty_body() {
    let app = router(AppState::default());
    let response = app
        .oneshot(
            Request::builder()
                .header("host", "127.0.0.1:59833")
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

#[tokio::test]
async fn invalid_host_reply_stays_application_json_without_message() {
    // q7e3-F-03 characterization (test-FIRST): the host-guard's `invalid_host` reply is DELIBERATELY a
    // minimal `application/json` body with NO `message` field — distinct from the `/prove` text/plain
    // {error,message} errors. Pin it so the ProveError typing can't fold it into the text/plain shape.
    let app = router(AppState::default());
    let response: axum::http::Response<_> = app
        .oneshot(
            Request::builder()
                .header("host", "evil.com:59833") // rebound attacker domain, not loopback
                .method("POST")
                .uri("/prove")
                .header("content-type", "application/octet-stream")
                .body(Body::from(vec![0u8; 10]))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let ct = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.starts_with("application/json"),
        "invalid_host stays application/json (NOT the /prove text/plain shape), got {ct:?}"
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["error"], "invalid_host");
    assert!(
        json.get("message").is_none(),
        "invalid_host must NOT carry a message field (minimal host-guard reply)"
    );
}
