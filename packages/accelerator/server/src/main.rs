//! Headless accelerator server — no Tauri, no GUI.
//!
//! Runs the same Axum HTTP server as the Tauri app but without any display
//! context. Used in CI for e2e testing against the native `bb` binary.
//!
//! Origin gating (SEC-01c — deny-by-default):
//!   • `ALLOWED_ORIGINS=a,b` — gate; pre-approve those origins.
//!   • unset (default)       — gate with an empty allowlist: **deny every non-localhost origin**
//!     (localhost/127.0.0.1/[::1] stay auto-approved). Unset no longer means "approve everyone".
//!   • `--allow-all` / `ACCEL_ALLOW_ALL=1` — opt back into no gating (all origins). Mutually
//!     exclusive with `ALLOWED_ORIGINS`.
//! All requests are additionally constrained to a loopback `Host` (SEC-01a, see `core::server::host`).

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

    // Origin gating, three explicit modes (SEC-01c — deny-by-default):
    //   • allow-all  (`--allow-all` or `ACCEL_ALLOW_ALL=1`)  → NO gating (auth_manager None). Opt-in.
    //   • allowlist  (`ALLOWED_ORIGINS=a,b`)                  → gate; pre-approve those origins.
    //   • default    (neither)                                → gate with an EMPTY allowlist: deny every
    //     non-localhost origin (localhost/127.0.0.1/[::1] stay auto-approved). **Unset no longer means
    //     "approve everyone"** — that was the SEC-01 headless fail-open.
    // `--allow-all` and `ALLOWED_ORIGINS` express conflicting intent → mutually exclusive, fail loud.
    let allow_all = std::env::args().skip(1).any(|a| a == "--allow-all")
        || std::env::var("ACCEL_ALLOW_ALL").is_ok_and(|v| v == "1" || v == "true");
    let allowed_origins_env = std::env::var("ALLOWED_ORIGINS").ok();

    let (auth_manager, config) = match resolve_gating(allow_all, allowed_origins_env.as_deref()) {
        Err(e) => {
            tracing::error!(error = %e, "Invalid origin-gating configuration; refusing to start");
            std::process::exit(1);
        }
        Ok(Gating::AllowAll) => {
            tracing::warn!(
                "Running with --allow-all: ALL browser origins can reach /prove without approval. \
                 Prefer ALLOWED_ORIGINS to restrict, or leave both unset for localhost-only gating."
            );
            (None, None)
        }
        Ok(Gating::Gated(origins)) => {
            tracing::info!(
                origins = ?origins,
                "Origin gating enabled (deny-by-default): localhost auto-approved; non-localhost must be in ALLOWED_ORIGINS"
            );
            let cfg = AcceleratorConfig {
                approved_origins: origins,
                // Headless has no approval popup → keep localhost auto-approved (SEC-04/R13). The
                // prompt-once default is desktop-only; operators scope localhost via ALLOWED_ORIGINS.
                auto_approve_localhost: true,
                ..Default::default()
            };
            (
                Some(Arc::new(AuthorizationManager::new())),
                Some(Arc::new(RwLock::new(cfg))),
            )
        }
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
/// an empty list, NOT an error — the caller still enables gating, preserving today's
/// "var present ⇒ gated" behavior. An empty list denies every NON-localhost origin (localhost,
/// 127.0.0.1, [::1] stay auto-approved via `AuthorizationManager::is_auto_approved`).
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

/// Resolved origin-gating mode (SEC-01c). Pure decision over the env inputs so it is unit-testable;
/// the side effects (process exit / warn / state construction) stay in `main`.
enum Gating {
    /// `--allow-all` / `ACCEL_ALLOW_ALL` — no auth_manager, every origin reaches `/prove`.
    AllowAll,
    /// Gated with this (possibly empty) allowlist. Empty ⇒ deny non-localhost (localhost auto-approved).
    Gated(Vec<CanonicalOrigin>),
}

/// Decide the gating mode. `--allow-all` and `ALLOWED_ORIGINS` are mutually exclusive (conflicting
/// intent → `Err`). Unset+not-allow-all ⇒ `Gated([])` (deny-by-default). An invalid non-empty
/// `ALLOWED_ORIGINS` entry ⇒ `Err`.
fn resolve_gating(allow_all: bool, allowed_origins: Option<&str>) -> Result<Gating, String> {
    match (allow_all, allowed_origins) {
        (true, Some(_)) => Err(
            "--allow-all / ACCEL_ALLOW_ALL is mutually exclusive with ALLOWED_ORIGINS".to_string(),
        ),
        (true, None) => Ok(Gating::AllowAll),
        (false, Some(raw)) => Ok(Gating::Gated(parse_allowed_origins_env(raw)?)),
        (false, None) => Ok(Gating::Gated(Vec::new())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn co(s: &str) -> CanonicalOrigin {
        CanonicalOrigin::parse(s).unwrap()
    }

    #[test]
    fn gating_allow_all_no_origins() {
        assert!(matches!(resolve_gating(true, None), Ok(Gating::AllowAll)));
    }

    #[test]
    fn gating_allow_all_conflicts_with_allowlist() {
        assert!(resolve_gating(true, Some("https://a.com")).is_err());
        assert!(resolve_gating(true, Some("")).is_err()); // even an empty allowlist is a conflict
    }

    #[test]
    fn gating_default_is_deny_by_default_empty_allowlist() {
        // The SEC-01c fix: neither flag → gated with an EMPTY allowlist (deny non-localhost).
        match resolve_gating(false, None) {
            Ok(Gating::Gated(v)) => assert!(v.is_empty()),
            other => panic!("expected Gated([]), got {:?}", other.is_ok()),
        }
    }

    #[test]
    fn gating_allowlist_parses_origins() {
        match resolve_gating(false, Some("https://a.com, http://b.com")) {
            Ok(Gating::Gated(v)) => assert_eq!(v, vec![co("https://a.com"), co("http://b.com")]),
            _ => panic!("expected Gated(origins)"),
        }
    }

    #[test]
    fn gating_present_but_empty_allowlist_still_gates() {
        match resolve_gating(false, Some("")) {
            Ok(Gating::Gated(v)) => assert!(v.is_empty()),
            _ => panic!("present-but-empty must still gate (deny non-localhost)"),
        }
    }

    #[test]
    fn gating_invalid_origin_errs() {
        assert!(resolve_gating(false, Some("not-a-url")).is_err());
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
        // presence to enable gating, which denies non-localhost origins). Empty/whitespace/
        // trailing-comma all parse to [].
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
