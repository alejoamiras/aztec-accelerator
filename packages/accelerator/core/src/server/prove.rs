//! `/prove` request handler + version/thread resolution.
//!
//! The core proving path: authorize the origin, buffer the body under a 50MB cap, serialize
//! behind the prove semaphore (bb already uses all cores), resolve+download the requested bb
//! version, then run the proof and return base64 + an `x-prove-duration-ms` header. Extracted
//! from server.rs (Q2).

use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::IntoResponse;
use serde_json::json;

use crate::{bb, versions};

use super::auth::authorize_origin;
use super::{json_error, AppState, ProveError, ServerStatus, StatusCallback};

/// Drop guard that resets tray status to Idle when the prove handler exits for any reason
/// (success, error, client disconnect, panic).
struct StatusGuard {
    cb: Option<StatusCallback>,
}

impl Drop for StatusGuard {
    fn drop(&mut self) {
        if let Some(ref cb) = self.cb {
            cb(ServerStatus::Idle);
        }
    }
}

/// Validate and resolve the requested Aztec version. Downloads the bb binary if needed.
pub(crate) async fn resolve_version<'a>(
    state: &AppState,
    requested: &'a Option<String>,
) -> Result<Option<&'a str>, ProveError> {
    let v = match requested {
        Some(v) => v,
        None => return Ok(None),
    };

    // Construct the validated value object ONCE at the ingress boundary (Q3). Parse failure returns
    // the same 400 the bare `is_valid_version` check did; every downstream sink takes `&AztecVersion`,
    // so the #99 traversal guard is enforced by construction rather than re-checked per sink.
    let version = match versions::AztecVersion::parse(v) {
        Some(av) => av,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                json_error(
                    "invalid_version",
                    &format!("Invalid x-aztec-version header (got '{v}')"),
                ),
            ));
        }
    };
    tracing::info!(version = %version, "Requested Aztec version");

    let bundled = state
        .bundled_version
        .as_deref()
        .unwrap_or(super::DEFAULT_BB_VERSION);

    if v != bundled && !versions::version_bb_path(&version).exists() {
        tracing::info!(version = %version, "Version not cached, downloading");
        if let Some(ref cb) = state.on_status {
            cb(ServerStatus::Downloading);
        }

        match versions::download_bb(&version).await {
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
            cb(ServerStatus::Proving);
        }
    }

    Ok(Some(v.as_str()))
}

/// Read the speed setting from config and convert to thread count.
/// Returns None for "full" (let bb use its default).
pub(crate) fn compute_threads(state: &AppState) -> Option<usize> {
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

pub(crate) async fn prove(
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
        cb(ServerStatus::Proving);
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
