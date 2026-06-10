# Aztec Accelerator: Tauri GUI + Headless Server — Quality Audit Map

**Generated**: 2026-06-10  
**Scope**: `/packages/accelerator/src-tauri/src/` (GUI) + `/packages/accelerator/server/src/` (headless)  
**Total LOC (both crates)**: ~3029 lines  
**Architecture**: GUI wraps accelerator-core server via Tauri; headless runs the same server without UI.

---

## 1. Module Inventory

### src-tauri (GUI crate)

| File | Purpose | LOC |
|------|---------|-----|
| **main.rs** | App bootstrap, tray setup, HTTPS listener, update poller, window lifecycle | 500 |
| **certs.rs** | macOS Safari HTTPS: keyless CA generation, trust management, atomic cert rotation, legacy migration (SEC-08) | 632 |
| **crash_recovery.rs** | Platform-agnostic crash recovery (macOS launchd KeepAlive + ThrottleInterval, Linux systemd, Windows Task Scheduler repeating trigger) | 493 |
| **commands.rs** | Tauri IPC commands: config mutations (set_speed, remove_origin), Safari enable/disable, auth response, update handling | 276 |
| **updater.rs** | Update check loop, artifact size cap (SEC-03), perform_update, Windows crash-recovery re-arm | 237 |
| **verified_sites.rs** | Embedded JSON registry of recognized origins; case-insensitive lookup with canonicalization | 211 |
| **windows.rs** | Window creation/focus for Settings, Auth popup (60s timeout), Update prompt; request_id-keyed popup labels (SEC-06) | 153 |
| **tray.rs** | Tray menu building (versions submenu, dev-mode items), icon animation loop (24-frame proving) | 172 |
| **server.rs** | Thin wrapper over accelerator-core::server; re-exports router/start/AppState; GUI-side spawn_https wrapper | 24 |
| **server/tls.rs** | HTTPS listener with tokio-rustls TlsAcceptor; independent error handling (never crashes app) | 73 |
| **lib.rs** | Module declarations + re-exports from accelerator-core (authorization, bb, config, versions, log_dir) | 15 |

**Total src-tauri**: 2786 LOC

### server (headless crate)

| File | Purpose | LOC |
|------|---------|-----|
| **main.rs** | Origin-gating config (allow-all vs allowlist vs deny-by-default), AppState construction, logging, server start | 243 |

---

## 2. Public Surface / Entrypoints

### Tauri Commands (IPC via `#[tauri::command]`)

**In commands.rs** (13 total):
- `get_config()` — read config state
- `get_autostart_enabled()` / `set_autostart()` — autolaunch + crash-recovery wiring
- `set_speed(Speed)` — config mutation + persist
- `remove_approved_origin(String)` — config mutation
- `get_system_info()` — platform + CPU count
- `get_verified_info(origin)` → `VerifiedSiteDto` — recognized site lookup
- `respond_auth(request_id, origin, allowed, remember)` — window close + timeout cancel + auth resolution (SEC-06 request_id-keyed)
- `enable_safari_support()` / `disable_safari_support()` (macOS only) — certs + trust + HTTPS spawn
- `set_auto_update(bool)` — config mutation
- `respond_update_prompt(action, auto_update)` — update install or remind-later

### Tray Menu Events (main.rs `.on_menu_event`)

- `"quit"` → disable crash recovery, exit(0)
- `"settings"` → open Settings window
- `"show_logs"` → open log dir in browser
- `"open_github"` → open GitHub in browser

### App Lifecycle (main.rs `.setup`)

1. **Log setup** (lines 240–265): rolling file appender (daily, 7-file retention), Unix perms 0o700
2. **Tray build** (lines 332–355): menu + icon + animation loop start
3. **State construction** (lines 366–417):
   - Callback wiring: on_status (tray text/tooltip + animation flag), on_versions_changed, show_auth_popup
   - HeadlessState::headless + AppState::desktop (GUI-specific callbacks)
4. **HTTPS startup** (lines 419–428): cert migration, try_start_https if Safari enabled
5. **HTTP server spawn** (lines 450–456): spawn_http_server task
6. **Update poller spawn** (lines 458–464): 5s warm-up, 12h loop (compile-gated `#[cfg(not(feature = "webdriver"))]`)
7. **WebDriver context** (lines 447–448): open Settings window if feature=webdriver (test isolation)

### Headless Server Entrypoint (server/main.rs)

- **Version flag**: `--version` / `-V` prints accelerator-server version
- **Origin gating** (resolve_gating):
  - `--allow-all` / `ACCEL_ALLOW_ALL=1` → no AuthorizationManager (all origins)
  - `ALLOWED_ORIGINS=a,b,c` → parse, canonicalize, dedupe, gate with allowlist
  - Default (neither) → gate with empty allowlist (deny all non-localhost) — SEC-01c fix
- **Config construction**: approved_origins, auto_approve_localhost=true (SEC-04/R13 — no popup)
- **Start**: calls accelerator_core::server::start(state)

---

## 3. Dependency Graph

### src-tauri Internal Imports

```
main.rs
  ├→ certs (certs_exist, is_ca_trusted, load_rustls_config, install_ca_trust, 
  │          migrate_legacy_ca_key, regenerate_leaf_if_expiring)
  ├→ crash_recovery (enable_crash_recovery, disable_crash_recovery)
  ├→ commands (AuthState, ConfigState, VerifiedSitesState, SharedAppState, PendingUpdate)
  ├→ verified_sites (VerifiedSitesRegistry::load)
  ├→ windows (show_auth_popup_window, open_settings_window, show_update_prompt_window)
  ├→ tray (build_tray_menu, build_tray_icon, start_animation_loop)
  ├→ updater (check_for_update, perform_update)
  └→ server (AppState, HeadlessState, ServerStatus, spawn_https)

commands.rs
  ├→ certs (migrate_legacy_ca_key, generate_and_save, install_ca_trust, load_rustls_config)
  ├→ server (spawn_https, AppState — via SharedAppState type alias)
  ├→ updater (perform_update)
  ├→ crash_recovery (enable_crash_recovery, disable_crash_recovery)
  └→ windows (sanitize_window_label)

windows.rs
  ├→ commands (sanitize_window_label)
  ├→ AUTH_DECISION_TIMEOUT (from server/core)
  └→ AuthorizationManager

tray.rs
  └→ versions (list_cached_versions)

updater.rs
  ├→ crash_recovery (disable_crash_recovery, rearm via enable_crash_recovery)
  └→ commands (ConfigState implicit via pub async fn check_for_update signature)

crash_recovery.rs
  └→ (no src-tauri imports — pure platform logic)

certs.rs
  └→ (no src-tauri imports — pure crypto)

verified_sites.rs
  ├→ authorization (canonicalize_origin)
  └→ include_str!("../../verified-sites.json")

server.rs
  └→ pub use accelerator_core::server::*
```

### Imports from accelerator-core

**In lib.rs** (re-exports):
- `authorization`, `bb`, `config`, `log_dir`, `versions` — used by main.rs + commands

**In server.rs** (re-export):
- `accelerator_core::server::*` — AppState, HeadlessState, ServerStatus, spawn_https, router, etc.

**Direct core usage**:
- `main.rs`: `authorization::AuthorizationManager`, `server::{AppState, HeadlessState, ServerStatus}`
- `commands.rs`: `authorization::{AuthDecision, AuthorizationManager}`, `config::{AcceleratorConfig, Speed}`
- `windows.rs`: `authorization::{AuthDecision, AuthorizationManager}`, `server::AUTH_DECISION_TIMEOUT`
- `verified_sites.rs`: `authorization::canonicalize_origin`
- `server/tls.rs`: `accelerator_core::server::{bind_with_retry, router_for_port, AppState, HTTPS_PORT}`

---

## 4. Similarity Candidates (Duplication Analysis)

### A. Within src-tauri

#### 1. **window.rs: Window construction boilerplate** (lines 36–55)
Three callers use `open_or_focus_window` with `WindowConfig`:
- Settings (label="settings", focus_if_open=true)
- Auth popup (label=request_id hash, focus_if_open=false, 60s timeout spawn)
- Update prompt (label="update-prompt", focus_if_open=false)

*Status*: **Unified via `open_or_focus_window` — good pattern.** But the 60s timeout spawn is Settings-unaware; if Settings ever needs timeout recovery, duplication risk emerges.

#### 2. **certs.rs: macOS trust lifecycle** (lines 326–346)
Both `rotate()` and `enable_safari_support` (commands.rs:151–183) follow:
1. Call `migrate_legacy_ca_key()` (SEC-08 fail-closed)
2. Generate/load certs
3. Install trust (prompt)
4. Start HTTPS

*Status*: **DUPLICATION — migrating legacy CA key happens twice**:
- Line 424 in main.rs (startup path)
- Line 162 in commands.rs (settings enable path)

The comment at commands.rs:157 acknowledges this: "SEC-08 (post-impl codex M1): the startup path runs this same fail-closed migration." This is intentional (Settings re-enable must not bypass the migration), but it's a maintenance burden — a single `enable_safari` helper (init or reinit) would dedup it.

#### 3. **crash_recovery.rs: config file path resolution** (lines 62–90, 133–154, 246–253)
Platform-specific enable/disable patterns have similar structure:
- Read current plist/service file
- Check/patch it
- Write back
- Call systemctl/schtasks

*Status*: **Platform-separated by `#[cfg]` — acceptable.** The abstraction via `CrashRecovery` trait is clean.

#### 4. **updater.rs + commands.rs: update size checking** (updater.rs:67–122 vs commands.rs:context)
`advertised_update_size()` is only called from `perform_update()`. The SEC-03 cap logic is self-contained.

*Status*: **No duplication — single call site.**

### B. Between src-tauri and server/main.rs (headless)

#### **Logging setup** (HIGH)
**src-tauri main.rs** (lines 240–265):
```rust
let file_appender = tracing_appender::rolling::RollingFileAppender::builder()
    .rotation(tracing_appender::rolling::Rotation::DAILY)
    .filename_prefix("accelerator")
    .filename_suffix("log")
    .max_log_files(7)
    .build(&log_path)?;
let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);

tracing_subscriber::registry()
    .with(env_filter)
    .with(fmt::layer().with_writer(std::io::stdout))
    .with(fmt::layer().with_writer(file_writer).with_ansi(false))
    .init();
```

**server/main.rs** (lines 37–42):
```rust
tracing_subscriber::registry()
    .with(env_filter)
    .with(fmt::layer().with_writer(std::io::stdout))
    .init();
```

*Status*: **DUPLICATION (different intent)**. Tauri logs to both file + stdout (dev diagnostics); headless logs to stdout only (CI/container-friendly). The tauri version has no equivalent in server; should be extracted to accelerator-core::logging if needed.

#### **AppState construction** (MEDIUM)
**src-tauri main.rs** (lines 366–417): Builds `HeadlessState::headless()` + `AppState::desktop()` with three GUI callbacks (on_status, on_versions_changed, show_auth_popup).

**server/main.rs** (lines 85–96): Builds `HeadlessState::headless()` + `AppState::headless()` with None callbacks.

*Status*: **Not duplication — different app types.** But the `HeadlessState::headless(...)` call signature is identical, and both parse `AZTEC_BB_VERSION` env the same way. This is fine (the builder pattern abstracts the differences). Risk: if `HeadlessState::headless()` signature changes, both sites must update.

#### **Origin gating config** (MEDIUM)
**server/main.rs** (lines 46–86): Parses `ALLOWED_ORIGINS` env, handles `--allow-all`, constructs gated/no-gate `AcceleratorConfig`.

**src-tauri main.rs**: No equivalent — all origins are pre-approved (the app is single-machine, Tauri sandboxed by OS).

*Status*: **Not duplication — deliberate design difference.** Headless needs CLI gating; desktop app doesn't. Risk: if a future tauri command needs origin gating, the parsing logic must be shared (accelerator-core module) to avoid drift.

#### **Env flag parsing** (LOW)
Both read `AZTEC_BB_VERSION` env. Both read `EnvFilter` env.

*Status*: **Expected repetition.** Env parsing in `main()` is idiomatic.

### C. Between src-tauri and accelerator-core

#### **Auth popup timeout** (windows.rs:118–128 vs core/server/auth.rs)
windows.rs spawns 60s timeout; core's AuthorizationManager resolves. The flow is:
1. Tauri command: `show_auth_popup_window()` spawns 60s tokio timeout
2. User response: `respond_auth()` resolves immediately
3. Timeout fires: calls `auth_manager.resolve()` again (idempotent no-op if already resolved)

*Status*: **Clean separation.** GUI owns timeout spawn; core owns resolution logic.

#### **Certificate file operations** (certs.rs vs core)
certs.rs writes, reads, checks CA/leaf; core (server/prove.rs) calls certs::load_rustls_config.

*Status*: **Clean separation.** certs.rs is GUI-only (macOS Keychain); core has no TLS.

---

## 5. House Conventions

### Command Error Style
**commands.rs**: All `#[tauri::command]` return `Result<T, String>` or naked `T`.
- Errors are serialized as JSON strings.
- Preferred: `config::save(&cfg).map_err(|e| e.to_string())?`
- Pattern: `mutate_config()` helper (lines 12–19) centralizes lock-mutate-save.

### Config Mutation Pattern
```rust
fn mutate_config(
    config: &ConfigState,
    f: impl FnOnce(&mut AcceleratorConfig),
) -> Result<(), String> {
    let mut cfg = config.write();
    f(&mut cfg);
    config::save(&cfg).map_err(|e| e.to_string())
}
```
Used by: `set_speed`, `remove_approved_origin`, `set_auto_update`, Safari enable/disable.
*Status*: **Good pattern — single source of truth for persist.**

### Window Label Construction
**commands.rs:141–145**: `sanitize_window_label(key)` → SHA-256 hash (first 6 bytes) → hex-encoded.
- Used for: auth popup (request_id), settings (static "settings"), update (static "update-prompt").
- SEC-06 benefit: opaque request_id hash prevents origin-string collisions.

### Platform Gating Patterns
- **`#[cfg(target_os = "macos")]`**: Keychain trust (add, verify, remove, query SHA-1), plist patching.
- **`#[cfg(target_os = "linux")]`**: systemd service creation.
- **`#[cfg(target_os = "windows")]`**: Task Scheduler XML, schtasks.exe, UTF-16LE encoding.
- **`#[cfg(not(feature = "webdriver"))]`**: Update check loop, update prompt window, should_poll_for_updates.

### Error Handling Idioms
- **Fail-closed** (certs.rs:185–217): `migrate_legacy_ca_key()` retries and RE-CHECKS the file; if still present, returns `Err` (SEC-08). Caller (main.rs) logs error, skips HTTPS.
- **Best-effort** (certs.rs:410–423): `remove_trusted_cert_by_sha1()` is a no-op on failure; app stays up.
- **Idempotent** (crash_recovery.rs:66–75): `enable_impl()` checks for existing KeepAlive before patching.

---

## 6. Test Surfaces

### Inline Tests

| Module | Test Count | Coverage |
|--------|------------|----------|
| **certs.rs** | 6 | CA/leaf generation, write_pem_file perms (0o600), ca.key never written (SEC invariant), legacy migration idempotent, fail-closed on lock, nested plist dicts |
| **crash_recovery.rs** | 6 | Windows task XML structure (TimeTrigger + IgnoreNew, no RestartOnFailure), exe escaping, macOS plist patch (last </dict> only), nested dict handling, invalid input |
| **updater.rs** | 3 | size_from_feed URL matching, missing size (older feeds), platform matching |
| **verified_sites.rs** | 5 | embedded registry loads, case-insensitive lookup, miss detection, invalid origin rejection, duplicate detection |
| **main.rs (tauri)** | 3 | should_prevent_exit: window-close (None → prevent), explicit quit (Some(0) → allow), restart (Some(i32::MAX) → allow) |
| **main.rs (headless)** | 12 | allow-all conflict, deny-by-default empty list, allowlist parsing, origin canonicalization, deduping, trailing commas, failure on invalid |

**Total**: ~35 tests

### Serial Tests
No `#[serial]` observed. Tests are isolated (tempdir, mocking, no global state).

### Coverage Gaps
- **Tauri commands**: No unit tests (IPC boundary tested via e2e/webdriver).
- **Window.rs**: No unit tests (UI creation not testable in unit context).
- **Updater.rs**: No integration test for actual network check/download (mocked in unit).
- **Tray animation**: No test of frame advancement or icon-set calls.

---

## 7. Long-Function / Large-Module Hotspots

### Large Modules (>400 LOC)

| File | LOC | Key Concerns |
|------|-----|--------------|
| **certs.rs** | 632 | 🔴 **HOTSPOT**: Largest file. Cert generation (write_new_cert_set:141–153, 30 LOC), rotation (rotate:317–346, 29 LOC), macOS trust lifecycle (7 functions across 110 LOC), migration (2 functions, 36 LOC), 50 LOC tests. Monolithic cert state machine. Risk: a new trust primitive (e.g. Linux cert import) adds >100 LOC without refactoring. |
| **crash_recovery.rs** | 493 | 🟡 **HOTSPOT**: Three platform impls (macOS 61 LOC, Linux 66 LOC, Windows 93 LOC) + shared trait. Platform logic is copy-pasted between enable/disable (plist read-patch-write, systemctl calls, Task Scheduler XML). Mutation in one platform often needs tripling. Tests help (6 tests). |
| **main.rs (tauri)** | 500 | 🟡 **HOTSPOT**: App bootstrap monolith. `.setup()` closure (152 LOC, lines 314–466) wires logging, tray, callbacks, HTTPS, HTTP, update poller. Callback closures capture 5+ clones (status, tray, app_handle, bundled_version, is_animating, etc.). Hard to test in isolation. Lines 180–221: spawn_http_server (42 LOC, includes exit-0-if-healthy logic + dynamic Err classification). |

### Long Functions (>60 LOC)

| File:Line | Function | LOC | Note |
|-----------|----------|-----|------|
| certs.rs:141 | `write_new_cert_set()` | 13 | Short, focused. |
| certs.rs:317 | `rotate()` | 29 | Atomic swap sequence (stage → trust → verify → swap → remove-old). |
| crash_recovery.rs:62 | `enable_impl()` (macOS) | 29 | Read plist, check, patch, write. |
| crash_recovery.rs:133 | `enable_impl()` (Linux) | 66 | Full systemd service template generation + systemctl. |
| crash_recovery.rs:257 | `enable_impl()` (Windows) | 60 | Task XML UTF-16 encoding, temp file, schtasks. |
| crash_recovery.rs:322 | `disable_impl()` (Windows) | 28 | Retry loop with /Query verification (correctness-critical). |
| crash_recovery.rs:353 | `task_xml()` | 37 | XML template generation (boilerplate). |
| main.rs:180 | `spawn_http_server()` | 42 | Server spawn, AddrInUse classification, exit-0-if-healthy guard, tray status updates. |
| main.rs:235–466 | `main()` | 242 | Crypto init, log setup, Tauri builder config, `.setup()` closure (inline), run event loop. |
| commands.rs:151 | `enable_safari_support()` | 33 | macOS only; migration, cert gen, trust install, HTTPS spawn (3 fallible steps). |
| commands.rs:221 | `respond_update_prompt()` | 50 | Action dispatch (update vs. later), pending update extract, async spawn. |
| updater.rs:85 | `perform_update()` | 95 | 🔴 **LONGEST**: Size cap enforcement (with residual analysis), download + progress, Windows crash-recovery disarm, install, re-arm. Needs refactoring. |
| windows.rs:77 | `show_auth_popup_window()` | 52 | URL encoding, window open, 60s timeout spawn (complex closure capture). |
| server/main.rs:24–102 | `main()` | 79 | Arg parsing, env parsing, gating decision, state construction, server start. |
| server/main.rs:112 | `parse_allowed_origins_env()` | 15 | Clean; split, trim, canonicalize, dedupe. |

---

## 8. Structural Observations

### Callback Architecture (GUI → Core)
main.rs creates three `Arc<dyn Fn>` callbacks for the server:
1. **on_status**: Updates tray text + tooltip + animation flag. Fires frequently (every proving status change).
2. **on_versions_changed**: Rebuilds tray Versions submenu (dev-mode only).
3. **show_auth_popup**: Spawns auth window + 60s timeout.

These are passed to `AppState::desktop()`. The headless server passes `None` for all three. *Risk*: If a callback fails (e.g., window spawn panics), it could crash the server task. Current design relies on callback implementations never panicking (no explicit error handling in the callback invocation site in core).

### HTTPS Server Dual-Spawn Pattern
- **Startup path** (main.rs:55–94): `try_start_https()` checks config, certs, macOS trust, loads TLS, calls `server::spawn_https()`.
- **Settings path** (commands.rs:151–183): `enable_safari_support()` migrates legacy CA, generates certs, installs trust, loads TLS, calls `server::spawn_https()`.

Both paths use identical `server::spawn_https()` but have divergent TLS-load + failure handling preambles. Comment F-09 (server.rs:14) acknowledges this is intentional (each caller has unique failure modes). *Maintenance risk*: Future HTTPS logic changes must be mirrored in both preambles.

### Tauri ↔ Core Boundary
**State managed by Tauri**:
- config (Arc<RwLock<AcceleratorConfig>>)
- auth_manager (Arc<AuthorizationManager>)
- verified_sites (Arc<VerifiedSitesRegistry>)
- pending_update (Arc<Mutex<Option<Update>>>)

These are passed to core as part of AppState; core never mutates them, only reads. Mutations flow back through Tauri commands (IPC). This is clean but couples Tauri to core's state types.

---

## 9. Feature Gating Summary

### `#[cfg(feature = "webdriver")]`
Enabled for: `cargo tauri dev --features webdriver` (e2e testing).
- Registers WebDriver plugin (port 4445)
- Disables background update check (compile-time)
- Disables update prompt window (compile-time)
- Opens Settings window at startup (gives WebDriver a browsing context)
- Skips update-check tests in CI

### `#[cfg(debug_assertions)]`
- Tauri: dev_mode enabled → tray shows Status item + Versions submenu + Show Logs
- Headless: (no equivalent)
- verified_sites.rs: panics on load error (dev catches mistakes) vs. logs + returns empty (release tolerates older JSON)

### `#[cfg(target_os = "macos")]`
- Keychain trust management (add, verify, remove, query SHA-1)
- LaunchAgent plist patching (KeepAlive + ThrottleInterval)
- Activation policy (Accessory = tray-only, no Dock)

### `#[cfg(target_os = "linux")]`
- systemd user service creation + daemon-reload + enable

### `#[cfg(target_os = "windows")]`
- Task Scheduler task creation (XML, UTF-16LE, schtasks.exe)
- AddrInUse exit-0-if-healthy guard (windows-only dual-launch issue)

### `#[cfg(unix)]`
- File permission setting (0o700 for log dir, 0o600 for PEM files)

---

## 10. Security-Critical Code Regions

| Region | Risk | Mitigation | Status |
|--------|------|-----------|--------|
| **certs.rs migration** (SEC-08) | Legacy CA key on disk (mint-any-cert primitive) if older install not cleaned | Fail-closed: migrate runs at startup + Settings enable; if key can't be removed, HTTPS is skipped (not brought up unsafely) | ✅ Audited |
| **crash_recovery.rs Windows task** | Task survives intentional quit if repeating trigger still exists | Quit menu calls disable_crash_recovery() BEFORE exit(0); updater disarms before NSIS install | ✅ In-code (lessons/phase-4.md) |
| **updater.rs size cap** (SEC-03) | Feed tampering: huge blob → OOM DoS during artifact buffer | Pre-flight check: reject if feed's advertised size > 500 MB (best-effort; signature still verifies) | ⚠️ Documented residual risk |
| **commands.rs auth popup** (SEC-06) | Origin-reuse: stale timeout of old request closes live new request if both use same label | Fixed in post-impl: window label = hash(request_id), NOT origin; resolve by request_id opaque key | ✅ Audited |
| **windows.rs auth popup timeout** | User closes window without responding → timeout still fires, resolves by request_id → no-op (harmless) | Timeout is idempotent; resolve() returns success for already-consumed sender | ✅ Good pattern |
| **verified_sites.rs** | Non-ASCII origin in JSON (punycode A-label required) | Load-time rejection: `origin.is_ascii()` check (raw level); not relying on url::Url auto-puncoding | ✅ Tested |

---

## 11. Maintenance Risks & Debt Summary

### High Priority

1. **updater.rs:85 `perform_update()`** (95 LOC)
   - Single function handles: size cap, download, Windows crash-recovery disarm/re-arm, install, restart.
   - Suggests splitting: `check_size_cap()`, `perform_download()`, `handle_crash_recovery_for_windows()`, `install_and_restart()`.
   - Risk: future update-flow changes are error-prone.

2. **certs.rs:317 `rotate()` + commands.rs:162 `enable_safari_support()`**
   - Both call `migrate_legacy_ca_key()` independently.
   - Suggest: `enable_safari_support_with_migration()` helper or a single `configure_safari()` entry point.

3. **main.rs `.setup()` closure** (152 LOC inline)
   - Captures 5+ clones, wires logging, tray, HTTPS, HTTP, update poller.
   - Suggest: Extract callback building, server spawn, to named functions (for testability + readability).

### Medium Priority

4. **crash_recovery.rs** (493 LOC, 3 platform impls)
   - macOS/Linux/Windows enable/disable logic is parallel-structure; common patterns could be abstracted.
   - Suggest: `CrashRecoveryConfig` struct, `write_config()`, `read_config()` helpers.

5. **Logging divergence** (tauri main.rs vs. server/main.rs)
   - Tauri logs to file + stdout; headless to stdout only.
   - Suggest: Extract to `accelerator-core::logging` module with optional file appender feature.

6. **HTTPS spawn split** (main.rs + commands.rs → server::spawn_https)
   - Two preambles (TLS load + error handling) differ by design, but mirror each other.
   - Suggest: Document pattern in server.rs; consider a builder-pattern helper if 3rd site emerges.

### Low Priority

7. **Tray animation** (tray.rs:140–172)
   - Frame loop is fire-and-forget (no shutdown). ~600 frames/min = acceptable overhead.
   - Suggest: Add shutdown path if app lifecycle warrants it (unlikely given tray-only app).

8. **Window focus behavior** (windows.rs:9–15, comment)
   - App stays Accessory (tray-only) so focused window might get buried behind fullscreen app.
   - Suggest: Document the design decision (tray-only vs. Dock presence trade-off) in a DESIGN.md.

---

## 12. Cross-Crate Consistency

### Naming

| Concept | src-tauri | server | Core |
|---------|-----------|--------|------|
| App version | `env!("CARGO_PKG_VERSION")` | `env!("CARGO_PKG_VERSION")` | ✅ Consistent |
| bb version | `env!("AZTEC_BB_VERSION")` (optional) | `std::env::var("AZTEC_BB_VERSION")` | ✅ Consistent |
| Config reload | No reload (static file at startup) | No reload | ✅ Consistent (design) |
| Origin gating | None (all pre-approved) | Via env + CLI | ✅ Intentional diff |

### Logging Levels

| Event | Level |
|-------|-------|
| Server bind, HTTP/HTTPS start | info |
| Callback fired | info (on_status), debug (auth timeout) |
| Cert operations | info (generation, rotation), warn (failures) |
| Crash recovery enable | info, warn on failure |
| Tauri command execution | (not logged by default; tracing in commands is sparse) |

**Gap**: Tauri commands don't log execution (only errors). Headless server logs gating decisions. Suggest: consistent `info` logging for command entry/success.

---

## 13. Summary Table

| Metric | Value | Note |
|--------|-------|------|
| **Total Files** | 11 (src-tauri) + 1 (server) | - |
| **Total LOC** | 2786 (tauri) + 243 (server) = 3029 | Excludes tests |
| **Modules** | 11 | - |
| **Tauri Commands** | 13 | All in commands.rs |
| **Inline Tests** | ~35 | Good coverage in crypto/crash-recovery; gaps in UI/IPC |
| **Largest File** | certs.rs (632 LOC) | Monolithic cert state machine |
| **Longest Function** | updater.rs:perform_update (95 LOC) | Candidates for splitting |
| **Duplication Hotspots** | 2 high, 2 medium, 2 low | Cert migration (2x), logging (1x), HTTPS spawn preambles (2x) |
| **cfg-gating extent** | High (`#[cfg]` used 40+ times) | webdriver, macos, linux, windows, unix all respected |
| **Security audit gaps** | Updated versions checked (core), trust management (tauri) | Both good, cross-verify SEC-* comments |
| **Maintenance debt** | Medium | Largest functions need refactoring; cross-crate patterns need docs |

