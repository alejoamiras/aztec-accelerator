//! `/prove` origin authorization.
//!
//! Auto-approves localhost + persisted origins; otherwise popup-gates the request with a 60s
//! auto-deny (`AUTH_DECISION_TIMEOUT`). Headless mode (no popup callback) denies unknown origins.
//! Extracted from server.rs (Q2).

use crate::authorization::{AuthDecision, AuthorizationManager, CanonicalOrigin};
use crate::config;
use axum::http::StatusCode;

use super::{json_error, AppState, ProveError, AUTH_DECISION_TIMEOUT};

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
    let (rx, is_first) = auth_manager.request(origin.as_str()).map_err(|_| {
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
            show_popup(origin.as_str());
        }
    }

    let decision = tokio::time::timeout(AUTH_DECISION_TIMEOUT, rx)
        .await
        .map_err(|_| {
            tracing::warn!(origin = %origin, "Authorization timed out");
            auth_manager.resolve(origin.as_str(), AuthDecision::Deny);
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
