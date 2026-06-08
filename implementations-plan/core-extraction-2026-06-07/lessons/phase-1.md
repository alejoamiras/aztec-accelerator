# Phase 1 — Create `accelerator-core` + rewire GUI (ATOMIC PR)

Base: `feat/core-extraction-phase-1` off `256483b` (Phase 0 merged, #324). **SSH agent down all session → use
the gh HTTPS helper for fetch/push:** `git -c credential.helper='!gh auth git-credential' fetch|push https://github.com/alejoamiras/aztec-accelerator.git …`.

## Verified core dependency set (grep of `use` in the 9 core-bound modules)
**Into `core/Cargo.toml` (done):** axum, tokio(full), serde(derive), serde_json, parking_lot, base64, tempfile,
tracing, dirs, which, tower-http(cors,set-header), http, reqwest(stream,json), flate2, tar, hex, sha2, url, time.
dev: tower(util), tokio(test-util), serial_test.
**Deliberately EXCLUDED (GUI/TLS-only — grep confirmed 0 refs in core modules):** tauri, tauri-plugin-*,
tauri-build, rcgen, x509-parser, rustls-pemfile, **tokio-rustls**, **hyper**, **hyper-util** (only `tls.rs` uses
these), tracing-appender, tracing-subscriber (logging setup lives in the binaries), urlencoding (verified_sites/commands).

## Module moves (git mv from src-tauri/src/ → core/src/)
MOVE: `authorization.rs`, `bb.rs`, `config.rs`, `versions.rs`, `server.rs`, `server/auth.rs`, `server/bind.rs`,
`server/probe.rs`, `server/prove.rs`.
STAY in src-tauri/src/: `certs.rs`, `verified_sites.rs`, `server/tls.rs`, `commands.rs`, `updater.rs`,
`crash_recovery.rs`, `tray.rs`, `windows.rs`, `main.rs`, `lib.rs`, `build.rs`.
→ After: `src-tauri/src/server/` holds ONLY `tls.rs`; `core/src/server/` holds auth/bind/probe/prove.

## Edits
1. **core/src/lib.rs** (new): `pub mod authorization; pub mod bb; pub mod config; pub mod versions; pub mod
   server;` + move `log_dir()` here (headless uses it; GUI re-exports). NO certs/verified_sites/commands/etc.
2. **core/src/server.rs**: REMOVE `mod tls; pub use tls::start_https;` (tls stays GUI). Keep `mod bind;` but
   change `use bind::bind_with_retry;` → `pub use bind::bind_with_retry;` (public seam for the GUI adapter).
   Keep `mod {auth,probe,prove};`, `pub use probe::healthy_aztec_on_port;`, `pub const HTTPS_PORT`.
   bb-version: `unwrap_or(env!("AZTEC_BB_VERSION"))` (health + via prove) → `unwrap_or(DEFAULT_BB_VERSION)` with
   a new `pub const DEFAULT_BB_VERSION: &str = "unknown";` (matches src-tauri/build.rs else-branch). `/health.version`
   keeps `…unwrap_or(env!("CARGO_PKG_VERSION"))` — `CARGO_PKG_VERSION` is always set (resolves to core's version,
   only the injected-app_version fallback), so it COMPILES; the injected value wins in prod.
3. **core/src/server/prove.rs**: same `env!("AZTEC_BB_VERSION")` → `DEFAULT_BB_VERSION` (import from `super`).
4. **core/src/server/bind.rs**: `pub(crate) fn bind_with_retry` → `pub fn bind_with_retry`.
5. **src-tauri/src/server.rs** (NEW thin wrapper): `pub use accelerator_core::server::*;` + `mod tls; pub use
   tls::start_https;`. Keeps `aztec_accelerator::server::{router,start_https,AppState,HTTPS_PORT,…}` paths STABLE.
6. **src-tauri/src/server/tls.rs**: `use super::{router, AppState, HTTPS_PORT};` + `use super::bind::bind_with_retry;`
   → `use accelerator_core::server::{router, AppState, HTTPS_PORT, bind_with_retry};`.
7. **src-tauri/src/lib.rs**: `pub use accelerator_core::{authorization, bb, config, versions, log_dir};` +
   `pub mod server;` (the wrapper) + keep `pub mod {certs, commands, crash_recovery, updater, verified_sites};`.
8. **src-tauri/Cargo.toml**: add `accelerator-core = { path = "../core" }`. (Leave existing deps — GUI still uses
   most via certs/tls/commands; trimming unused ones is optional polish, not required for compile.)
9. **server/Cargo.toml + server/src/main.rs**: NOT touched in Phase 1 (headless still via src-tauri → repointed
   in Phase 2). BUT to keep headless `/health.aztec_version` truthful once core's default is "unknown", set
   `bundled_version: std::env::var("AZTEC_BB_VERSION").ok()` in server/main.rs (runtime env; CI can export it).
   [If deferred, headless reports aztec_version="unknown" until Phase 3 — CI-only, e2e passes explicit
   x-aztec-version so proving is unaffected. Decide at execution.]
10. **.github/workflows/accelerator.yml**: add a `core` clippy + `cargo test -p accelerator-core` job (current CI
    only tests src-tauri — final-codex condition).
11. build.rs UNCHANGED (still emits AZTEC_BB_VERSION for GUI tray/main, checks verified-sites.json, tauri_build).

## Validation gate (before commit — ATOMIC, all must pass)
- `cargo test -p accelerator-core` (moved tests run here, incl. health_reports_injected_app_version).
- `cargo build`/`cargo test` src-tauri (GUI compiles via the wrapper; WebDriver/e2e unaffected).
- `cargo build` headless server (still via src-tauri in Phase 1).
- `cargo fmt` + clippy clean. `bun run lint:actions` for the workflow change.

## Attempts
- Scaffold: `core/Cargo.toml` written (dep set above).
- **Result: GREEN.** Executed the spec verbatim; no surprises.
  - `git mv` the 9 modules → `core/src/`; new `core/src/lib.rs`; removed `mod tls`/`start_https` from core's
    server.rs; `use bind::bind_with_retry` → `pub use`; added `pub const DEFAULT_BB_VERSION = "unknown"`;
    swapped both `env!("AZTEC_BB_VERSION")` → `DEFAULT_BB_VERSION` (server.rs) / `super::DEFAULT_BB_VERSION`
    (prove.rs); `bind_with_retry` `pub(crate)`→`pub`.
  - GUI: new thin `src-tauri/src/server.rs` wrapper (`pub use accelerator_core::server::*;` + `mod tls; pub use
    tls::start_https;`); `lib.rs` re-exports `accelerator_core::{authorization,bb,config,log_dir,versions}` +
    keeps certs/commands/crash_recovery/updater/verified_sites; `tls.rs` imports from `accelerator_core::server`;
    `src-tauri/Cargo.toml` += `accelerator-core = { path = "../core" }`. **main/tray/windows/commands edit-free**
    (the wrapper kept paths stable — exactly as both contradiction-checks required).
  - **Validation (all green):** `cargo test` core → **113 passed**; src-tauri build + `cargo test --lib` → **16
    passed**; headless `cargo build` → ok; `cargo clippy` core + src-tauri → clean; `cargo build --features
    webdriver` → ok; `cargo fmt` applied; `bun run lint:actions` → clean.
  - **Dep win visible already:** core's standalone tree has NO `tokio-rustls`/`rcgen` (TLS-serving/cert-gen);
    `reqwest`→native-tls/hyper stays (bb download) — matches the corrected Phase-2 `cargo tree` assertion.
  - CI: extended the Clippy + Rust Tests jobs to fmt/clippy/`cargo test` the standalone core crate (+ core build cache).
  - **Deferred to Phase 2/3 (documented):** headless `/health.aztec_version` now defaults to `"unknown"` (was the
    baked `env!`) until the copy-bb.ts hook injects `bundled_version`. CI-only, informational; `/prove` is
    unaffected (callers pass `x-aztec-version`). server/main.rs intentionally untouched (clean phase boundary).
