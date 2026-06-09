# Accelerator Rust — Repo Map (QUALITY audit, Phase 1)

Scope: the three Rust crates under `packages/accelerator/`. They are **three independent Cargo packages** (NOT one workspace — deliberate, so the headless server never feature-unifies Tauri/rustls/rcgen; see `core/Cargo.toml` header comment). Each has its own `Cargo.lock`.

- `accelerator-core` (`core/`) — GUI-agnostic lib: HTTP proving server, bb cache, version resolution, origin auth. `build.rs`-free.
- `aztec-accelerator` (`src-tauri/`) — Tauri desktop binary; depends on `core`, adds tray/updater/windows/commands/certs/crash-recovery/HTTPS.
- `accelerator-server` (`server/`) — thin headless binary; depends on `core` only.

Edition 2021, rust-version 1.77.2. All three pinned to version `1.0.5-rc.1`.

---

## 1. Crate / module inventory

### `core/` — `accelerator-core` (GUI-agnostic lib)

| Module | LOC | One-sentence purpose |
|---|---|---|
| `lib.rs` | 27 | Crate root: declares 4 public modules (`authorization`, `bb`, `config`, `server`, `versions`) and the single free fn `log_dir()`. |
| `versions.rs` | 1209 | bb version cache: `AztecVersion` value-object + validation, network-tier retention/eviction, GitHub download + SHA-256 verify + tarball extract + atomic install. **Largest file; mostly tests (~580 prod / ~630 test).** |
| `server.rs` | 1122 | Axum server core: `AppState`/`HeadlessState`, `ServerStatus` enum, callbacks, `router()`, `/health` handler, `start()`, `json_error`. Declares the 4 server submodules. **~210 prod / ~910 test.** |
| `config.rs` | 394 | `AcceleratorConfig` + `Speed` enum (serde), `load()`/`save()` (atomic temp+rename, 0o600), `migrate_approved_origins` canonicalization migration. |
| `authorization.rs` | 372 | `canonicalize_origin` (RFC 6454), `AuthDecision`, `AuthorizationManager` (pending-origin map, MAX_PENDING=10, auto-approve localhost). |
| `bb.rs` | 279 | `find_bb` (5-step search chain), `prove` (spawns `bb prove`, 5-min timeout, `kill_on_drop`), proof header prepend, stderr truncation. |
| `server/prove.rs` | 207 | `/prove` handler: authorize → buffer body (50MB) → semaphore → resolve+download version → `bb::prove` → base64. `StatusGuard` drop-resets tray. |
| `server/auth.rs` | 135 | `authorize_origin`: localhost/persisted auto-approve, popup-gate w/ 60s timeout, headless deny. |
| `server/bind.rs` | 124 | `bind_with_retry`: TCP bind with `AddrInUse` retry (100ms poll, 5s budget) + injectable-timing inner. |
| `server/probe.rs` | 74 | `healthy_aztec_on_port`: probe `:59833/health`, classify healthy-Aztec vs foreign squatter. |

### `src-tauri/` — `aztec-accelerator` (Tauri desktop binary)

| Module | LOC | One-sentence purpose |
|---|---|---|
| `lib.rs` | 15 | Crate root: re-exports `core::{authorization,bb,config,log_dir,versions}`; declares `certs`, `commands`, `crash_recovery`, `server`, `updater`, `verified_sites`. |
| `main.rs` | 495 | Binary entrypoint: logging init, rustls provider install, Tauri builder, tray/HTTPS/HTTP-server/update-loop wiring, exit handling. **Largest function in the crate (`setup` closure).** |
| `certs.rs` | 552 | TLS cert lifecycle: keyless-CA + leaf generation (rcgen), atomic PEM write (0o600), expiry-based rotation, macOS Keychain trust mgmt (`security` CLI), legacy ca.key migration. |
| `crash_recovery.rs` | 493 | Per-platform restart-on-crash: `CrashRecovery` trait + `PlatformRecovery` ZST; macOS plist patch, Linux systemd unit, Windows Task Scheduler XML. |
| `commands.rs` | 260 | 12 Tauri `#[command]` fns (config/autostart/speed/origin/system-info/verified-info/auth/safari/auto-update/update-prompt), `mutate_config` helper, state type aliases. |
| `verified_sites.rs` | 211 | `VerifiedSitesRegistry`: embedded `verified-sites.json` registry (recognition-badge UX, **NOT a security boundary**), canonicalized lookup. |
| `tray.rs` | 172 | Tray icon: menu building (dev vs prod), Versions submenu, embedded PNG icons, 20fps proving-animation loop. |
| `windows.rs` | 142 | Window mgmt: `open_or_focus_window` helper + Settings / auth-popup / update-prompt window builders; auth-popup 60s deny timeout. |
| `updater.rs` | 130 | Auto-update: `check_for_update` (honors auto_update pref), `perform_update` (download → Windows disarm-recovery → install → restart). |
| `server/tls.rs` | 71 | `start_https`: GUI-only HTTPS adapter over `core::server::router` using `tokio_rustls` + `hyper_util`. |
| `server.rs` | 9 | Thin re-export shim: `pub use accelerator_core::server::*;` + `pub use tls::start_https`. |
| `build.rs` | 23 | Injects `AZTEC_BB_VERSION` env from `AZTEC_VERSION` file, syntax-checks `verified-sites.json`, calls `tauri_build::build()`. |

### `server/` — `accelerator-server` (headless binary)

| Module | LOC | One-sentence purpose |
|---|---|---|
| `main.rs` | 81 | `--version` flag, tracing init, `ALLOWED_ORIGINS` env → optional auth gate, builds `AppState{HeadlessState}` (bb version from `AZTEC_BB_VERSION` env), calls `core::server::start`. |

---

## 2. Entrypoints

### HTTP routes (defined once in `core/src/server.rs:138-148` `router()`)
- `GET /health` → `core::server::health` (`server.rs:150`). Reports status/api_version/version/aztec_version/available_versions/bb_available, conditional `https_port`, debug-only `runtime`.
- `POST /prove` → `core::server::prove::prove` (`server/prove.rs:117`). 50MB body limit, CORS (`Any` origin), CORP header.

Both routes are served by **three** listeners over the *same* router:
- HTTP `127.0.0.1:59833` — `core::server::start` (`server.rs:119`), used by all crates.
- HTTPS `127.0.0.1:59834` — `src-tauri::server::tls::start_https` (`server/tls.rs:15`), GUI-only.

### Tauri `#[command]` fns (registered in `main.rs:246-259` `generate_handler!`)
All in `src-tauri/src/commands.rs` unless noted:
- `get_config` (:31), `get_autostart_enabled` (:36), `set_autostart` (:42), `set_speed` (:56), `remove_approved_origin` (:64), `get_system_info` (:78), `get_verified_info` (:95), `respond_auth` (:105), `enable_safari_support` (:139 macOS / :182 stub), `disable_safari_support` (:173 macOS / :189 stub), `set_auto_update` (:195), `respond_update_prompt` (:205).

### `main()` entrypoints (the two-binary pattern)
- `src-tauri/src/main.rs:181` — desktop `fn main()` (sync; Tauri owns the runtime). Largest setup closure (`:260-462`).
- `server/src/main.rs:20` — headless `#[tokio::main] async fn main()`.

### Background tasks / threads
- **HTTP server task** — `main.rs:400` `tauri::async_runtime::spawn` running `server::start`; classifies `AddrInUse`, Windows redundant-instance bow-out via `healthy_aztec_on_port`.
- **HTTPS server task** — `main.rs:85` (in `try_start_https`) + `commands.rs:160` (on Safari-enable). Accept-loop in `tls.rs:40`, spawning a task per connection (`tls.rs:52`).
- **Background leaf-renewal thread** — `main.rs:94` `std::thread::spawn` → `certs::regenerate_leaf_if_expiring` (off the startup path so the trust prompt can't block launch).
- **Update poller loop** — `main.rs:452` spawn; 5s initial delay then 12h loop calling `run_update_check`. `#[cfg(not(feature="webdriver"))]` + runtime-gated by `should_poll_for_updates`.
- **Tray animation loop** — `tray.rs:140` `start_animation_loop`, 50ms interval, fire-and-forget no-shutdown.
- **Auth-popup deny-timeout tasks** — `windows.rs:107` (per popup) and inside `core/server/auth.rs:88` (the request-side 60s timeout). Plus download-cleanup spawn in `prove.rs:75`.

### Public exports re-exported from `core`'s `lib.rs`
`lib.rs` exposes modules `authorization`, `bb`, `config`, `server`, `versions` + `log_dir()`. `src-tauri/lib.rs:8` re-exports `{authorization, bb, config, log_dir, versions}` so `aztec_accelerator::…` paths stay stable; `src-tauri/server.rs` re-exports all of `core::server::*`.

---

## 3. Trust boundaries

### Untrusted input ingress
- **HTTP `Origin` header** → `core/server/auth.rs:23` → `authorization::canonicalize_origin` (`authorization.rs:21`, RFC-6454 parse, rejects path/query/userinfo/unknown-scheme). The canonicalization-at-ingress is the security boundary.
- **`x-aztec-version` header** → `core/server/prove.rs:154` → `versions::AztecVersion::parse` (`versions.rs:85`) → `is_valid_version` (`versions.rs:331`, the **#99 path-traversal guard**: rejects `..`, leading/trailing dot, non-alnum, >128 chars). Validation-as-constructor pattern: only a parsed `&AztecVersion` reaches the cache-path/download-URL/`remove_dir_all` sinks.
- **HTTP `/prove` body** — `prove.rs:128` `to_bytes` capped at 50MB; oversized → 413.
- **`ALLOWED_ORIGINS` env** (headless) — split/trim into `approved_origins` (`server/main.rs:43`).
- **`AZTEC_BB_VERSION` / `BB_BINARY_PATH` env** — `server/main.rs:71`, `bb.rs:20` (explicit binary override — a trust hole if env is attacker-controlled, but that's a local-user threat model).
- **Config file** `~/.aztec-accelerator/config.json` — `config.rs:87` `load()`; malformed → defaults; runs origin canonicalization migration.
- **`verified-sites.json`** (embedded at compile time via `include_str!`, `verified_sites.rs:20`) — parsed in `try_load` (`:83`); rejects non-ASCII origins, dupes, bad displayName. Build-time syntax check in `build.rs:18`.
- **Tarball bytes** from GitHub — `versions.rs:527` `extract_bb_from_tarball` rejects non-regular-file (symlink) entries (`:543`); bounded 64MB streaming download (`:426`).
- **bb subprocess stderr/stdout** — `bb.rs:127`; stderr truncated to 500 chars (`truncate_stderr`), generic error returned to HTTP client (no path/witness leak).
- **Tauri command args** (`origin`, `action`, `enabled`, …) — cross the webview→Rust boundary in `commands.rs`.

### Secret / key handling
- **TLS CA private key** — `certs.rs:101-117` `write_new_cert_set`: CA key generated in-memory, signs leaf, **dropped/never written to disk** (closes an audit HIGH); `rcgen` built with `zeroize` feature to scrub on drop. `migrate_legacy_ca_key` (`certs.rs:145`) deletes any legacy on-disk `ca.key`.
- **Leaf cert + key** — written via `write_pem_file` (`certs.rs:167`) atomically with 0o600 perms; loaded into rustls in `load_rustls_config` (`certs.rs:202`).
- **macOS Keychain trust** — `security add-trusted-cert` / `verify-cert` / `find-certificate -Z` / `delete-certificate` subprocesses (`certs.rs:313-373`).
- **minisign / Ed25519 updater key** — NOT in Rust source; the updater public key lives in `tauri.conf.json`; signature verification is inside `tauri-plugin-updater` (`updater.rs:103` `update.install`).

### External calls (network + subprocess spawn)
- **Network**: GitHub release tarball download (`versions.rs:430`), GitHub API digest fetch (`versions.rs:291`), redundant-instance health probe (`probe.rs:26` reqwest to localhost), updater feed (via plugin).
- **Subprocess spawn**: `bb prove` (`bb.rs:118`), macOS `xattr`/`codesign` on downloaded binary (`versions.rs:385,397`), macOS `security` (certs), Linux `systemctl` (crash_recovery), Windows `schtasks.exe` (crash_recovery, absolute-path-resolved `:246`), `open`/`xdg-open`/`explorer` for browser (`main.rs:35`).

---

## 4. Dependency graph (one level)

```
server/main.rs ───────────────► accelerator-core  (authorization, config, server::{start,AppState,HeadlessState})
                                       ▲
                                       │ (path dep, core only — NO tauri/rustls/rcgen)
src-tauri/  ───────────────────────────┤
  main.rs ─► lib re-exports, server, certs, commands, crash_recovery, updater, verified_sites, tray, windows
  server.rs (shim) ─► core::server::*  + tls::start_https
  server/tls.rs ─► core::server::{bind_with_retry, router, AppState, HTTPS_PORT}
  certs.rs ─► rcgen, tokio-rustls, x509-parser, rustls-pemfile  (GUI-only TLS stack)
  commands.rs ─► core::{authorization, config}, verified_sites, certs, server::AppState, updater, crash_recovery
  windows.rs ─► core::{authorization, server::AUTH_DECISION_TIMEOUT}, commands::sanitize_window_label
  tray.rs ─► core::versions
  updater.rs ─► commands::ConfigState, crash_recovery
```

**core internal**: `server.rs` (parent) → `server/{prove,auth,bind,probe}`. `prove.rs` → `bb`, `versions`, `auth`. `auth.rs` → `authorization`, `config`. `versions.rs` → (leaf, only external crates). `config.rs` → `authorization::canonicalize_origin`. `bb.rs` → `versions`. `authorization.rs` → (leaf, `url`).

**core↔src-tauri boundary** — what core exposes vs what src-tauri layers on:
- Core exposes: the entire HTTP server + router + `/health` + `/prove`, `AppState`/`HeadlessState` (with optional GUI-callback slots `on_status`/`on_versions_changed`/`show_auth_popup`), `ServerStatus`, `bind_with_retry`, `healthy_aztec_on_port`, `AUTH_DECISION_TIMEOUT`, `HTTPS_PORT`, and all of authorization/config/bb/versions.
- src-tauri layers on: **TLS serving** (`tls.rs` — the only HTTPS code; core has zero TLS-serve deps), **cert lifecycle** (`certs.rs`), **the actual GUI callbacks** that core only declares slots for (tray `set_text`/`set_tooltip`, version-submenu rebuild, popup window), Tauri commands, crash-recovery, updater, windows. Core defines the *seams*; src-tauri fills them. Clean inversion — core never calls up.

---

## 5. Frameworks / libs

- **HTTP server**: `axum` 0.8 (+ `tower-http` cors/set-header, `tower` util in tests). HTTPS path drops to `hyper` 1 + `hyper-util` (auto/server/service) manually (src-tauri only).
- **TLS stack**: `tokio-rustls` 0.26 (serving), `rcgen` 0.13 (pem/x509-parser/zeroize features — cert gen), `rustls-pemfile` 2 (PEM parse), `x509-parser` 0.18 (expiry parse). rustls CryptoProvider = `aws-lc-rs` (installed in `main.rs:184`; both aws-lc-rs and ring are present so it must be explicit).
- **Tauri**: `tauri` 2 (tray-icon, image-png), plugins `tauri-plugin-autostart`, `tauri-plugin-updater`, `tauri-plugin-process`, `tauri-plugin-webdriver` (optional, `webdriver` feature). `tauri-build` 2 in build-deps.
- **Async runtime**: `tokio` 1 (full) everywhere; `parking_lot` 0.12 for `RwLock`/`Mutex`.
- **serde**: `serde` 1 (derive) + `serde_json` 1 across all crates.
- **HTTP client**: `reqwest` 0.12 (stream, json) — downloads + health probe.
- **Archive/crypto**: `flate2` 1, `tar` 0.4, `sha2` 0.11, `hex` 0.4, `base64` 0.22.
- **Misc**: `url` 2 (origin canon), `time` 0.3 (cert validity), `which` 7 (bb lookup), `dirs` 6 (paths), `tempfile` 3, `urlencoding` 2 (window URLs).
- **Tracing**: `tracing` 0.1; subscribers — `tracing-subscriber` (env-filter/registry), `tracing-appender` (rolling file, desktop only).
- **Test-only**: `serial_test` 3 (env-var-mutating tests), `tokio` test-util (paused-time).

---

## 6. Test surfaces

All tests are inline `#[cfg(test)] mod tests` (no `tests/` integration dirs anywhere). Rough coverage:

| Module | Test density | What's covered |
|---|---|---|
| `core/server.rs` | **Very high** (~910 LOC, ~30 tests) | `/health` contract, CORS preflight, `/prove` all paths: invalid version, invalid/denied/timeout origin, 429 too-many, headless deny, success-path status sequence (fake bb via `BB_BINARY_PATH`), text/plain wire-contract characterization. |
| `core/versions.rs` | **Very high** (~630 LOC) | tier classification, retention/eviction edge cases, `AztecVersion::parse` ≡ `is_valid_version`, traversal rejection, tarball extract (nested/symlink/corrupt/empty), sha256, install atomic-replace. 2 network tests gated by `ACCELERATOR_DOWNLOAD_TEST`. |
| `core/authorization.rs` | High | localhost auto-approve variants, `canonicalize_origin` (~20 cases: ports/case/trailing-dot/path/query/userinfo/extension schemes/garbage), request/resolve, MAX_PENDING. |
| `core/config.rs` | High | serde roundtrips, Speed thread counts, `migrate_approved_origins` (canonicalize/dedupe/drop/order). |
| `core/bb.rs` | Medium | `find_bb` env override + chain (`#[serial]`), proof header prepend, char-boundary stderr truncation. |
| `core/server/bind.rs` | Medium | wait-out transient holder, hard-deadline give-up, non-AddrInUse immediate propagation. |
| `core/server/probe.rs` | Medium | `is_healthy_aztec_response` classification (pure fn). |
| `core/server/{prove,auth}.rs` | None directly | Exercised transitively via `server.rs` tests (handlers are `pub(crate)`). |
| `src-tauri/certs.rs` | Medium | cert/leaf PEM gen, rustls load, 0o600 perms, **no-ca-key invariant**, legacy-key migration. macOS trust fns untested (subprocess). |
| `src-tauri/crash_recovery.rs` | Medium | `task_xml` (Windows, cfg-gated), plist patch (macOS, cfg-gated, nested-dict). The systemd/schtasks side-effecting fns untested. |
| `src-tauri/verified_sites.rs` | Medium | embedded registry loads, case-insensitive lookup, rejects invalid/dup/non-ASCII. |
| `src-tauri/main.rs` | Low | only `should_prevent_exit` (3 tests). The 200-line `setup` closure is untested. |
| `src-tauri/{commands,tray,windows,updater}.rs`, `server/tls.rs` | **None** | No tests — Tauri-runtime-coupled. |
| `server/main.rs` | **None** | No tests. |

CLAUDE.md claims ~90 Rust unit tests; concentration is heavily in `core` (server + versions + authorization).

---

## 7. Generated / vendored / fixture

- **Embedded data fixtures** (not logic): tray PNGs `include_bytes!` in `tray.rs:12-38` (1 idle + 24 proving frames); `verified-sites.json` `include_str!` in `verified_sites.rs:20`.
- **Generated env**: `AZTEC_BB_VERSION` injected by `build.rs` from the `AZTEC_VERSION` file; `CARGO_PKG_VERSION` from Cargo.toml.
- **Cargo.lock** ×3 (committed; `src-tauri/Cargo.lock` shows as modified in git status).
- **Test-only synthetic fixtures**: in-test tarball builders (`versions.rs` GzEncoder), fake-bb shell script (`server.rs:631`), test CA/leaf builder (`certs.rs:408`).
- No codegen, no vendored third-party source, no `OUT_DIR` Rust generation beyond Tauri's own `generate_context!`.

---

## Quality-relevant signals

### (a) Cross-boundary DUPLICATION candidates

> Context: the core-extraction (2026-06-07) already de-duped most server wiring into `core`. `src-tauri/server.rs` and `src-tauri/server/tls.rs` are intentionally thin shims. The remaining duplication is mostly in **bootstrapping/state-construction** and **env/version plumbing**, which the extraction did NOT unify.

1. **`AppState` / `HeadlessState` construction is hand-rolled in 3 places with near-identical field lists** — the single biggest duplication smell.
   - `server/src/main.rs:62-75` (headless: auth/config from `ALLOWED_ORIGINS`, semaphore, app_version, bundled_version from env).
   - `src-tauri/src/main.rs:345-367` (desktop: same core fields + 3 GUI callbacks).
   - `core/src/server.rs` test helper `auth_state_with_popup` (`:720-739`) builds yet another variant.
   No builder / constructor for `HeadlessState`; every call site spells out the struct literal with `..Default::default()`. A `HeadlessState::with_origins(...)` or `AppState::headless(...)` would collapse these. **High-value, low-risk consolidation.**

2. **Two `main.rs` + two tracing-subscriber init blocks.** `server/src/main.rs:32-37` and `src-tauri/src/main.rs:205-211` both build an `EnvFilter::try_from_default_env().unwrap_or(info)` + `registry().with(...).with(fmt::layer)...init()`. The desktop one adds a file-writer layer; otherwise identical boilerplate. Not trivially shareable (different layer sets, different crates) but a `core::init_tracing(extra_layer)` helper is plausible.

3. **bb-version fallback string `"unknown"` defined in two spots.** `core/server.rs:36` `DEFAULT_BB_VERSION = "unknown"` and `build.rs:9` emits `AZTEC_BB_VERSION=unknown`. Same sentinel, two definitions — they can silently drift. Low severity.

4. **`origin_denied` error body built twice in the same file.** `core/server/auth.rs:61-67` (headless no-popup deny) and `:124-132` (user-clicked deny) construct the identical `(FORBIDDEN, json_error("origin_denied", "Access denied for origin: {origin}"))`. A local `fn denied(origin)` would dedupe. Trivial.

5. **`config.write()` + `config::save()` pattern** — already partially addressed: `commands.rs:12` `mutate_config` exists and is used by 6 commands. BUT the *same* lock-mutate-save also appears un-helper'd in `core/server/auth.rs:112-119` (persist approved origin) and `src-tauri/main.rs:103-108` (`reset_safari_support`). The helper lives in `src-tauri/commands.rs` so `core` can't use it — a candidate to push a `config::mutate` into core.

6. **macOS subprocess-wrapper shape repeated.** `certs.rs` has 4 near-identical `Command::new("security").args([...]).output()` + status-check + stderr-log blocks (`add_trusted_cert` :313, `verify_cert_trusted` :333, `ca_keychain_sha1` :347, `remove_trusted_cert_by_sha1` :361); `versions.rs:385-415` repeats the same shape for `xattr`/`codesign`; `crash_recovery.rs` for `systemctl`/`schtasks`. No shared "run CLI, log on failure" helper. Medium — cross-module, so lower priority than #1.

7. **`sanitize_window_label` / hex-of-sha256-prefix** — `commands.rs:129` hashes an origin to a 6-byte hex label; conceptually similar to `versions.rs sha256_hex` but different enough (truncation, purpose) that merging is not warranted. Flagged only to pre-empt a false-positive cluster.

8. **`home_dir().unwrap_or_else(|| PathBuf::from("."))` + `.join(".aztec-accelerator")`** repeated across `config.rs:75`, `versions.rs:133`, `certs.rs:13` (and `log_dir` uses `data_local_dir`). Four functions independently re-derive the app's base dir. A `core::app_dir()` would centralize. Low-medium.

### (b) Largest / most complex functions (Long Method / Large Class candidates)

1. **`src-tauri/src/main.rs:260-462` — the Tauri `.setup(move |app| {...})` closure (~200 lines).** THE complexity hotspot. Mixes: activation policy, version read, status menu item, autostart/crash-recovery check, tray menu+icon build, animation loop start, **three callback closures** (versions-changed, auth-popup, on_status), full `AppState` construction, HTTPS startup + legacy-key migration, managed-state registration, startup bb diagnostics, webdriver window, the HTTP-server spawn with the entire `AddrInUse`/redundant-instance classification inline (`:407-440`), and the update-poller spawn. Deeply nested (closures-in-closures-in-spawn), many concerns, untested. Prime extraction target: pull the callback builders and the server-spawn block into named fns.

2. **`core/src/versions.rs:342-420` — `download_bb` (~80 lines).** Orchestrates cache-check → download → verify → install → chmod → **macOS xattr+codesign** (the codesign block alone is ~35 lines with two match arms each doing cleanup+error-return, `:397-415`). The macOS post-processing is a distinct concern bolted onto the cross-platform download flow. The Q11 refactor already extracted `download_tarball`/`verify_digest`/`install_version_dir`; the codesign tail is the remaining lump.

3. **`core/src/server/auth.rs:14-135` — `authorize_origin` (~120 lines, one fn).** Linear but long: auth-manager guard → origin extract → canonicalize → approved-check → headless-deny → request → popup → 60s timeout (nested `.map_err` chains `:88-106`) → Allow{remember}-persist / Deny match. Multiple early returns + the remember-persist nested-if make it dense. Reasonable to keep, but a candidate to split the "await decision" from the "apply decision".

4. **`core/src/versions.rs:213-252` — `versions_to_evict` (~40 lines).** Nested: group-by-tier loop containing sort + retain + effective-limit branch + drain. Moderate cyclomatic complexity; well-tested so lower risk, but the `effective_limit` bundled-in-tier logic (`:237-244`) is subtle.

5. **`src-tauri/src/crash_recovery.rs` — `enable_impl` (Linux `:133-198`, ~65 lines).** Builds the systemd unit string + create-dir + write + two `systemctl` subprocess calls + match. The Windows `enable_impl` (`:257-316`) is comparably long (UTF-16 BOM encoding + tempfile dance + schtasks). Per-platform so only one compiles, but each is a long mixed-concern fn (string-build + fs + subprocess).

6. **`src-tauri/src/certs.rs:261-296` — `rotate` (~35 lines, heavily `#[cfg(macos)]`-laced).** stage → capture-old-sha1 → trust+verify-or-discard → atomic 3× rename → remove-old-anchor. The cfg-gating interleaved with the happy path makes it hard to read; the non-macOS build sees a much simpler fn than the macOS one.

7. **`core/src/server.rs:150-190` — `health` handler.** Not long but mixes 3 concerns: available-versions assembly, the conditional `https_port` (Q7 logic), and debug-only `runtime` injection via `#[allow(unused_mut)]` + cfg blocks. Minor.

> Note on file size: `core/server.rs` (1122) and `core/versions.rs` (1209) look like Large-File smells but are **~80% tests**. Production surface per module is modest. The genuine Large-Class/Large-Method concern is `main.rs`'s `setup` closure (#1), not the test-heavy core files.
