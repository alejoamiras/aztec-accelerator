//! `/prove` request handler + version/thread resolution.
//!
//! The core proving path: authorize the origin, buffer the body under a 50MB cap, serialize
//! behind the prove semaphore (bb already uses all cores), resolve+download the requested bb
//! version, then run the proof and return base64 + an `x-prove-duration-ms` header. Extracted
//! from server.rs (Q2).

use axum::extract::{Request, State};
use axum::http::HeaderValue;
use axum::response::IntoResponse;
use serde_json::json;

use crate::{bb, versions};

use super::auth::authorize_origin;
use super::{AppState, ProveError, ServerStatus, StatusCallback};

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
/// Outcome of version resolution: the version string for `bb::prove` (`None` = bundled default) and,
/// when the requested version isn't cached + isn't the bundled one, the parsed version `prove()` must
/// download first. **Pure** — no status emission, no download. (F-08: `prove` owns the whole
/// Proving→Downloading→Proving status sequence so it lives in one place.)
#[derive(Debug)]
pub(crate) struct ResolvedVersion<'a> {
    pub(crate) version: Option<&'a str>,
    pub(crate) to_download: Option<versions::AztecVersion>,
}

pub(crate) fn resolve_version<'a>(
    state: &AppState,
    requested: &'a Option<String>,
) -> Result<ResolvedVersion<'a>, ProveError> {
    let v = match requested {
        Some(v) => v,
        None => {
            return Ok(ResolvedVersion {
                version: None,
                to_download: None,
            })
        }
    };

    // Construct the validated value object ONCE at the ingress boundary (Q3). Parse failure returns
    // the same 400 the bare `is_valid_version` check did; every downstream sink takes `&AztecVersion`,
    // so the #99 traversal guard is enforced by construction rather than re-checked per sink.
    let version = match versions::AztecVersion::parse(v) {
        Some(av) => av,
        None => {
            return Err(ProveError::InvalidVersion(v.to_string()));
        }
    };
    tracing::info!(version = %version, "Requested Aztec version");

    let bundled = state
        .bundled_version
        .as_deref()
        .unwrap_or(super::DEFAULT_BB_VERSION);

    let to_download = if v != bundled && !versions::version_bb_path(&version).exists() {
        tracing::info!(version = %version, "Version not cached, will download");
        Some(version)
    } else {
        None
    };

    Ok(ResolvedVersion {
        version: Some(v.as_str()),
        to_download,
    })
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
            ProveError::PayloadTooLarge(e.to_string())
        })?;
    tracing::debug!(payload_bytes = body.len(), "Prove request payload size");

    // Limit to one concurrent prove — bb already uses all cores.
    let _permit = state
        .prove_semaphore
        .acquire()
        .await
        .map_err(|_| ProveError::ServiceUnavailable)?;

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

    // Resolve (pure: parse + cache check), then OWN the status sequence here (F-08): the whole
    // Proving→(Downloading→Proving)→Idle machine lives in one function. `resolve_version` no longer
    // emits status or downloads.
    let ResolvedVersion {
        version: version_for_prove,
        to_download,
    } = resolve_version(&state, &requested_version)?;
    if let Some(version) = to_download {
        if let Some(ref cb) = state.on_status {
            cb(ServerStatus::Downloading);
        }
        match versions::download_bb(&version).await {
            Ok(_) => {
                tracing::info!(version = %version, "Download complete");
                let bundled_owned = state
                    .bundled_version
                    .as_deref()
                    .unwrap_or(super::DEFAULT_BB_VERSION)
                    .to_string();
                let on_versions_changed = state.on_versions_changed.clone();
                tokio::spawn(async move {
                    versions::cleanup_old_versions(&bundled_owned).await;
                    if let Some(cb) = on_versions_changed {
                        cb();
                    }
                });
            }
            Err(e) => {
                tracing::error!(version = %version, error = %e, "Failed to download bb");
                return Err(ProveError::DownloadFailed {
                    version: version.to_string(),
                    detail: e.to_string(),
                });
            }
        }
        // Re-emit Proving after the Downloading interlude — preserves the redundant leading Proving so
        // the download-arm sequence stays [Proving, Downloading, Proving, Idle]. (F-08 / opus H2)
        if let Some(ref cb) = state.on_status {
            cb(ServerStatus::Proving);
        }
    }
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

    let proof = result.map_err(|e| ProveError::ProveFailed(e.to_string()))?;
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &proof);

    let mut response = axum::Json(json!({ "proof": encoded })).into_response();
    response.headers_mut().insert(
        "x-prove-duration-ms",
        HeaderValue::from_str(&elapsed.as_millis().to_string()).unwrap(),
    );

    Ok(response)
}
