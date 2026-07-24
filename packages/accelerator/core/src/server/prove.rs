//! `/prove` request handler + version/thread resolution.
//!
//! The core proving path: authorize the origin, buffer the body under a 50MB cap, serialize
//! behind the prove semaphore (bb already uses all cores), resolve+download the requested bb
//! version, then run the proof and return base64 + an `x-prove-duration-ms` header. Extracted
//! from server.rs (Q2).

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::HeaderValue;
use axum::response::IntoResponse;
use serde_json::json;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

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
/// Outcome of version resolution: the validated version for `bb::prove` (`None` = bundled default)
/// and whether `prove()` must download it first. **Pure** — no status emission, no download. (F-08:
/// the old `Option<&str>` + `Option<AztecVersion>` double representation is collapsed into ONE
/// validated `AztecVersion` + a `needs_download` flag; `prove` owns the whole
/// Proving→Downloading→Proving status sequence so it lives in one place.)
#[derive(Debug)]
pub(crate) struct ResolvedVersion {
    pub(crate) version: Option<versions::AztecVersion>,
    /// Only meaningful when `version` is `Some` (the bundled default is never downloaded).
    pub(crate) needs_download: bool,
}

pub(crate) fn resolve_version(
    state: &AppState,
    requested: &Option<String>,
) -> Result<ResolvedVersion, ProveError> {
    let v = match requested {
        Some(v) => v,
        None => {
            return Ok(ResolvedVersion {
                version: None,
                needs_download: false,
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

    // F-007: normalize an explicit bundled request to `None` — the bundled bb ships as the sidecar, never
    // the version cache, so it resolves via `find_bb(None)`. This also makes any `Some(v)` downstream an
    // unambiguously NON-bundled request, so `find_bb` can hard-error on a bad cache entry without a
    // wrong-version fallback (a bundled `Some(v)` would otherwise be indistinguishable).
    if v == bundled {
        return Ok(ResolvedVersion {
            version: None,
            needs_download: false,
        });
    }

    // codex audit #3: `x-aztec-version` is remote-controlled, so a non-bundled request must clear the
    // downgrade policy BEFORE we download/execute it — otherwise a dApp could force an authentic-but-
    // -vulnerable OLD bb (or an arbitrary dev build). Every version that survives this gate is one we
    // consider safe to prove with. The bundled request already returned above.
    if let Err(rej) = versions::check_version_selectable(&version, bundled) {
        tracing::warn!(version = %version, bundled = %bundled, reason = rej.reason(), "Refused remote bb version");
        return Err(ProveError::VersionNotAllowed {
            version: version.to_string(),
            reason: rej.reason(),
        });
    }

    // Re-download when the cache entry is absent OR present-but-marker-invalid (tampered/legacy), not
    // merely when the path is missing — `verify_cached_bb` rehashes the binary against its marker (F-007).
    let needs_download = versions::verify_cached_bb(&version).is_err();
    if needs_download {
        tracing::info!(version = %version, "Version not cached (or unverified), will download");
    }

    Ok(ResolvedVersion {
        version: Some(version),
        needs_download,
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

/// F-009: absolute deadline for buffering the request body while holding the single prove
/// permit. Bounds a slowloris/stalled uploader before the permit is released. 30s is generous
/// for 50MB over loopback (~1.7 MiB/s); it is a whole-body deadline, not an idle timeout, so
/// drip-feeding cannot extend it.
const BODY_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Reject an honestly-declared oversize body BEFORE acquiring the prove permit, so a client
/// advertising `Content-Length > MAX_BODY_SIZE` is turned away without occupying the single
/// permit. Chunked/underreported requests are still bounded by `to_bytes` — this is a cheap
/// fast-path, never the sole limit.
fn reject_declared_oversize(headers: &axum::http::HeaderMap) -> Result<(), ProveError> {
    // Inspect EVERY Content-Length value, and each comma-separated element within it (HTTP/2 and
    // some proxies emit a duplicate-but-consistent list). Parse as u64 so a value that would
    // overflow usize on 32-bit targets can't wrap below the cap; reject if any element exceeds the
    // cap or is malformed. Cheap pre-permit fast-path — the to_bytes limit remains authoritative.
    let mut seen: Option<u64> = None;
    for value in headers.get_all(axum::http::header::CONTENT_LENGTH) {
        let Ok(s) = value.to_str() else {
            return Err(ProveError::PayloadTooLarge(
                "non-ASCII Content-Length".to_string(),
            ));
        };
        for part in s.split(',') {
            let part = part.trim();
            // RFC 7230 §3.3.2: a Content-Length is `1*DIGIT`. Reject empty/partial/non-digit
            // elements (a permissive skip let `""`, `","`, `1,`, `,1` slip past); parse as u64 so
            // an over-long value can't wrap below the cap.
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(ProveError::PayloadTooLarge(format!(
                    "malformed Content-Length {part:?}"
                )));
            }
            let len: u64 = part.parse().map_err(|_| {
                ProveError::PayloadTooLarge(format!("unparsable Content-Length {part:?}"))
            })?;
            if len > MAX_BODY_SIZE as u64 {
                return Err(ProveError::PayloadTooLarge(format!(
                    "declared Content-Length {len} exceeds {MAX_BODY_SIZE}"
                )));
            }
            // RFC 7230 §3.3.2: multiple Content-Length values must all agree.
            match seen {
                Some(prev) if prev != len => {
                    return Err(ProveError::PayloadTooLarge(format!(
                        "conflicting Content-Length values {prev} and {len}"
                    )));
                }
                _ => seen = Some(len),
            }
        }
    }
    Ok(())
}

/// F-009: acquire the single prove permit, THEN buffer the body (under the size cap + an
/// absolute read timeout). Returns the owned permit alongside the bytes so the caller holds it
/// for the whole prove; the permit is released by RAII on every exit path (timeout, size error,
/// disconnect, cancellation, panic, success). Testable seam.
async fn acquire_and_read_body(
    semaphore: Arc<Semaphore>,
    raw_body: Body,
    max_body_size: usize,
    read_timeout: Duration,
) -> Result<(OwnedSemaphorePermit, Bytes), ProveError> {
    let permit = semaphore
        .acquire_owned()
        .await
        .map_err(|_| ProveError::ServiceUnavailable)?;

    let body = tokio::time::timeout(read_timeout, axum::body::to_bytes(raw_body, max_body_size))
        .await
        .map_err(|_| {
            tracing::warn!(
                timeout_secs = read_timeout.as_secs(),
                "Timed out reading /prove request body"
            );
            ProveError::BodyReadTimeout
        })?
        .map_err(|e| {
            tracing::warn!("Failed to read request body: {e}");
            ProveError::PayloadTooLarge(e.to_string())
        })?;

    Ok((permit, body))
}

/// F-009: try to enter the bounded set of in-flight + waiting authorized `/prove` requests.
/// Non-blocking: if the cap (`MAX_INFLIGHT_PROVE` permits) is full, shed immediately with 429
/// (`ProveQueueFull`) rather than queueing. The returned guard must be held for the whole request
/// so it is released (RAII) on every exit path. Testable seam.
fn try_enter(waiters: Arc<Semaphore>) -> Result<OwnedSemaphorePermit, ProveError> {
    waiters
        .try_acquire_owned()
        .map_err(|_| ProveError::ProveQueueFull)
}

pub(crate) async fn prove(
    State(state): State<AppState>,
    request: Request,
) -> Result<impl IntoResponse, ProveError> {
    tracing::info!("Received /prove request");

    // Extract headers before consuming the request body. Run authorization FIRST
    // so unapproved origins are rejected without buffering the (potentially large) body.
    let (parts, raw_body) = request.into_parts();
    authorize_origin(&state, &parts.headers).await?;

    // F-009: cap total in-flight + waiting authorized /prove requests. Held (RAII) for the whole
    // request; a burst beyond MAX_INFLIGHT_PROVE is shed immediately with 429 instead of queueing
    // behind slow uploaders and stacking fresh per-request read timeouts.
    let _inflight = try_enter(state.prove_waiters.clone())?;

    // F-009: turn away an honestly-declared oversize body before taking the prove permit.
    reject_declared_oversize(&parts.headers)?;

    // F-009: acquire the single prove permit BEFORE buffering the body, so concurrent requests
    // can't each pin a 50MB buffer ahead of the concurrency gate (memory DoS), and read the body
    // under an absolute timeout so a stalled uploader can't hold the only permit indefinitely.
    // `_permit` is held for the whole prove and released by RAII on every exit path.
    let (_permit, body) = acquire_and_read_body(
        state.prove_semaphore.clone(),
        raw_body,
        MAX_BODY_SIZE,
        BODY_READ_TIMEOUT,
    )
    .await?;
    tracing::debug!(payload_bytes = body.len(), "Prove request payload size");

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
        needs_download,
    } = resolve_version(&state, &requested_version)?;
    // q7e3-F-08 borrow discipline: borrow the version for the download arm (`as_ref`), never move it —
    // `bb::prove` still needs it after the download.
    if let (true, Some(version)) = (needs_download, version_for_prove.as_ref()) {
        if let Some(ref cb) = state.on_status {
            cb(ServerStatus::Downloading);
        }
        match versions::download_bb(version).await {
            Ok(_) => {
                tracing::info!(version = %version, "Download complete");
                let bundled_owned = state
                    .bundled_version
                    .as_deref()
                    .unwrap_or(super::DEFAULT_BB_VERSION)
                    .to_string();
                // F-007: the version we just downloaded is about to be proved — exempt it from this
                // cleanup so the detached eviction can't delete it out from under `bb::prove`.
                let in_use_owned = version.as_str().to_string();
                let on_versions_changed = state.on_versions_changed.clone();
                tokio::spawn(async move {
                    // q7e3-F-08: the caller parses now; an unparseable bundled (defensive,
                    // unreachable in practice) skips cleanup — same outcome as the old internal
                    // parse-else-return. The "unknown" sentinel still parses, so eviction semantics
                    // in unknown-bundled builds are unchanged (#352 stays deferred).
                    if let Some(bundled) = versions::AztecVersion::parse(&bundled_owned) {
                        let in_use = versions::AztecVersion::parse(&in_use_owned);
                        versions::cleanup_old_versions(&bundled, in_use.as_ref()).await;
                    }
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
    let result = bb::prove(&body, version_for_prove.as_ref(), threads).await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header::CONTENT_LENGTH, HeaderMap};

    fn content_length(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(CONTENT_LENGTH, HeaderValue::from_str(v).unwrap());
        h
    }

    #[test]
    fn declared_oversize_rejected_before_permit() {
        // F-009: an honestly-declared oversize Content-Length is turned away up front.
        assert!(matches!(
            reject_declared_oversize(&content_length(&(MAX_BODY_SIZE + 1).to_string())),
            Err(ProveError::PayloadTooLarge(_))
        ));
        // At/under the cap and absent are allowed (still bounded later by to_bytes).
        assert!(reject_declared_oversize(&content_length(&MAX_BODY_SIZE.to_string())).is_ok());
        assert!(reject_declared_oversize(&HeaderMap::new()).is_ok());
        // Comma-list (HTTP/2 duplicate) with an oversize element must NOT slip past.
        assert!(matches!(
            reject_declared_oversize(&content_length(&format!("{0}, {0}", MAX_BODY_SIZE + 1))),
            Err(ProveError::PayloadTooLarge(_))
        ));
        // Malformed / empty / partial values are rejected, not silently ignored.
        for bad in ["not-a-number", "", ",", "1,", ",1", "1 2"] {
            assert!(
                matches!(
                    reject_declared_oversize(&content_length(bad)),
                    Err(ProveError::PayloadTooLarge(_))
                ),
                "must reject malformed Content-Length {bad:?}"
            );
        }
        // Conflicting comma-list values (RFC 7230 §3.3.2) are rejected even when each is under cap.
        assert!(matches!(
            reject_declared_oversize(&content_length("10, 20")),
            Err(ProveError::PayloadTooLarge(_))
        ));
        // Agreeing duplicates under the cap are fine.
        assert!(reject_declared_oversize(&content_length("10, 10")).is_ok());
    }

    #[test]
    fn waiter_cap_sheds_excess_with_queue_full() {
        // F-009: fill the cap; the next entry is shed with ProveQueueFull; a slot frees on drop.
        let waiters = Arc::new(Semaphore::new(2));
        let g1 = try_enter(waiters.clone()).expect("slot 1");
        let _g2 = try_enter(waiters.clone()).expect("slot 2");
        assert!(matches!(
            try_enter(waiters.clone()),
            Err(ProveError::ProveQueueFull)
        ));
        drop(g1);
        assert!(try_enter(waiters.clone()).is_ok(), "slot freed on drop");
    }

    #[tokio::test(start_paused = true)]
    async fn body_not_read_until_permit_available() {
        // F-009 ordering: with zero prove permits, even a READY body must not be returned — the
        // permit gates the read (proves permit-before-body, not merely that the guard is live).
        let sem = Arc::new(Semaphore::new(0));
        let fut = acquire_and_read_body(
            sem,
            Body::from(Bytes::from_static(b"ready")),
            1024,
            Duration::from_secs(30),
        );
        tokio::pin!(fut);
        assert!(
            tokio::time::timeout(Duration::from_secs(60), &mut fut)
                .await
                .is_err(),
            "acquire_and_read_body must not resolve without a prove permit"
        );
    }

    #[tokio::test]
    async fn permit_acquired_before_body_and_released_on_drop() {
        let sem = Arc::new(Semaphore::new(1));
        let (permit, body) = acquire_and_read_body(
            sem.clone(),
            Body::from(Bytes::from_static(b"hello")),
            1024,
            Duration::from_secs(30),
        )
        .await
        .expect("happy path");
        assert_eq!(&body[..], &b"hello"[..]);
        assert_eq!(
            sem.available_permits(),
            0,
            "permit held across the whole prove"
        );
        drop(permit);
        assert_eq!(sem.available_permits(), 1, "permit released on drop");
    }

    #[tokio::test]
    async fn oversized_body_errs_and_releases_permit() {
        let sem = Arc::new(Semaphore::new(1));
        let res = acquire_and_read_body(
            sem.clone(),
            Body::from(vec![0u8; 10]),
            4, // tiny cap to force the length error deterministically
            Duration::from_secs(30),
        )
        .await;
        assert!(matches!(res, Err(ProveError::PayloadTooLarge(_))));
        assert_eq!(
            sem.available_permits(),
            1,
            "permit released after size error"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn stalled_body_times_out_and_releases_permit() {
        let sem = Arc::new(Semaphore::new(1));
        // A body whose stream never yields → to_bytes never completes → the read timeout fires.
        // Under start_paused the virtual clock auto-advances to the only pending timer.
        let body =
            Body::from_stream(futures_util::stream::pending::<Result<Bytes, std::io::Error>>());
        let res = acquire_and_read_body(sem.clone(), body, 1024, Duration::from_secs(30)).await;
        assert!(matches!(res, Err(ProveError::BodyReadTimeout)));
        assert_eq!(sem.available_permits(), 1, "permit released after timeout");
    }
}
