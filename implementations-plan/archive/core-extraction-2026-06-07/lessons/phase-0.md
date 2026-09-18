# Phase 0 — Version-decoupling prep

## Baseline (headless dep tree, on main @ feat/core-extraction-phase-0 base)
- `cargo tree -p accelerator-server --edges normal --prefix none | sort -u | wc -l` = **446** (codex's 475 counts all edges incl. build/dev).
- GUI/serving crates present in headless tree today: `rcgen tao tauri tokio-rustls wry` (the subtree the extraction removes).
- Cold per-target build-time: deferred to the Phase-3 measurement step (deps don't change until Phase 1/2).

## Work
1. `HeadlessState` already has `bundled_version: Option<String>`; both bb-version reads (server.rs:146, prove.rs:62) already use `unwrap_or(env!("AZTEC_BB_VERSION"))`. Fallback PRESERVED this phase.
2. Add `app_version: Option<String>` to `HeadlessState`; `/health` (server.rs:159) reads `state.app_version.as_deref().unwrap_or(env!("CARGO_PKG_VERSION"))` instead of the bare `env!`.
3. Constructors inject both: GUI (main.rs) `Some(env!(...))`; headless (server/main.rs) `app_version: Some(env!("CARGO_PKG_VERSION"))` (bundled_version stays None → env! fallback until the Phase-2/3 copy-bb.ts hook).
4. New test: `/health.version == injected app_version`.

## Attempts

- **Result: GREEN (single attempt).** Phase 0 was smaller than scoped — `HeadlessState.bundled_version` already
  existed with `unwrap_or(env!("AZTEC_BB_VERSION"))` at both bb-version sites, so only `app_version` needed plumbing.
  - Added `app_version: Option<String>` to `HeadlessState`; `/health` now reads
    `state.app_version.as_deref().unwrap_or(env!("CARGO_PKG_VERSION"))` (fallback preserved → `..Default::default()`
    tests unchanged).
  - Injected in both production constructors: GUI `main.rs:347` + headless `server/main.rs:63`
    (`Some(env!("CARGO_PKG_VERSION").to_string())`).
  - New test `health_reports_injected_app_version` (asserts the injected value wins).
  - Validation: `cargo test` (src-tauri) `--lib server::tests` → **29 passed, 0 failed**; `cargo build` headless →
    **exit 0**; `cargo fmt` applied.
  - Note: in a RELEASE build both crate versions are patched to the release version, so headless `/health.version`
    is unchanged in release; in dev it now reports `accelerator-server`'s own version (was `src-tauri`'s via the lib
    `env!`) — a latent-bug fix, release-invisible.
  - bb-version `env!` removal deferred to Phase 1 (when core has no `build.rs`); the fallback stays this phase per plan.
