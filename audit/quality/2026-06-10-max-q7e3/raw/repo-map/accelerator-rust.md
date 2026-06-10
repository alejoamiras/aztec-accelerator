# Repo map — Rust crates (`packages/accelerator/{core, src-tauri, server}`)

Phase-1 mapper output for the 2026-06-10 quality (maintainability) audit. Paths relative to repo root.
Total production Rust: ~7,862 LOC across 25 files (incl. in-file `#[cfg(test)]` blocks).

Key structural fact: a **core extraction** landed 2026-06-07 (PR #325, `core-extraction-2026-06-07` plan).
`bb.rs`, `config.rs`, `authorization.rs`, `versions*`, and the whole HTTP server moved from `src-tauri`
into the new `accelerator-core` lib crate; `src-tauri/src/lib.rs` re-exports them so old
`aztec_accelerator::…` paths still compile. The three crates are deliberately **NOT a Cargo workspace**
(each has its own `Cargo.lock`) to prevent feature-unification dragging Tauri/rustls/rcgen into the
headless build — documented in all three `Cargo.toml`s; do not "fix" this in the audit.

---

## 1. Module inventory

### `packages/accelerator/core` — lib crate `accelerator-core` (Tauri-free core)

| Path | LOC | Purpose |
|---|---|---|
| `core/src/lib.rs` | 27 | Crate root: declares `authorization`/`bb`/`config`/`server`/`versions` + `log_dir()` helper. |
| `core/src/server.rs` | 1424 | Axum server hub: ports/consts, `ServerStatus`, callback type aliases, `HeadlessState`/`AppState` (+constructors), `start`, `router`/`router_for_port` (CORS, body limit, Host guard), `/health` handler + SEC-05 tiering, `json_error`. **Tests start line 330** → ~1,094 LOC (77%) is the in-file test module. |
| `core/src/server/bind.rs` | 124 | `bind_with_retry`: TCP bind with AddrInUse retry budget (updater-restart overlap vs genuine second instance). |
| `core/src/server/probe.rs` | 74 | `healthy_aztec_on_port`: classify a lost `:59833` bind — redundant own instance vs foreign squatter. |
| `core/src/server/host.rs` | 135 | SEC-01a loopback `Host`/`:authority` allowlist middleware (`host_is_trusted` + `guard`) — anti-DNS-rebinding. |
| `core/src/server/auth.rs` | 141 | `/prove` origin authorization: approved-list check, popup-gating w/ 60s timeout, remember-persist into config. |
| `core/src/server/prove.rs` | 236 | `/prove` handler: authorize → buffer body (50MB) → semaphore → `resolve_version` → optional bb download → `bb::prove` → base64 + duration header; `StatusGuard` drop-resets tray status. |
| `core/src/authorization.rs` | 613 | RFC-6454 `canonicalize_origin`, `CanonicalOrigin` newtype (parse-don't-validate, strict serde), `AuthDecision`, `AuthorizationManager` (pending-request map, request-id resolve SEC-06, localhost auto-approve). Tests from line 280. |
| `core/src/config.rs` | 389 | `Speed` enum (+thread mapping), `AcceleratorConfig` schema + lenient `de_approved_origins`, `load()`, atomic 0o600 `save()`. Tests from line 168. |
| `core/src/bb.rs` | 279 | `find_bb` 5-step search chain (env override → version cache → sidecar → `~/.bb` → PATH), `prove()` subprocess runner (tempdir, 5-min timeout, kill_on_drop, stderr truncation), field-count header prepend. Tests from line 172. |
| `core/src/versions/mod.rs` | 1006 | Version identity & cache policy: `NetworkTier`, `AztecVersion` value object (Q3 traversal guard), platform/url/path helpers, `versions_to_evict` retention, `list_cached_versions`, `is_valid_version`, `cleanup_old_versions`, GitHub digest fetch. **Tests from line 380** → ~626 LOC (62%) test. |
| `core/src/versions/downloader.rs` | 363 | `download_bb` pipeline: bounded-stream download (64MB) → fail-closed digest verify → `install_version_dir` (tmp+rename) → macOS Gatekeeper finalize; `CappedReader` gzip-bomb cap (SEC-07). Tests from line 303. |

### `packages/accelerator/src-tauri` — bin crate `aztec-accelerator` (Tauri GUI shell)

| Path | LOC | Purpose |
|---|---|---|
| `src-tauri/src/lib.rs` | 15 | Re-exports core modules (`authorization,bb,config,log_dir,versions`) + declares GUI-only modules. |
| `src-tauri/src/main.rs` | 500 | Binary entry: logging (file appender), config/auth state, Tauri builder + plugins, tray wiring, callbacks (`on_status`, versions-changed, auth-popup), HTTPS startup (`try_start_https`/`reset_safari_support`), HTTP spawn w/ AddrInUse classification, update poller, exit handling. Tests from line 480. |
| `src-tauri/src/server.rs` | 24 | Thin shim: `pub use accelerator_core::server::*` + `mod tls` + `spawn_https` wrapper (F-09). |
| `src-tauri/src/server/tls.rs` | 73 | `start_https`: bind-retry on 59834, `https_bound` flag, manual TLS accept loop via tokio-rustls + hyper-util. |
| `src-tauri/src/commands.rs` | 276 | All 12 Tauri commands + state type aliases (`ConfigState`, `AuthState`, `VerifiedSitesState`, `SharedAppState`, `PendingUpdate`), `mutate_config` lock-mutate-save helper, `sanitize_window_label`. |
| `src-tauri/src/certs.rs` | 632 | Safari HTTPS cert surface: `CertPaths` trio (live/staged/swap), keyless-CA generate/rotate (stage→trust→verify→swap), `migrate_legacy_ca_key` (SEC-08 fail-closed), atomic `write_pem_file`, rustls config load, leaf-expiry calc, macOS `security` trust subcommands. Tests from line 449. |
| `src-tauri/src/crash_recovery.rs` | 493 | `CrashRecovery` trait + `PlatformRecovery` ZST; per-OS impls: macOS plist KeepAlive patch, Linux systemd user unit, Windows Task Scheduler repeating-trigger task (UTF-16 XML, verified delete). Tests from line 403. |
| `src-tauri/src/updater.rs` | 237 | Auto-update: `check_for_update` (pref-driven), `perform_update` (SEC-03 pre-flight size cap, Windows disarm/re-arm crash recovery around install), `size_from_feed`. Tests from line 193. |
| `src-tauri/src/verified_sites.rs` | 211 | Embedded `verified-sites.json` registry (compile-time include), strict validation (ASCII, canonical, dedupe), `lookup` for popup badge. Tests from line 128. |
| `src-tauri/src/tray.rs` | 172 | Tray menu/submenu building, icon construction, 24-frame 20fps animation loop (`include_bytes!` PNGs). No tests. |
| `src-tauri/src/windows.rs` | 153 | Window management: `WindowConfig` + `open_or_focus_window` dedupe, Settings/auth-popup (60s auto-deny timeout)/update-prompt windows. No tests. |
| `src-tauri/build.rs` | 22 | Build script: `AZTEC_VERSION` → `AZTEC_BB_VERSION` env, `verified-sites.json` syntax check, `tauri_build::build()`. |

### `packages/accelerator/server` — bin crate `accelerator-server` (headless)

| Path | LOC | Purpose |
|---|---|---|
| `server/src/main.rs` | 243 | Headless entry: `--version`, stdout tracing init, SEC-01c origin-gating resolution (`Gating` enum, `resolve_gating`, `parse_allowed_origins_env`), builds `HeadlessState`/`AppState`, calls core `start`. Tests from line 152. |

---

## 2. Public surface

### `accelerator-core` pub exports (via `core/src/lib.rs`)
- `pub mod authorization` — `canonicalize_origin`, `CanonicalOrigin` (+`NonCanonicalOrigin`), `AuthDecision`, `AuthorizationManager` (`new/request/resolve/is_auto_approved/is_approved`)
- `pub mod bb` — `find_bb`, `prove`
- `pub mod config` — `Speed`, `AcceleratorConfig`, `config_path`, `load`, `save`
- `pub mod server` — `start`, `router`, `router_for_port`, `AppState` (`headless`/`desktop`), `HeadlessState` (`headless`), `ServerStatus`, `StatusCallback`/`VersionsChangedCallback`/`ShowAuthPopupCallback`, `bind_with_retry`, `healthy_aztec_on_port`, consts `HTTPS_PORT`/`DEFAULT_BB_VERSION`/`AUTH_DECISION_TIMEOUT` (HTTP `PORT=59833` is private)
- `pub mod versions` — `NetworkTier`, `AztecVersion`, `download_bb`, `versions_base_dir`, `bb_binary_name`, `version_bb_path`, `current_platform`, `download_url`, `versions_to_evict`, `list_cached_versions`, `is_valid_version`, `cleanup_old_versions`
- `pub fn log_dir()`

`src-tauri/src/lib.rs` re-exports all of the above + adds `certs`, `commands`, `crash_recovery`, `server` (shim adding `start_https`/`spawn_https`), `updater`, `verified_sites`.

### Binary entrypoints
- `aztec-accelerator` → `src-tauri/src/main.rs::main` (Tauri `.setup` closure does the real wiring; `autobins=false`, single `[[bin]]`)
- `accelerator-server` → `server/src/main.rs::main` (tokio main)

### HTTP routes (both listeners, defined once in `core/src/server.rs::router_for_port`)
- `GET /health` — SEC-05 Origin-tiered body (detailed only for absent/approved Origin)
- `POST /prove` — origin auth → semaphore → version resolve/download → bb prove
- Layers (inner→outer): 50MB `DefaultBodyLimit` → permissive CORS (GET/POST; `x-aztec-version` allowed, `x-prove-duration-ms` exposed) → CORP header → SEC-01a loopback-Host guard (outermost)
- Listeners: HTTP `127.0.0.1:59833` (both binaries), HTTPS `127.0.0.1:59834` (desktop-only, Safari opt-in)

### Tauri commands (12, registered in `main.rs::invoke_handler`, defined in `commands.rs`)
`get_config`, `get_autostart_enabled`, `set_autostart`, `set_speed`, `remove_approved_origin`,
`get_system_info`, `get_verified_info`, `respond_auth`, `enable_safari_support` (macOS real / stub elsewhere),
`disable_safari_support` (same split), `set_auto_update`, `respond_update_prompt`.

---

## 3. Dependency graph (one level deep, production code)

### core (internal)
- `server.rs` → `crate::{authorization, bb, config, versions}`; declares submodules `bind`, `probe`, `auth`, `host`, `prove`
- `server/auth.rs` → `crate::{authorization, config}`, `super::{json_error, AppState, ProveError, AUTH_DECISION_TIMEOUT}`
- `server/prove.rs` → `crate::{bb, versions}`, `super::auth::authorize_origin`, `super::{json_error, AppState, ProveError, ServerStatus, StatusCallback}`
- `server/probe.rs` → `super::PORT`
- `server/bind.rs`, `server/host.rs` → no crate-internal imports (leaf modules)
- `bb.rs` → `crate::versions`
- `config.rs` → `crate::authorization::CanonicalOrigin`
- `versions/downloader.rs` → `super::{…helpers, AztecVersion}`
- `authorization.rs`, `lib.rs` → leaf

### src-tauri (GUI)
- `lib.rs` → re-export `accelerator_core::{authorization, bb, config, log_dir, versions}`
- `main.rs` → `aztec_accelerator::{authorization, commands, server, certs, config, log_dir, verified_sites}` (+ body refs: `crash_recovery`, `updater`, `bb`, `server::spawn_https`), local `mod tray`, `mod windows`
- `commands.rs` → `crate::{authorization, config, verified_sites}` (+ body: `crate::{crash_recovery, updater, certs, server::spawn_https, server::AppState}`)
- `server.rs` → `accelerator_core::server::*`, `mod tls`
- `server/tls.rs` → `accelerator_core::server::{bind_with_retry, router_for_port, AppState, HTTPS_PORT}`
- `updater.rs` → `crate::commands::ConfigState` (+ body: `crate::crash_recovery`)
- `verified_sites.rs` → `crate::authorization::canonicalize_origin`
- `tray.rs` → `aztec_accelerator::versions`
- `windows.rs` → `aztec_accelerator::{authorization, commands}` (+ body: `server::AUTH_DECISION_TIMEOUT`)
- `certs.rs`, `crash_recovery.rs` → no crate-internal imports (leaf; external deps only)

### server (headless)
- `main.rs` → `accelerator_core::{authorization, config, server}` only

No cycles. Cross-crate flow: `server` → `core`; `src-tauri` → `core` (direct + via re-export). Note `tray.rs`/`windows.rs` import via the **public crate path** (`aztec_accelerator::…`) while sibling modules use `crate::…` — two idioms for the same crate.

---

## 4. Similarity candidates (pairs read & judged)

**Pre-checked pairs from the brief — mostly resolved by the 2026-06-07 core extraction:**

1. ~~`src-tauri/src/bb.rs` vs `core/src/bb.rs`~~ — **NOT a live pair.** src-tauri's `bb.rs` was deleted in PR #325; `lib.rs` re-exports core's. Its presence in hotspot data is pre-deletion churn.
2. ~~`src-tauri/src/versions.rs` vs `core/src/versions/*`~~ — **NOT a live pair.** Same deletion; versions logic lives solely in core (`mod.rs` + `downloader.rs`).
3. `src-tauri/src/server.rs` + `server/tls.rs` vs `core/src/server.rs::start` — **LOW residual duplication.** `start()` (HTTP) and `start_https` share the bind-retry → log-listening → serve shape, but HTTPS needs a manual `TlsAcceptor` + hyper-util accept loop where HTTP uses `axum::serve`; divergence is essential, shared parts (`bind_with_retry`, `router_for_port`) already extracted. Not worth unifying.
4. `core/src/server*` gating vs `server/src/main.rs` — **DIFFERENT CONCERNS.** Headless main owns CLI/env *gating resolution* (`resolve_gating`, `parse_allowed_origins_env`); core owns request-time enforcement (`server/auth.rs`). No logic overlap. The real overlap is pair #6 below (binary bootstrap).
5. `core/src/config.rs` vs src-tauri usage — config lives in core; src-tauri only consumes. But see pair #7 (lock-mutate-save).

**Live candidates found by reading:**

6. **`server/src/main.rs::main` vs `src-tauri/src/main.rs::main` (binary bootstrap)** — both init a `tracing_subscriber::registry()+EnvFilter+fmt` stack (desktop adds the file layer), both construct `AcceleratorConfig`→`Arc<RwLock>`→`AuthorizationManager`→`HeadlessState::headless(env!("CARGO_PKG_VERSION"), …)`→`AppState`, both wrap `server::start` errors (headless: log+exit(1); desktop: classify AddrInUse → tray text). ~30 lines of parallel-evolving boilerplate; moderate — divergences are real (file logging, tray, exit-0-if-healthy) but the shared spine could live in core.
7. **Lock-mutate-save config pattern × 3 sites** — `src-tauri/commands.rs::mutate_config` (the named helper, used by 5 commands), `src-tauri/main.rs::reset_safari_support` (manual `cfg.write(); cfg.safari_support=false; config::save`), and `core/src/server/auth.rs` Allow-remember arm (manual `cfg.write(); push; config::save` with warn-on-error). The helper exists but two sites (one cross-crate) bypass it. Concrete, small, real.
8. **Atomic write-tmp-rename idiom × 3 implementations** — `core/config.rs::save` (tmp file + 0o600 + rename, no fsync), `src-tauri/certs.rs::write_pem_file` (tmp + 0o600 + **fsync** + rename), `core/versions/downloader.rs::install_version_dir` (tmp **dir** + rename). Three hand-rolled atomic-write routines with subtly different durability guarantees; plus the 0o700-parent-dir chmod idiom repeated in `main.rs` (log dir), `config.rs::save`, `certs.rs::generate_certs`. Strongest cross-module duplication in the codebase.
9. **Loopback-literal matching × 3** — `core/server/host.rs::host_is_trusted` (`"127.0.0.1"|"localhost"|"::1"`, brackets stripped), `core/authorization.rs::is_auto_approved` (`"localhost"|"127.0.0.1"|"[::1]"`, brackets KEPT), plus `canonicalize_origin`'s own host normalization (lowercase + trailing-dot trim, also re-done in host.rs). Three near-identical normalize+match sequences in two files whose literal sets could silently drift (note the `[::1]` vs `::1` representational difference is load-bearing and undocumented at the `is_auto_approved` site).
10. **Popup-window close-by-label × 3** — `commands.rs::respond_auth` (auth window), `commands.rs::close_update_prompt`, `windows.rs` timeout closure: each does `app.get_webview_window(label)` → `window.close()`. Trivial but tri-plicated; label format `auth-{hash}` is constructed independently in `commands.rs::respond_auth` AND `windows.rs::show_auth_popup_window` — a format-string drift risk between files.
11. **`/health` minimal-body contract coupling** — `core/server/probe.rs::is_healthy_aztec_response` pins `{status:"ok", api_version:1}` which must match the SEC-05 minimal body in `server.rs::health`; the two literals live in different files with no shared const.
12. **macOS `security` subprocess wrappers** (`certs.rs`: `add_trusted_cert`/`verify_cert_trusted`/`ca_keychain_sha1`/`remove_trusted_cert_by_sha1`) and **`schtasks`/`systemctl` wrappers** (`crash_recovery.rs`) — same Command→output→status/stderr-log shape ~8×; intra-file repetition, not cross-module.

---

## 5. Frameworks + house conventions

- **Stack:** axum 0.8 + tokio (full) + tower-http (cors, set-header) + hyper/hyper-util (TLS path only); reqwest (downloads/probe); parking_lot (`RwLock`/`Mutex` everywhere — never std sync); serde/serde_json; tracing (+tracing-appender/subscriber in binaries); rcgen/x509-parser/tokio-rustls/rustls-pemfile (certs, GUI-only); flate2+tar (tarballs); sha2/hex/base64; uuid v4 (request ids); `url::Url` for origin parsing; tempfile; dirs; which.
- **Tauri 2** plugins: autostart, updater, process, webdriver (feature-gated `webdriver` for E2E builds — gates the update poller and opens Settings at startup).
- **Comment idiom:** heavy "why"-comments carrying audit finding tags — `SEC-xx` (security findings, e.g. SEC-01a/01c/03–08), `Q-xx` (quality findings Q1–Q14), `F-xx` (F-01–F-09), `R-xx`, `PR-n`, plus `(codex M1/L3)` attributions and plan-folder references (`implementations-plan/...`). Doc comments often pin behavioral contracts to named characterization tests ("pinned by `prove_error_responses_stay_text_plain`"). Expect ANY refactor to preserve/update these tags.
- **Error idiom:** NO `thiserror`/`anyhow` anywhere. Two-tier: `Box<dyn std::error::Error + Send + Sync>` for core/internal fallible fns (~31 uses), `Result<(), String>` for Tauri command boundaries (~13 uses, serialized to the webview). HTTP errors are `(StatusCode, String)` (`ProveError`) with `json_error()` building a `text/plain` JSON string (deliberately NOT `axum::Json` — Content-Type pinned by test). One bespoke error type: `NonCanonicalOrigin`. Infallible-by-policy paths use `tracing::warn!/error!` + best-effort `let _ =`.
- **Newtype/parse-don't-validate idiom:** `CanonicalOrigin` and `AztecVersion` are validated-by-construction value objects with `Deref<Target=str>`; sinks take the typed value so ingress validation is structural.
- **Test placement:** in-file `#[cfg(test)] mod tests` at the bottom of every tested module (no `tests/` integration dirs in any crate). Router tests use `tower::ServiceExt::oneshot`; env-mutating tests use `serial_test` (`core/bb.rs`, `core/server.rs`); injectable-timing inner fns (`bind_with_retry_inner`, `extract_bb_from_tarball_capped`, `migrate_legacy_ca_key_at`) are the standard testability seam.
- **Platform idiom:** `#[cfg(target_os = …)]` fn pairs with non-target stubs (certs trust mgmt, crash_recovery, `finalize_downloaded_binary`); `crash_recovery` uses a trait + ZST dispatch instead.

## 6. Test surfaces

- **Rust in-file unit tests** (grep of `#[test]`/`#[tokio::test]`): core ≈ 131 fns (server.rs 31, authorization 30, versions/mod 28, config 21, bb 8, host 6, bind 3, downloader 3, probe 1); src-tauri ≈ 23 fns (verified_sites 7, certs 7, crash_recovery 4, main 3, updater 2); server ≈ 11 fns. Total ≈ 165. Untested modules: `tray.rs`, `windows.rs`, `commands.rs`, `server/tls.rs`, both `lib.rs`, src-tauri `server.rs` shim.
- **Playwright UI mock tests** (28): `packages/accelerator/e2e/{authorize,settings,update-prompt}.spec.ts` + `tauri-mock.js` (EXCLUDED surface — note only).
- **WebDriver E2E** (9, macOS+Linux PR gate): `packages/accelerator/e2e-webdriver/{smoke,settings,auth-flow}.spec.ts` + `helpers.ts` (EXCLUDED).
- **Script tests:** `packages/accelerator/scripts/copy-bb.test.ts` (bun:test) + updater/crash-recovery smoke scripts (`*.sh`/`*.ps1`) (EXCLUDED).
- TS E2E for the whole proving path lives outside this package (`_e2e.yml` reusable workflow).

## 7. Generated / vendored / fixture paths to exclude

- `packages/accelerator/src-tauri/target/`, `packages/accelerator/server/target/`, `packages/accelerator/core/target/` — build output
- `packages/accelerator/src-tauri/gen/` — Tauri-generated schemas
- `packages/accelerator/src-tauri/Cargo.lock`, `server/Cargo.lock`, `core/Cargo.lock` — lockfiles
- `packages/accelerator/src-tauri/icons/` + `src-tauri/ui/` assets (PNGs/HTML — not Rust; `ui/` HTML/JS is the webview frontend, separate audit surface)
- `packages/accelerator/test-results/` — Playwright artifacts
- `packages/accelerator/e2e/`, `e2e-webdriver/`, `scripts/` — test surfaces only (per brief)
- `verified-sites.json` / `verified-sites.schema.json` — data files (validated by build.rs + verified_sites.rs tests)
- `src-tauri/AZTEC_VERSION` — build-input data file
- No Rust fixture dirs exist; test data is inline in `#[cfg(test)]` blocks (e.g. hand-built tarballs in downloader tests)

## 8. Change hotspots (`git log --since=2026-05-25`, Rust files)

⚠️ Interpretation caveat: the core extraction (#325, 2026-06-07) RENAMED most paths mid-window. Counts on deleted `src-tauri` paths (`versions.rs` 8, `bb.rs` 4, `authorization.rs` 2, `config.rs` 1, `server/{prove,probe,bind,auth}.rs` 1 each, `core/src/versions.rs` 1) are pre-move churn on logic that NOW lives in core — fold them mentally into their core successors.

| Count | File | Temp | Note |
|---|---|---|---|
| 19 | `src-tauri/src/server.rs` | **HOT (historic)** | Churn hit the pre-extraction monolith; file is now a 24-line shim. Successor logic = `core/src/server*` |
| 14 | `src-tauri/src/main.rs` | **HOT** | Wiring hub; touched by nearly every feature/security PR |
| 10 | `src-tauri/src/commands.rs` | **HOT** | SEC-06/SEC-08/Q9 churn |
| 8 | `server/src/main.rs` | **HOT** | SEC-01c gating rework |
| 7 | `core/src/server.rs` | **HOT** | + inherits the 19 above; SEC-01a/05, F-01/07/08/09, #348 |
| 5 | `src-tauri/src/windows.rs` | WARM | SEC-06 request-id labels |
| 5 | `src-tauri/src/certs.rs` | WARM | SEC-08, keyless-CA rotation |
| 4 | `src-tauri/src/server/tls.rs` | WARM | Q2/Q7 extraction era |
| 4 | `core/src/server/auth.rs` | WARM | SEC-04/06 (+1 as src-tauri path) |
| 4 | `core/src/authorization.rs` | WARM | CanonicalOrigin/F-02 (+2 as src-tauri path) |
| 3 | `src-tauri/src/updater.rs` | WARM | SEC-03 size cap |
| 3 | `src-tauri/src/crash_recovery.rs` | WARM | Windows task work |
| 3 | `core/src/server/prove.rs` | WARM | F-08 status ownership |
| 3 | `core/src/config.rs` | WARM | SEC-04 auto_approve_localhost (+1 as src-tauri path) |
| 2 | `core/src/versions/{mod,downloader}.rs` | COLD-ish | SEC-07 cap was the last touch (+8 pre-move) |
| 1 | `core/src/server/host.rs` | COLD count, **NEW file** | Born 2026-06-09 in SEC-01a PR #338 — newest module |
| 1 | `core/src/{bb,lib}.rs`, `core/src/server/{bind,probe}.rs`, `src-tauri/src/verified_sites.rs`, `src-tauri/build.rs` | COLD | Stable since extraction |
| 0 | `src-tauri/src/tray.rs` | COLD | Untouched in window (also untested) |
