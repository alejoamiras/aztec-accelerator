# Phase 7 — architectural splits [RISKY CORE PATH] — progress + designs

## Done
- **Q5·1** (#306, MERGED): extracted `#probeAndParseHealth` from `checkAcceleratorStatus` (SDK). Single
  surgical split (cache fast-path stays in caller; body moved verbatim, no re-indent). 26 SDK tests + tsc.
- **Q14** (#307, armed): `is_auto_approved` rewritten to reuse `url::Url` parsing (Substitute Algorithm)
  instead of hand-rolled prefix-strip/`:`-split. **Ask-B resolved: PURE REFACTOR** — the auto-approve set
  is unchanged (the decider tests `auto_approved_localhost_variants` incl. `[::1]` + `non_localhost_not_
  auto_approved` stay green; `url::Url::host_str()` returns `[::1]` WITH brackets). Nothing to surface.

## rc-dry-run timing (correction)
The 1.0.x-rc dry-run is a **pre-stable-cut BATCH gate**, NOT per-phase: it validates Q4+Q6+Q7+Q1+Q2
together before the next stable cut (plan: "the server/updater/safari ones gate on an rc dry-run before
the next stable cut"). Dispatching it after Q4 alone is premature — Q4 doesn't even touch updater.rs, so
the updater-smoke path is unchanged. Run ONE rc-dry-run once the risky/safety-critical set is complete.

## Q1 — `AppState` split [KEYSTONE, Q2 depends on it] — COMPLETE DESIGN
Central state on the hot `/prove` path → multi-file restructure. NOT a single-file crank.

**Target shape:**
```rust
pub struct HeadlessState {           // the 5 server-relevant fields
    pub bundled_version: Option<String>,
    pub https_port: Option<u16>,
    pub config: Option<Arc<RwLock<config::AcceleratorConfig>>>,
    pub auth_manager: Option<Arc<AuthorizationManager>>,
    pub prove_semaphore: Option<Arc<Semaphore>>,
}
pub struct GuiCallbacks {            // the 3 GUI callbacks (present iff a GUI is wired)
    pub on_status: StatusCallback,
    pub on_versions_changed: VersionsChangedCallback,
    pub show_auth_popup: ShowAuthPopupCallback,
}
#[derive(Clone, Default)]
pub struct AppState { pub core: Arc<HeadlessState>, pub gui: Option<Arc<GuiCallbacks>> }
```

**Blast radius (measured):**
- **~12 core accesses** — uniform rewrite `state.<f>` → `state.core.<f>` (config×5, https_port×3,
  auth_manager×2, prove_semaphore×1, bundled_version in resolve_version).
- **~7 gui accesses** — the intricate part: `state.on_status` (was `Option<Callback>`) →
  `state.gui.as_ref().map(|g| &g.on_status)` (the Option now lives on `gui`, not per-field). on_status×4,
  show_auth_popup×2, on_versions_changed×1. Each `if let Some(ref cb) = state.on_status` site becomes
  `if let Some(g) = state.gui.as_ref()` then use `g.on_status`. THE care-sensitive sites (the prove
  handler's status emits, the auth-popup trigger).
- **Construction:** headless (server/src/main.rs:62 — 3 fields + Default) →
  `AppState { core: Arc::new(HeadlessState{auth_manager, config, prove_semaphore, ..Default::default()}), gui: None }`.
  GUI (main.rs:347 — all 8) → core fields into `HeadlessState`, the 3 callbacks into
  `Some(Arc::new(GuiCallbacks{...}))`. Plus `state_with_https.https_port = ...` (main.rs:377) →
  needs HeadlessState mutation before the Arc (build HeadlessState, set https_port, then Arc::new).
- **~8 test construction sites** in server.rs (most are `AppState::default()` or `..Default::default()`
  with 1-2 fields) + the Phase-0 char tests that set `on_status`/`prove_semaphore` → update to the new shape.

**Validation:** `cargo test --lib` (the Phase-0 char tests — prove ordering, status sequence — are the
behavior guard) + `cargo check --bin` + `cargo check -p accelerator-server` (the headless crate consumes
AppState — MUST stay green) + clippy. Then Q2 (server.rs module split) builds on this.

**Why a focused pass, not deep-tail:** ~19 access rewrites across server.rs handlers + main.rs + the
headless crate + Default + the gui-Option-of-struct intricacy. A compaction mid-rewrite leaves a broken
hot path across 3 files. Implement in one focused pass with full budget; the char tests are the net.

## Remaining P7
Q1 (designed↑) → Q2 (server.rs → bind.rs/tls.rs/handlers/, depends on Q1) → Q6 (UpdateCoordinator,
safety-critical) → Q7 (SafariSupportManager + the missing-recovery Ask-B fix, safety-critical) → Q5·2
(PhaseReporter, low-value churn). Then ONE rc-dry-run (Q4+Q6+Q7+Q1+Q2) before any stable cut.
LESSONS_FILE=implementations-plan/quality-refactor-2026-06-05/lessons/phase-7.md
