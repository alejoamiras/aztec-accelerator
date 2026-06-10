# Phase 2 — PR-2 (Rust comment-invariant→seam)

Branch `quality/pr2-rust-q7e3` off main (after PR-1 #349 merged green; plan dir now on main).

## PR-1 close (done)
codex post-impl on the PR-1 diff: **SHIP** — no behavior delta; all 4 (F-02/05/06/11) confirmed behavior-preserving (F-06 pin semantics exact, F-05 branch-equivalent incl. `"unknown"`/empty-array edges, F-11 phase sequence preserved, F-02 root-barrel contract unchanged). #349 CI all-green, merged.

## F-12 execution strategy (survey done; next firing executes)
`core/src/server.rs` `mod tests` is `:331`–`~1100` (~770 LOC). The audit's caveat holds: these are **router-level integration tests** (each builds `router(state)` + `oneshot`s a request), not submodule unit tests — so cluster by **owning concern**, and keep the genuinely cross-cutting ones with the handler:
- **STAY in server.rs** (test the `/health` handler + router/CORS, which live in server.rs): `health_returns_ok`, `health_reports_injected_app_version`, `health_advertises_https_port_when_https_bound`, `health_hides_https_port_*`, `health_includes_cors_headers`, `health_includes_runtime_diagnostics_in_debug`, `health_includes_available_versions`, `health_minimal_for_unapproved_cross_origin`, `cors_preflight_returns_correct_headers`, `cors_allows_aztec_version_header`. Plus the `#[test]` unit tests at `:1284+` unless they target a submodule fn.
- **MOVE to `server/auth.rs`** (auth-gate flow + the shared `auth_state_with_popup` helper at `:935`): `prove_auto_approves_localhost_origin`, `prove_triggers_popup_for_unknown_origin`, `prove_returns_403_when_origin_denied`, `prove_approves_remembered_origin`, `prove_returns_403_without_popup_in_headless`, `prove_returns_429_when_too_many_pending_origins`, `prove_returns_403_on_authorization_timeout`. (`auth_state_with_popup` becomes `pub(crate)` or moves with them.)
- **MOVE to `server/host.rs`**: `prove_rejects_forged_host_dns_rebinding`, `prove_allows_no_origin_only_with_trusted_loopback_host` (host-guard behavior).
- **MOVE to `server/prove.rs`**: `prove_returns_error_when_bb_not_found` (`#[serial]`), `prove_rejects_invalid_version_header`, `prove_error_responses_stay_text_plain_json_string`, `prove_success_path_and_status_sequence` (`#[serial]`).
- **Shared imports** the moved `#[cfg(test)] mod`s need: `crate::server::{router, router_for_port}`, `AppState`, `crate::server::HeadlessState`/`crate::config`/`crate::authorization`, `axum::{body::Body, http::{Request,StatusCode,HeaderMap}}`, `tower::ServiceExt` (oneshot), `parking_lot::RwLock`, `serial_test::serial`. Most types are already `pub(crate)`.
- **Risk: LOW** (compiler + `cargo test` catch every broken `use`/path). Validate with `cargo test --manifest-path packages/accelerator/core/Cargo.toml`.
- **Then F-03** (`ProveError` text/plain delegate, host.rs excluded) lands its diff in the now-relocated `prove.rs`/`auth.rs` test files — the reason F-12 goes first.

## Remaining PR-2 order
F-12 → F-03 [test-first: invalid_host + text/plain] → F-01 [test-first: launch-vs-settings] → F-04 → F-09 → F-10 [test-first: rearm-before-restart] → F-13 → F-15. (F-14 deferred → tracked issue.)
