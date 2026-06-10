//! `/prove` origin authorization.
//!
//! Approves persisted origins (and localhost only when `auto_approve_localhost` is set — desktop
//! default is prompt-once, SEC-04); otherwise popup-gates the request with a 60s auto-deny
//! (`AUTH_DECISION_TIMEOUT`). Headless mode (no popup callback) denies unapproved origins. All
//! requests are first constrained to a loopback `Host` (SEC-01a, `super::host`). Extracted from
//! server.rs (Q2).

use crate::authorization::{AuthDecision, AuthorizationManager, CanonicalOrigin};
use crate::config;

use super::{AppState, ProveError, AUTH_DECISION_TIMEOUT};

/// Check if the request origin is authorized. Returns Ok(()) if approved.
pub(crate) async fn authorize_origin(
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

    let origin = match CanonicalOrigin::parse(raw_origin) {
        Some(canon) => canon,
        None => {
            tracing::warn!(raw_origin = %raw_origin, "Invalid Origin header (path/query/userinfo/unknown scheme); rejecting");
            return Err(ProveError::InvalidOrigin);
        }
    };

    let approved = state.config.as_ref().is_some_and(|cfg| {
        let cfg = cfg.read();
        AuthorizationManager::is_approved(
            &origin,
            &cfg.approved_origins,
            cfg.auto_approve_localhost,
        )
    });

    if approved {
        return Ok(());
    }

    // No popup callback = headless mode → deny immediately
    if state.show_auth_popup.is_none() {
        tracing::info!(origin = %origin, "Origin not approved (no popup available), denying");
        return Err(ProveError::OriginDenied(origin.to_string()));
    }

    tracing::info!(origin = %origin, "Origin not approved, requesting authorization");
    let (rx, request_id, is_first) = auth_manager.request(origin.as_str()).map_err(|_| {
        tracing::warn!(origin = %origin, "Too many pending authorization requests");
        ProveError::TooManyRequests
    })?;

    if is_first {
        if let Some(ref show_popup) = state.show_auth_popup {
            show_popup(origin.as_str(), &request_id);
        }
    }

    let decision = tokio::time::timeout(AUTH_DECISION_TIMEOUT, rx)
        .await
        .map_err(|_| {
            tracing::warn!(origin = %origin, "Authorization timed out");
            auth_manager.resolve(&request_id, AuthDecision::Deny);
            ProveError::AuthorizationTimeout
        })?
        .map_err(|_| ProveError::AuthorizationCancelled)?;

    match decision {
        AuthDecision::Allow { remember } => {
            tracing::info!(origin = %origin, remember, "Origin authorized");
            if remember {
                if let Some(ref cfg_lock) = state.config {
                    // q7e3-F-13: shared core helper; the closure's bool keeps the conditional save (only
                    // when the origin is new) — no always-write on the piggyback-Allow path. Warn-and-
                    // continue on save failure (a config-write error must NOT fail an approved prove).
                    if let Err(e) = config::lock_mutate_save(cfg_lock, |cfg| {
                        if cfg.approved_origins.contains(&origin) {
                            false
                        } else {
                            cfg.approved_origins.push(origin);
                            true
                        }
                    }) {
                        tracing::warn!(error = %e, "Failed to persist approved origin");
                    }
                }
            }
            Ok(())
        }
        AuthDecision::Deny => {
            tracing::info!(origin = %origin, "Origin denied");
            Err(ProveError::OriginDenied(origin.to_string()))
        }
    }
}
