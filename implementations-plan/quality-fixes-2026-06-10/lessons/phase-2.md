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

## F-12 ✓ (committed 7cb82f1)
File-extracted `server.rs` `mod tests` → `server/tests.rs` (`mod tests;`); `super::*` paths unchanged (file module = same module-tree node). server.rs 1424→331; core 132/132; clippy clean.

## F-03 — test-first DONE (committed), enum refactor NEXT
- **Characterization committed** (`q7e3-F-03` test): `invalid_host_reply_stays_application_json_without_message` pins host.rs's 403 + `application/json` + `{"error":"invalid_host"}` (NO message). Plus the existing `prove_error_responses_stay_text_plain_json_string` pins the prove/auth text/plain `{error,message}` sites. core 133/133.
- **Enum to implement** (`server.rs:312` `type ProveError = (StatusCode, String)` → `enum ProveError` + `impl IntoResponse` delegating to `(status, json_error(code, &msg)).into_response()` so Content-Type stays text/plain; `json_error`/`ProveErrorBody` stay). **host.rs EXCLUDED** — keep its `axum::Json` invalid_host. Variants (exact status/code/message — copy verbatim):
  - `InvalidVersion(String v)` → BAD_REQUEST, "invalid_version", `format!("Invalid x-aztec-version header (got '{v}')")` (prove.rs:64)
  - `PayloadTooLarge(String e)` → PAYLOAD_TOO_LARGE, "payload_too_large", `format!("Body too large or unreadable: {e}")` (prove.rs:123)
  - `ServiceUnavailable` → SERVICE_UNAVAILABLE, "service_unavailable", "Proving service shutting down" (prove.rs:135)
  - `DownloadFailed{version,detail}` → INTERNAL_SERVER_ERROR, "download_failed", `format!("Failed to download bb v{version}: {detail}")` (prove.rs:183)
  - `ProveFailed(String e)` → INTERNAL_SERVER_ERROR, "prove_failed", `e` (prove.rs:223, `e.to_string()`)
  - `InvalidOrigin` → BAD_REQUEST, "invalid_origin", "Origin header is not a valid RFC 6454 origin" (auth.rs:41)
  - `OriginDenied(String origin)` → FORBIDDEN, "origin_denied", `format!("Access denied for origin: {origin}")` (auth.rs:67 AND :132)
  - `TooManyRequests` → TOO_MANY_REQUESTS, "too_many_requests", "Too many pending authorization requests" (auth.rs:79)
  - `AuthorizationTimeout` → FORBIDDEN, "authorization_timeout", "Authorization request timed out" (auth.rs:99)
  - `AuthorizationCancelled` → FORBIDDEN, "authorization_cancelled", "Authorization request was cancelled" (auth.rs:105)
  - Update 11 call sites (prove.rs ×5, auth.rs ×6) to `Err(ProveError::Variant(..))`. Check `/prove` handler still returns `Result<_, ProveError>` (axum calls `.into_response()` on Err). Validate: 133/133 + the 2 characterization tests.

## F-09 ✓, F-15 ✓, F-13 ✓, F-10 ✓, F-01 ✓ (committed)
- F-09: `PendingState::insert`/`remove` encapsulate the dual-map sync (behavior-identical; auth-flow tests).
- F-15: `config::load_from`/`save_to`; roundtrip test exercises the real save/load.
- F-13: `core::config::lock_mutate_save(lock, FnOnce->bool)` — 3 callers (auth.rs conditional, mutate_config always+propagate, reset_safari_support always+swallow). Both crates green.
- F-10: `CrashRecoveryGuard` (Drop rearms early-returns, explicit `rearm_now()` before the no-return restart; `cfg(any(windows,test))`). 3 guard tests first.

## F-01 codex consult (AFK-logged) — DEVIATION from plan-prescribed shape
**Consult:** codex xhigh on the F-01 design (the plan prescribed `prepare_https(mode)`; I flagged the two flows genuinely diverge).
**Verdict: LIGHTER-SHAPE.** A single `prepare_https(mode)` would encode trust/generation/reset/save/renewal/error policy in one interface — where silent regressions hide; the existing `server.rs` (unifies only `spawn_https`) already shows the safer architecture. Extract only (1) the pure Launch-gate classifier, (2) optionally a tiny load_tls_then_spawn helper. Full-merge's easiest-to-break, in order: reset-vs-skip asymmetry, save-flag ordering, verify→ensure-trust (would prompt on launch), SEC-08 migrate-first.
**Decision:** proceeded with lighter-shape (classifier + 4 characterization tests; **skipped** the optional helper as marginal cross-file dedup). Rationale vs the AFK "codex-conflicts-with-approved-scope→stop" rule: this is *within* F-01's scope (still de-dups + tests the fragile gate, less invasively) and is the SAFER path to the plan's own hard constraint (behavior-preserving, preserve the 5 divergences) — not a scope change. Logged for review.

## F-04 ✓ (committed 0911119) — ALL 8 PR-2 FINDINGS DONE
Extracted `build_tray` + `build_desktop_state` from the ~150-line `.setup()` closure; the closure stays a thin ordered sequencer (SEC-08 migrate-first + manage-before-webdriver/HTTP kept verbatim inline). Key move: `build_desktop_state` takes `status` BY VALUE → the audit's clone-before-move hazard is now compiler-enforced. Validated: clippy `--all-targets` on default AND `--features webdriver`; 29 tests.

## Gotcha: commitlint silently ate docs commits
`docs:` commits whose subject started uppercase ("F-09 ✓ …") failed commitlint's `subject-case` rule; with `-q` + grep'd output the failure was invisible — five docs commits accumulated in the working tree before detection. Refactor commits passed because `q7e3-…` starts lowercase. Fix: lowercase docs subjects; check `git log` after docs-only commits.
