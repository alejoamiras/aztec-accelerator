//! Headless accelerator server — no Tauri, no GUI.
//!
//! Runs the same Axum HTTP server as the Tauri app but without any display
//! context. Used in CI for e2e testing against the native `bb` binary.
//!
//! Set `ALLOWED_ORIGINS=origin1,origin2` to restrict which origins can call `/prove`.
//! When unset, all origins are auto-approved (no auth_manager).

use accelerator_core::authorization::{AuthorizationManager, CanonicalOrigin};
use accelerator_core::config::AcceleratorConfig;
use accelerator_core::server::{start, AppState, HeadlessState};
use parking_lot::RwLock;
use std::sync::Arc;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Minimal arg handling: `--version` / `-V` prints the crate version and exits.
    // The version comes from this crate's Cargo.toml (patched per-release by the
    // release workflow), giving the headless tarball a self-report surface.
    if std::env::args()
        .skip(1)
        .any(|a| a == "--version" || a == "-V")
    {
        println!("accelerator-server {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stdout))
        .init();

    tracing::info!("Starting headless accelerator server");

    // If ALLOWED_ORIGINS is set, enforce origin gating with those origins pre-approved.
    // Without it, auth_manager is None and all origins are auto-approved.
    let (auth_manager, config) = if let Ok(origins_str) = std::env::var("ALLOWED_ORIGINS") {
        // PRESENCE semantics (F-02): the var being SET enables gating even if it parses to an
        // empty list (= deny ALL browser origins) — exactly as before. Only an invalid NON-empty
        // entry is fatal (operator security input → fail loud, don't silently drop).
        let origins = match parse_allowed_origins_env(&origins_str) {
            Ok(o) => o,
            Err(e) => {
                tracing::error!(error = %e, "Invalid ALLOWED_ORIGINS; refusing to start");
                std::process::exit(1);
            }
        };
        tracing::info!(origins = ?origins, "Restricting to allowed origins");
        let cfg = AcceleratorConfig {
            approved_origins: origins,
            ..Default::default()
        };
        (
            Some(Arc::new(AuthorizationManager::new())),
            Some(Arc::new(RwLock::new(cfg))),
        )
    } else {
        (None, None)
    };

    let state = AppState::headless(HeadlessState::headless(
        env!("CARGO_PKG_VERSION"),
        // bb-version injected from the runtime env (the Phase-3 CI hook sets AZTEC_BB_VERSION from the
        // copy-bb.ts @aztec/bb.js resolution). Unset → None → core's "unknown" default; /prove is
        // unaffected (callers pass x-aztec-version). (core-extraction Phase 2)
        std::env::var("AZTEC_BB_VERSION").ok(),
        config,
        auth_manager,
    ));

    if let Err(e) = start(state).await {
        tracing::error!("Accelerator server error: {e}");
        std::process::exit(1);
    }
}

/// Parse the `ALLOWED_ORIGINS` env value into canonical origins (F-02).
///
/// Pipeline: split on `,` → trim → drop empty segments → canonicalize each non-empty entry →
/// dedupe (order-preserving). Returns `Err` (operator should fix it) only when a NON-empty
/// entry is not a valid RFC-6454 origin. An all-empty value (`""`, `",,"`, whitespace) yields
/// an empty list, NOT an error — the caller still enables gating (deny-all), preserving today's
/// "var present ⇒ gated" behavior.
fn parse_allowed_origins_env(raw: &str) -> Result<Vec<CanonicalOrigin>, String> {
    let mut out: Vec<CanonicalOrigin> = Vec::new();
    for seg in raw.split(',') {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        let canon = CanonicalOrigin::parse(seg)
            .ok_or_else(|| format!("invalid origin in ALLOWED_ORIGINS: {seg:?}"))?;
        if !out.contains(&canon) {
            out.push(canon);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn co(s: &str) -> CanonicalOrigin {
        CanonicalOrigin::parse(s).unwrap()
    }

    #[test]
    fn parses_and_canonicalizes() {
        assert_eq!(
            parse_allowed_origins_env("HTTPS://A.COM:443, http://b.com").unwrap(),
            vec![co("https://a.com"), co("http://b.com")],
        );
    }

    #[test]
    fn present_but_empty_yields_empty_list_not_error() {
        // The security-critical case: a present-but-empty value must NOT error (the caller keys on
        // presence to enable deny-all gating). Empty/whitespace/trailing-comma all parse to [].
        for raw in ["", "   ", ",", ",,", " , "] {
            assert_eq!(
                parse_allowed_origins_env(raw).unwrap(),
                Vec::<CanonicalOrigin>::new(),
                "raw {raw:?} should be Ok([])",
            );
        }
    }

    #[test]
    fn trailing_comma_and_whitespace_tolerated() {
        assert_eq!(
            parse_allowed_origins_env("https://a.com, ,").unwrap(),
            vec![co("https://a.com")],
        );
    }

    #[test]
    fn dedupes_order_preserving() {
        assert_eq!(
            parse_allowed_origins_env("https://b.com,https://a.com,HTTPS://b.com:443").unwrap(),
            vec![co("https://b.com"), co("https://a.com")],
        );
    }

    #[test]
    fn fails_fast_on_invalid_non_empty_entry() {
        assert!(parse_allowed_origins_env("https://a.com,not a url").is_err());
        assert!(parse_allowed_origins_env("https://a.com/path").is_err());
    }
}
