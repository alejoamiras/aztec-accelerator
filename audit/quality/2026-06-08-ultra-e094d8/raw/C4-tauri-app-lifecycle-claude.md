# Cluster C4 — tauri-app-lifecycle (Claude finder)

**Verdict:** 4 findings. One **architectural** (the 203-line `.setup` god-closure — the strongest smell by a wide margin), two **structural** (HTTPS-startup duplication across the `main`/`commands` boundary; auth-popup window-label + close logic split across `commands`/`windows`), one **local** (cross-binary `AppState`/`HeadlessState` construction duplication — flagged primarily as a coordinator hand-off, since the second instance lives outside this cluster). No Middle Man finding: the 12 invoke-registered `#[command]` fns are thin but each is a genuine IPC trust-boundary translation (Tauri `State`/`AppHandle` → core call), which is the legitimate adapter role, not a delegating pass-through.

---

## Finding 1 — `main.rs` `.setup` closure is a 203-line bootstrap god-method

**Smell:** Long Method (Bloater), with secondary **Divergent Change** (Change Preventer). The single `.setup(move |app| { … })` closure spans `main.rs:260–462` (203 lines) and serially performs ~9 unrelated bootstrap concerns in one lexical scope.

**Maintenance impact:** architectural. Blast radius: this one closure is the entire app's wiring spine — every lifecycle concern (tray, callbacks, state, HTTPS, HTTP server, AddrInUse classification, update poller) funnels through it. Change frequency: hot — it is touched by *any* feature that adds startup behavior (the inline comments referencing Q7, P4, "WINDOWS-ONLY for now" prove it mutates often). A 203-line closure with three nested callback closures defined inline is the highest-future-cost structure in the cluster.

**Concrete evidence** — the closure mixes these distinct responsibilities, each currently inline:
- macOS activation policy + bundled-version read (`main.rs:262–264`)
- status `MenuItem` construction (`main.rs:266–268`)
- autostart→crash-recovery check (`main.rs:271–276`)
- tray menu + tray icon build, with an inline 4-arm menu-event `match` (`main.rs:279–301`)
- animation loop spawn (`main.rs:304–305`)
- **three nested callback closures defined inline**: `on_versions_changed` (`main.rs:316–330`), `show_auth_popup` (`main.rs:335–342`), `on_status` (`main.rs:354–364`)
- full `AppState` + `HeadlessState` construction (`main.rs:345–367`)
- legacy-CA migration + HTTPS startup + `SharedAppState` manage (`main.rs:373–379`)
- startup `bb`-not-found diagnostics (`main.rs:386–390`)
- HTTP-server spawn carrying a **34-line inline AddrInUse / redundant-instance classification block** (`main.rs:400–441`)
- background update-poller spawn (`main.rs:448–459`)

The AddrInUse classification (`main.rs:407–431`) alone — downcast to `io::Error`, `ErrorKind::AddrInUse` check, `cfg!(target_os="windows")` gate, `healthy_aztec_on_port().await` probe, clean-exit decision — is a self-contained policy buried inside a spawned async block inside the setup closure, three nesting levels deep.

**Why it harms future change:** adding any startup step (or reordering for a Temporal-Coupling fix) means editing a 200-line closure where local bindings (`status`, `tray`, `is_animating`, `config_state`, `auth_manager`) are cloned 2–4 times each (`status_clone`, `status_for_diagnostics`, `status_for_server`, `tray_clone`, `tray_for_versions`, `tray_for_diagnostics`, …) with hand-threaded lifetimes. A reader cannot unit-test "what happens when the port is in use" or "is the update poller gated correctly" in isolation — the logic only exists inside the closure. The clone-stutter (≈8 `*_clone`/`*_for_*` rebindings) is the classic symptom of a method that owns too much shared state.

**Smallest safe refactoring:** Extract Method (here Extract Function) repeatedly — pull cohesive blocks into free functions the closure calls:
- `fn classify_server_error(e, app_handle, status, tray) -> ()` for the AddrInUse/redundant-instance block (`main.rs:407–441`)
- `fn build_callbacks(...) -> (StatusCallback, VersionsChangedCallback, ShowAuthPopupCallback)` or three small `make_on_status` / `make_on_versions_changed` / `make_show_auth_popup` constructors (`main.rs:316–364`)
- `fn build_app_state(config_state, auth_manager, callbacks, bundled_version) -> AppState` (`main.rs:345–367`) — this *also* kills Finding 4's duplication
- `fn spawn_update_poller(app, config_state)` (`main.rs:448–459`)
- `fn run_startup_diagnostics(status, tray)` (`main.rs:386–390`)

The `.setup` body then reads as a ~20-line orchestration list.

**What disappears:** the 200-line monolith collapses to a linear sequence of named steps; the ~8 `*_clone`/`*_for_*` rebindings shrink to function arguments; each extracted unit (especially the server-error classifier and the poller gate) becomes independently testable; Divergent Change pressure drops because new startup steps add/modify one small function instead of editing the shared closure.

**Instances:** `packages/accelerator/src-tauri/src/main.rs:260–462` (whole closure); sub-blocks: `:316–330`, `:335–342`, `:354–364` (inline callbacks), `:345–367` (state build), `:400–441` (server spawn + classification), `:448–459` (poller).

---

## Finding 2 — HTTPS-server startup duplicated across the `main` / `commands` trust boundary

**Smell:** Duplicate Code (Dispensable). The "load TLS config → clone the shared `AppState` → `async_runtime::spawn` → `start_https(state, tls)` → log error on failure" sequence is written twice, in two modules, with a third partial copy of the safari-config flip.

**Maintenance impact:** structural. Blast radius: 2 files (`main.rs`, `commands.rs`) — the two Safari-HTTPS entry points (launch-time auto-start vs Settings-triggered enable). Change frequency: moderate — any change to how HTTPS is spawned (e.g. capturing the `JoinHandle` for shutdown, adding a bind-result channel, changing error reporting) must be made in both, or they silently diverge.

**Concrete evidence:**
- `main.rs:84–89` (`try_start_https`): `let state_for_https = state.clone(); tauri::async_runtime::spawn(async move { if let Err(e) = aztec_accelerator::server::start_https(state_for_https, tls_config).await { tracing::error!("HTTPS server error: {e}"); } });`
- `commands.rs:159–164` (`enable_safari_support`): `let state = (**shared_state).clone(); tauri::async_runtime::spawn(async move { if let Err(e) = crate::server::start_https(state, tls_config).await { tracing::error!("HTTPS server error: {e}"); } });`

Byte-for-byte the same spawn body (identical `tracing::error!("HTTPS server error: {e}")` literal), differing only in how `state` is obtained and the `tls_config` it closes over. Both are preceded by a `certs::load_rustls_config()` call (`main.rs:72`, `commands.rs:155`). Separately, `safari_support = false` is flipped in two ways: via `mutate_config` in `disable_safari_support` (`commands.rs:174`) but via a raw `cfg_lock.write()` + `config::save` in `main.rs`'s `reset_safari_support` (`main.rs:105–107`) — the latter cannot reuse `mutate_config` because that helper is private to the `commands` module and operates on `ConfigState`, not on `state.config: Option<…>`.

**Why it harms future change:** the moment HTTPS startup needs to do anything beyond fire-and-forget — capture the task handle for graceful shutdown, report bind success back to the caller, or change the log target — a developer fixes one site and the other keeps the old behavior. The duplication is exactly the kind that "rots independently": these two copies already differ in their state-acquisition preamble, which masks that the spawn tail is identical.

**Smallest safe refactoring:** Extract Function into the GUI `server` wrapper (`src-tauri/src/server.rs`, which already owns the `start_https` re-export): `pub fn spawn_https(state: AppState, tls: Arc<rustls::ServerConfig>)` containing the clone-free spawn + log tail. Both call sites pass an owned `AppState`. Optionally also lift a `fn load_tls_or_log(...) -> Option<Arc<ServerConfig>>` since both precede the spawn with the same `load_rustls_config` call.

**What disappears:** the duplicated `async_runtime::spawn` + `start_https` + identical error-log literal collapses to one helper; future shutdown/handle/reporting changes happen in one place; the two entry points keep only their genuinely different preambles (config-load + state source).

**Instances:** `packages/accelerator/src-tauri/src/main.rs:84–89`; `packages/accelerator/src-tauri/src/commands.rs:159–164`. Related config-flip inconsistency: `main.rs:105–107` vs `commands.rs:174`.

---

## Finding 3 — Auth-popup window-label scheme + popup-close logic split across `commands` and `windows`

**Smell:** Duplicate Code + **Inappropriate Intimacy** (Coupler). The `auth-<hash>` window-label construction and the "find window by label → close it" teardown are reimplemented in two modules that must stay in lockstep, with no single owner of the label scheme.

**Maintenance impact:** structural. Blast radius: 2 files (`commands.rs`, `windows.rs`) plus an implicit contract with the `authorize.html` frontend. Change frequency: moderate — touched whenever the popup identity scheme, lifecycle, or close behavior changes. Because the label is the cross-module handle that ties the *server-side* auth callback, the *timeout* cleanup, and the *user-response* handler to the same OS window, a drift between the two `format!` sites silently breaks popup close (window leaks, or the wrong window is targeted).

**Concrete evidence:**
- Label built in `windows.rs:82`: `let label = format!("auth-{}", commands::sanitize_window_label(origin));`
- The *same* label rebuilt independently in `commands.rs:120`: `let label = format!("auth-{}", sanitize_window_label(&origin));`
- Window-close-by-label teardown appears three times with the same `if let Some(window) = app.get_webview_window(<label>) { let _ = window.close(); }` shape: `commands.rs:121–123` (respond_auth), `windows.rs:110–112` (timeout cleanup), and `commands.rs:257–259` (`close_update_prompt`, hard-coded `"update-prompt"`).

`sanitize_window_label` is correctly shared (one definition, `commands.rs:129`), but the `"auth-"` prefix convention that turns a sanitized hash into the *actual* window label is duplicated as a bare string literal in two files. Nothing structurally prevents `windows.rs` from emitting `auth-<h>` while a future `commands.rs` edit emits `authpopup-<h>`.

**Why it harms future change:** changing the popup label scheme (e.g. to include a request-id so concurrent auth prompts for the same origin don't collide) requires editing both `format!` sites in two modules in exactly the same way — Shotgun-Surgery-flavored coupling. A developer who finds only one site introduces a teardown that can never match the window that was opened.

**Smallest safe refactoring:** Extract Function — give the label scheme one owner next to `sanitize_window_label`: `pub fn auth_window_label(origin: &str) -> String { format!("auth-{}", sanitize_window_label(origin)) }`. Both `windows.rs:82` and `commands.rs:120` call it. Optionally Extract a tiny `fn close_window(app, label)` for the repeated get-or-close teardown (`commands.rs:121`, `windows.rs:110`, `commands.rs:257`).

**What disappears:** the duplicated `"auth-"` prefix literal and the parallel `format!` calls collapse to one function; the label contract becomes single-sourced so the open-site and close-site can never disagree; the three copies of get-window-then-close shrink to one helper.

**Instances:** label: `packages/accelerator/src-tauri/src/windows.rs:82`, `packages/accelerator/src-tauri/src/commands.rs:120`. Close-by-label teardown: `commands.rs:121–123`, `windows.rs:110–112`, `commands.rs:257–259`.

---

## Finding 4 — `AppState` / `HeadlessState` construction duplicated across the GUI and headless binaries (coordinator hand-off)

**Smell:** Duplicate Code (Dispensable), cross-cluster. The `AppState { core: Arc::new(HeadlessState { … prove_semaphore: Some(Arc::new(Semaphore::new(1))), app_version: Some(env!("CARGO_PKG_VERSION")…), … }), .. }` assembly is hand-written in three production sites: the GUI setup (this cluster), the headless server binary, and the core `start` doctest/helpers.

**Maintenance impact:** local-to-structural, but **the two instances live in different crates/clusters**, so I flag it primarily for the coordinator to decide where the shared constructor belongs. Blast radius: `src-tauri/src/main.rs` (C4) + `accelerator/server/src/main.rs` (a different cluster) + the core `HeadlessState` definition. Change frequency: every time a *required* core field is added — e.g. when `prove_semaphore` or `app_version` was introduced, every constructor site had to opt in by hand, and a site that forgets simply gets `None` and silently degrades (no popup / no semaphore).

**Concrete evidence:**
- GUI build: `main.rs:345–367` — `core: Arc::new(HeadlessState { bundled_version: Some(bundled_version), app_version: Some(env!("CARGO_PKG_VERSION").to_string()), https_bound: Default::default(), config: Some(config_state.clone()), auth_manager: Some(auth_manager.clone()), prove_semaphore: Some(Arc::new(tokio::sync::Semaphore::new(1))) })`
- Headless build: `packages/accelerator/server/src/main.rs:62–75` — same shape: `core: Arc::new(HeadlessState { auth_manager, config, prove_semaphore: Some(Arc::new(tokio::sync::Semaphore::new(1))), app_version: Some(env!("CARGO_PKG_VERSION").to_string()), bundled_version: std::env::var("AZTEC_BB_VERSION").ok(), ..Default::default() })`

The repeated fragments are the `prove_semaphore: Some(Arc::new(Semaphore::new(1)))` line (the "limit proving to 1" invariant, restated verbatim) and the `app_version: Some(env!("CARGO_PKG_VERSION").to_string())` line. Both must restate the semaphore-of-1 invariant; if the concurrency policy changes (e.g. configurable parallelism), every constructor changes.

**Why it harms future change:** the semaphore-of-1 invariant is policy that should live in one factory, not be copy-pasted into each binary's `main`. Adding a new mandatory core dependency means hunting down every `HeadlessState { … }` literal; a missed site compiles fine (`..Default::default()` fills `None`) and fails only at runtime. `env!("CARGO_PKG_VERSION")` also resolves per-crate, so the two sites are subtly *not* interchangeable — exactly the trap that makes "just extract it" non-trivial and worth a coordinator decision.

**Smallest safe refactoring:** Extract Function onto the core type — e.g. `HeadlessState::with_proving_defaults(config, auth_manager) -> HeadlessState` (or a `HeadlessStateBuilder`) that sets `prove_semaphore` to the canonical `Semaphore::new(1)` and leaves version fields as caller-injected params (since `CARGO_PKG_VERSION` must come from the *binary* crate, not core). Both `main`s call it; the GUI additionally attaches its 3 callbacks.

**What disappears:** the restated semaphore-of-1 invariant and the per-site `prove_semaphore`/version boilerplate collapse to one constructor; adding a mandatory core field becomes a compile-time-enforced change to one function signature instead of a silent `None` in a forgotten literal.

**Instances:** `packages/accelerator/src-tauri/src/main.rs:345–367`; `packages/accelerator/server/src/main.rs:62–75` (out-of-cluster — coordinator to site the shared constructor in `accelerator-core`).
