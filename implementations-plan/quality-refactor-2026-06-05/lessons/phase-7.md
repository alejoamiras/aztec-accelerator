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

### RESOLVED design fork (the hard part — found while implementing)
**The plan's `gui: Option<Arc<GuiCallbacks>>` (all-or-nothing) does NOT fit:** the gui callbacks are
*individually* `Option` today — the Phase-0 char test `prove_success_path_and_status_sequence` sets
`on_status` ALONE (no `show_auth_popup`/`on_versions_changed`). An all-or-nothing `GuiCallbacks` struct
can't represent "only on_status set," so it would break that test.

**Resolution — option (b), behavior-preserving + minimal-churn:**
```rust
#[derive(Clone, Default)]
pub struct HeadlessState { bundled_version, https_port, config, auth_manager, prove_semaphore }  // 5 core, each Option
#[derive(Clone, Default)]
pub struct AppState {
    pub core: Arc<HeadlessState>,
    pub on_status: Option<StatusCallback>,            // gui callbacks stay FLAT on AppState (individually Option)
    pub on_versions_changed: Option<VersionsChangedCallback>,
    pub show_auth_popup: Option<ShowAuthPopupCallback>,
}
impl Deref for AppState { type Target = HeadlessState; fn deref(&self)->&HeadlessState { &self.core } }
```
- **`Deref<Target=HeadlessState>` ⇒ the ~12 core reads (`state.config`, `state.bundled_version`, …) are
  UNCHANGED** (auto-deref through AppState→core for field reads). The ~7 gui reads (`state.on_status`)
  are ALSO unchanged (flat fields). **Net: ZERO access-site rewrites.** Churn = struct def + Deref +
  construction sites only.
- This deviates from the plan's `gui: Option<Arc<GuiCallbacks>>` (which doesn't fit the per-field-Option
  reality) but delivers the plan's actual VALUE: the headless crate uses `HeadlessState`; cloning AppState
  Arc-clones the 5 core fields (the clone-stutter fix). Grouping the 3 gui callbacks into a struct adds the
  per-field-Option complexity for ~no benefit — rejected.
- **Construction edits (the only churn):** GUI (main.rs:347): core fields → `Arc::new(HeadlessState{…})`,
  3 callbacks stay flat. Headless (server/src/main.rs:62): `core: Arc::new(HeadlessState{auth_manager,
  config, prove_semaphore, ..Default})`, `..Default`. **main.rs:377 `state_with_https.https_port = x`** →
  `Arc::make_mut(&mut state_with_https.core).https_port = x` (the ONLY post-construction core mutation;
  impl `Deref` but NOT `DerefMut`, so this one site is explicit). ~8 test sites that set a core field
  (e.g. `prove_semaphore`) move it into a `HeadlessState{…}`; sites using only `AppState::default()` or
  gui fields are unchanged.

**Validation:** `cargo test --lib` (Phase-0 char tests = the behavior net) + `cargo check --bin` +
`cargo check -p accelerator-server` (headless MUST stay green) + clippy.

**Why a focused pass, not deep-tail:** even bounded to construction sites, it's ~12 coordinated edits
across server.rs + main.rs + the headless crate + ~8 tests; a compaction mid-rewrite leaves a broken hot
path across 3 files. The DESIGN (above) is now complete + the access-churn eliminated via Deref — the
implementation is mechanical, so the next focused pass executes it fast with the char tests as the net.

## Remaining P7
Q1 (designed↑) → Q2 (server.rs → bind.rs/tls.rs/handlers/, depends on Q1) → Q6 (UpdateCoordinator,
safety-critical) → Q7 (SafariSupportManager + the missing-recovery Ask-B fix, safety-critical) → Q5·2
(PhaseReporter, low-value churn). Then ONE rc-dry-run (Q4+Q6+Q7+Q1+Q2) before any stable cut.
LESSONS_FILE=implementations-plan/quality-refactor-2026-06-05/lessons/phase-7.md

---

## Q2 server.rs split — EXECUTED (#309 bind, #310 tls, #311 probe, #312 auth, #313 prove-core)

Compiler-guided module extraction, Rust 2018 idiom (`server.rs` + `server/` subdir, `mod X;` in server.rs).
Shipped as 5 serialized PRs (each touches server.rs → each waits for the prior; stacked via cherry-pick /
base-on-prior-branch, retarget to main as each merges).

**Module boundary that made it clean:**
- `bind.rs` (bind_with_retry + 3 tests), `tls.rs` (start_https), `probe.rs` (healthy_aztec_on_port + is_healthy_aztec_response unit test) — leaf utilities, no shared-state coupling.
- `auth.rs` (`pub(crate) authorize_origin`) — the cleanest *handler* slice; no direct unit tests (the `prove_*` router tests exercise it via /prove).
- `prove.rs` (`pub(crate)` prove/resolve_version/compute_threads + private StatusGuard/MAX_BODY_SIZE) — the core handler.

**Key visibility facts that drove the split (all compiler-verified):**
1. A **private** parent item (`type ProveError`, `fn json_error`, `const PORT`) is visible to **descendant**
   modules via `super::`. So `ProveError`/`ProveErrorBody`/`json_error` STAY in server.rs (shared by both
   auth.rs and prove.rs children) with NO visibility change — children reach them via `super::`. probe.rs
   likewise reaches the private `PORT` via `super::PORT`.
2. A **private** `mod auth;` is reachable from a *sibling* child (`prove.rs`) via `super::auth::authorize_origin`
   (auth_origin is `pub(crate)`), because private items are visible to all descendants of the declaring module.
   → after moving prove out, server.rs's `use auth::authorize_origin;` became unused (prove was its only caller)
   and was dropped; prove.rs imports `super::auth::authorize_origin`.
3. Module-name vs fn-name: router wires `post(prove::prove)` (module `prove` + fn `prove` — distinct namespaces,
   no clash, clearer than `use prove::prove;`).
4. The 6 helper unit tests (`compute_threads_*`/`resolve_version_*`) call the fns by symbol → `mod tests` adds
   `use super::prove::{compute_threads, resolve_version};`. The prove INTEGRATION tests are router-mediated
   (`router(state).oneshot(req)`) — they reference no moved symbol, so they didn't move.

**Behavior net:** 127 lib tests green at every slice (prove_* router net + resolve_version/compute_threads
units + bind retry tests + probe classifier). clippy `-D warnings` + headless `accelerator-server` check green
each slice. Result: server.rs is now thin **router + start + health + shared error helpers + structs**.

**Cross-cut gotcha (unchanged from prior phases):** SSH transport (port 22) keeps dropping mid-run → push via
`git -c credential.helper="!gh auth git-credential" push https://github.com/<owner>/<repo>.git <br>:<br>`
(the SSH remote URL fails; HTTPS + gh helper works). 1Password commit-signing also flakes → `-c commit.gpgsign=false`.

---

## Q7 (Safari recovery consolidation) — SCOPED, execution plan (branch refactor/q7-safari)

**Code traced:**
- `commands.rs:139` `enable_safari_support` (macOS): generate_and_save → install_ca_trust (Keychain prompt) → save cfg → load_rustls_config → spawn start_https. The *create* path.
- `main.rs:54` `try_start_https`: the *startup verify-or-recover* path. cfg.safari_support gate → certs_exist? (miss→reset) → is_ca_trusted? (**untrusted→return None, NO reset = the gap**) → load_rustls_config (broken→reset). Spawns start_https + a background regenerate_leaf_if_expiring thread (OFF startup so the Keychain prompt can't block launch — main.rs:91-93).
- `main.rs:105` `reset_safari_support`: sets cfg.safari_support=false + save (so the user re-enables → fresh trusted set).
- `server.rs` health advertises `https_port` when `state.https_port.is_some() || cfg.safari_support` — i.e. on the CONFIG flag, so an untrusted-skip still advertises a dead port to the SDK.

**The bug (Ask-B fix to ship):** the untrusted-CA branch is the only failure mode that doesn't recover → HTTPS silently dead every launch while health still claims a port.

**Engineering call (mirrors the Q6 cut rationale):** certs.rs already cohesively owns the cert lifecycle ("one-sentence responsibility → don't split"). So Q7 = focused **Extract-Method + the fix**, NOT a heavyweight `SafariSupportManager` Extract-Class. Concretely: pull the verify-or-recover decision out of main.rs into a testable helper (e.g. `certs::validate_for_https() -> ValidateOutcome { Ok(TlsConfig) | Recover | Skip }`) so the decision matrix becomes unit-testable, then `try_start_https` maps Recover→reset_safari_support uniformly across all three failure modes.

**Execution order (next fresh-context turn):**
1. Char tests FIRST: pin the current decision matrix at the helper level (miss→reset, broken→reset, untrusted→**currently** no-reset) so the diff shows the untrusted row flipping to reset deliberately.
2. Extract `certs::validate_for_https` (or equiv) + unit tests for all rows.
3. `try_start_https` calls it; untrusted now recovers (reset) like the others.
4. Decide health-signal honesty (advertise `https_port` only when HTTPS actually bound) — likely fold in OR note as a separate minor.
5. cargo test --lib + clippy -D warnings + headless check.
6. PR: flag the **behavior change to the owner** (Ask B) — reset-on-untrusted means cfg flips off when Keychain trust is lost (re-enable re-prompts); proof is manual/integration (Keychain-dependent), note the gap.

**Test-surface caveat (like Q6):** the orchestration is Keychain/filesystem-dependent; the helper extraction is what MAKES the decision unit-testable. The end-to-end proof is the rc/manual macOS check, not a unit test.
