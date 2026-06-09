# C1 core-server — quality findings (Claude)

**Cluster verdict:** 3 findings. One structural duplication smell with real change-amplification (shared-state construction), one local Temporal Coupling in the prove status sequence, one local Feature Envy in `compute_threads`. No Long Method / Large Class worth flagging — the Q2 split already decomposed the old monolith into `prove.rs` / `auth.rs` / `bind.rs` / `probe.rs`, and each prod fn is cohesive and short. Severity spread: 1 structural, 2 local. The `(StatusCode, String)` error-return pattern was examined and is a NON-FINDING (idiomatic Rust `Result`, and the `text/plain` wire shape is a documented, test-pinned contract — see note at end).

---

## Finding 1 — `HeadlessState`/`AppState` hand-constructed at every call site with no constructor

1. **Title** — Shared server state struct is hand-rolled in all three binaries; no `HeadlessState`/`AppState` constructor or builder.

2. **Smell** — **Duplicate Code** (Dispensables), with a **Data Clumps** sub-smell. The same field cluster — `prove_semaphore: Some(Arc::new(Semaphore::new(1)))`, `app_version: Some(env!("CARGO_PKG_VERSION").to_string())`, `auth_manager`, `config`, wrapped in `AppState { core: Arc::new(HeadlessState { … }), .. }` — is re-spelled at each of three construction sites. There is no `HeadlessState::new(...)` / builder / `with_defaults` (confirmed: `rg` for `impl HeadlessState` / `fn new` on the struct returns nothing; the only shared factory is `#[derive(Default)]`, which can't express the non-`Default` semaphore=1 / version-injection defaults).

3. **Maintenance impact** — **structural**. Blast radius: 3 files (`packages/accelerator/server/src/main.rs`, `packages/accelerator/src-tauri/src/main.rs`, plus the in-file test helper `auth_state_with_popup` + the success-path test in `core/src/server.rs`). Change frequency: moderate-to-hot — this is the single integration seam every new server-state field must thread through, and the codebase is actively adding fields here (the inline comments show `https_bound` (Q7), `app_version` (Phase 0), `bundled_version` (Phase 2/3) were each bolted on in sequence).

4. **Concrete evidence** —
   - `packages/accelerator/server/src/main.rs:62-75` — headless construction: `AppState { core: Arc::new(HeadlessState { auth_manager, config, prove_semaphore: Some(Arc::new(tokio::sync::Semaphore::new(1))), app_version: Some(env!("CARGO_PKG_VERSION").to_string()), bundled_version: std::env::var("AZTEC_BB_VERSION").ok(), ..Default::default() }), ..Default::default() }`.
   - `packages/accelerator/src-tauri/src/main.rs:345-367` — GUI construction: same `AppState { core: Arc::new(HeadlessState { … prove_semaphore: Some(Arc::new(tokio::sync::Semaphore::new(1))), app_version: Some(env!("CARGO_PKG_VERSION").to_string()), … }) }`, here filling all six core fields plus the three GUI callbacks.
   - `packages/accelerator/core/src/server.rs:726-738` (`auth_state_with_popup` test helper) and `:641-650` (`prove_success_path_and_status_sequence`) — re-spell `prove_semaphore: Some(Arc::new(Semaphore::new(1)))` and the same `AppState { core: Arc::new(HeadlessState { … }) }` shape yet again.
   - Duplicated literal logic, every site: the semaphore is *always* `Semaphore::new(1)` (the "limit proving to 1, bb uses all cores" invariant is stated in 3 comments — `core/src/server.rs:97`, `prove.rs:142`, and implied at each construction — but enforced by copy-paste, not a factory); `app_version` is *always* `env!("CARGO_PKG_VERSION")` in both real binaries (server/main.rs:67, src-tauri/main.rs:348).

5. **Why it harms future change** — Adding one server-state field (the pattern the comments show is recurrent) is **Shotgun Surgery**: you must edit all three real construction sites or the field silently falls back to its `Default` in whichever binary you forgot — and because `..Default::default()` swallows the omission, the compiler will NOT catch it. The semaphore-of-1 invariant is especially fragile: nothing stops a future site from writing `Semaphore::new(4)` and breaking the "bb already uses all cores" contract, because the "1" is a magic literal repeated per site rather than owned by a constructor.

6. **Smallest safe refactoring** — **Introduce Parameter Object / Extract Factory**: add `impl HeadlessState { pub fn new(config, auth_manager, app_version, bundled_version) -> Self }` (or a small builder) that bakes in `prove_semaphore: Some(Arc::new(Semaphore::new(1)))` and `https_bound: default` once. The headless and GUI binaries call it; the two test helpers call it. `AppState` keeps `Default` for the GUI-callback fields.

7. **What disappears** — The repeated `Some(Arc::new(Semaphore::new(1)))` / `Some(env!("CARGO_PKG_VERSION").to_string())` literals; the `..Default::default()`-hides-an-omission footgun; the 3-site edit cost when a core field is added (collapses to 1 signature + N call-site arg additions the compiler now enforces).

8. **Instances** —
   - `packages/accelerator/server/src/main.rs:62-75`
   - `packages/accelerator/src-tauri/src/main.rs:345-367`
   - `packages/accelerator/core/src/server.rs:726-738` (test helper)
   - `packages/accelerator/core/src/server.rs:641-650` (test)

---

## Finding 2 — `/prove` re-emits `ServerStatus::Proving` because `resolve_version` clobbers it with `Downloading`

1. **Title** — Proving status is set, possibly overwritten by a nested helper, then re-set — split across two functions in a fixed, undocumented order.

2. **Smell** — **Temporal Coupling** (named analog; mapping below) with a **Feature Envy** tinge. `prove` sets `on_status(Proving)` at `prove.rs:160-162`, then calls `resolve_version`, which — on the download branch — sets `on_status(Downloading)` (`prove.rs:66-68`) and is then *obligated* to set `on_status(Proving)` again before returning (`prove.rs:94-96`) so the caller's Proving state is restored. The two functions must agree on who owns the status and in what order; neither signature nor type encodes it. Temporal Coupling: the calls only produce the correct tray sequence if executed in this exact interleaving, and the correctness lives in a comment-free implicit contract spanning the function boundary.

3. **Maintenance impact** — **local**, but on a **hot path** (every `/prove` request, the app's core flow). Blast radius: 2 functions in 1 file (`core/src/server/prove.rs`). The exact status string sequence is test-pinned (`prove_success_path_and_status_sequence`, `core/src/server.rs:626-685`), so getting the interleaving wrong fails CI — which is good for safety but means the coupling is also **Test Brittleness**: an innocent refactor that moves the Proving emission will redden a characterization test whose intent ("bundled path sets Proving, guard resets to Idle") is non-obvious.

4. **Concrete evidence** —
   - `packages/accelerator/core/src/server/prove.rs:160-162` — `prove` sets `cb(ServerStatus::Proving)`.
   - `packages/accelerator/core/src/server/prove.rs:66-68` — `resolve_version` (download branch) sets `cb(ServerStatus::Downloading)`, *replacing* the Proving the caller just set.
   - `packages/accelerator/core/src/server/prove.rs:94-96` — `resolve_version` sets `cb(ServerStatus::Proving)` *again* on the way out, solely to undo line 66's side effect for the caller's benefit. The envied data is `state.on_status` (lives on `AppState`/`HeadlessState` in `core/src/server.rs:107`); `resolve_version` reaches into it three times (`:66`, `:94`) to manage a status the *caller* is also managing.
   - The non-download branch of `resolve_version` never touches status, so `prove`'s line-160 Proving stands — meaning the same helper has two different status post-conditions depending on an internal branch, which the caller cannot see.

5. **Why it harms future change** — Add a third phase (say a `Verifying` status, or a "warming bb" step) and you must reason about *all three* emission points and the implicit "helper restores caller's state" rule, or the tray flickers/sticks. Moving `resolve_version` to before the semaphore acquire, or memoizing downloads, risks emitting `Downloading`→(no restore)→stuck. The status-state machine is real but smeared across two functions with no single owner.

6. **Smallest safe refactoring** — **Extract the status-phase transitions into a guard/owner**: have `resolve_version` return a "did we download" signal (or take no status responsibility at all) and let `prove` own the full `Idle→Downloading→Proving→Idle` sequence in one place — mirroring the existing `StatusGuard` (`prove.rs:20-30`) that already centralizes the Idle reset. I.e. **Move Function** of the status side-effects up to the single caller, leaving `resolve_version` to do only version resolution + download.

7. **What disappears** — The redundant `cb(ServerStatus::Proving)` at `prove.rs:94-96` (it exists only to repair line 66); the cross-function ordering contract; `resolve_version`'s envy of `state.on_status`. The status machine becomes readable in one function.

8. **Instances** —
   - `packages/accelerator/core/src/server/prove.rs:160-162` (set Proving)
   - `packages/accelerator/core/src/server/prove.rs:66-68` (clobber → Downloading)
   - `packages/accelerator/core/src/server/prove.rs:94-96` (restore → Proving)
   - Pinned by `packages/accelerator/core/src/server.rs:626-685`.

---

## Finding 3 — `compute_threads` is a method living on the free-function side, operating entirely on `config::Speed`

1. **Title** — `compute_threads` reaches through `AppState → config → Speed` to re-derive what `Speed` already knows.

2. **Smell** — **Feature Envy** (Couplers). `compute_threads(state)` (`core/src/server/prove.rs:104-113`) uses nothing from `state` except to reach `config.read().speed`, then branches on `speed.is_full()` vs `speed.to_threads()`. The whole body is logic *about a `Speed`*, parked on the server side. The envied data + behavior (`is_full`, `to_threads`) already live on `Speed` (`core/src/config.rs:16-34`).

3. **Maintenance impact** — **local / cosmetic-to-local**. Blast radius: 2 files (`core/src/server/prove.rs`, `core/src/config.rs`). Change frequency: low (speed model is stable). Flagging because it's a clean, low-risk consolidation that also removes a `RwLock` read + `Option` dance from the prove hot path's helper.

4. **Concrete evidence** —
   - `packages/accelerator/core/src/server/prove.rs:104-113` — `state.config.as_ref().and_then(|cfg| { let cfg = cfg.read(); if cfg.speed.is_full() { None } else { Some(cfg.speed.to_threads()) } })`. The only field of `state` touched is `config`, and only to extract `speed`.
   - `packages/accelerator/core/src/config.rs:18` (`to_threads`) and `:32` (`is_full`) — the data and the two operations the helper composes already belong to `Speed`.
   - The "None ⟺ Full" mapping is a property of `Speed`, not of the server.

5. **Why it harms future change** — Add a speed tier, or change the "Full means bb-default threads (None)" rule, and you edit `Speed` in `config.rs` AND remember this server-side helper re-encodes the Full→None decision. The thread-derivation rule is duplicated in spirit: `Speed` owns `to_threads`/`is_full`, but the "Full→None, else Some(to_threads)" composition lives in `prove.rs`. That's a two-site rule that should be one.

6. **Smallest safe refactoring** — **Move Method** the Full→`Option<usize>` mapping onto `Speed`: add `Speed::thread_limit(self) -> Option<usize>` (`None` for Full, else `Some(self.to_threads())`) in `config.rs`. `compute_threads` collapses to `state.config.as_ref().map(|c| c.read().speed).and_then(Speed::thread_limit)` — or inline at the single call site (`prove.rs:168`).

7. **What disappears** — The server-side re-encoding of the Full→None rule; one of the two places that must change when the speed model changes; the helper's reach-through into `config.speed`. The `Speed` type becomes self-describing for "how many threads, or let bb decide."

8. **Instances** —
   - `packages/accelerator/core/src/server/prove.rs:104-113` (the helper)
   - `packages/accelerator/core/src/server/prove.rs:168` (sole caller)
   - target type: `packages/accelerator/core/src/config.rs:16-34`

---

## Examined, NOT flagged (NON-FINDINGS)

- **`json_error` returning `String` → `(StatusCode, String)` "error-as-control-flow":** This is idiomatic Rust `Result<_, ProveError>` propagation via `?`, not error-as-control-flow in the smell sense. Crucially, the `text/plain` (not `axum::Json`) wire shape is a **deliberate, documented, test-pinned contract** (`core/src/server.rs:192-208` doc comment + `prove_error_responses_stay_text_plain_json_string` at `:533`): the SDK's `ky` client keys error parsing on Content-Type. "Refactoring" it to `axum::Json` would be a behavior change, not a cleanup. Leave it.
- **Long Method / Large Class:** `prove` (~90 LOC incl. comments, `prove.rs:117-207`) reads linearly as authorize → buffer → permit → resolve → run → encode, with the genuinely separable pieces (`authorize_origin`, `resolve_version`, `compute_threads`) already extracted by Q2. Not worth a further split. `server.rs` prod surface is ~210 LOC (rest is tests); not a Large Class.
- **Duplicate startup `tracing::error! + exit(1)`** (server/main.rs:77-80 vs the GUI's richer AddrInUse-classifying block at src-tauri/main.rs:400+): these are genuinely *different* policies (headless = fail hard; GUI = classify redundant-instance and exit 0). Not duplication.
- **`ServerStatus` enum + `display_text`/`is_busy`:** purpose-built to *replace* stringly-typed status (Q10, documented `core/src/server.rs:44-72`). The byte-identical-string requirement is a migration constraint, not a smell.
