# Harden Report: quality

**Repo:** aztec-accelerator
**Date:** 2026-06-05
**Effort:** high
**Run ID:** 2026-06-05-sdk-accel
**Models:** Phase 1 map = Explore (Sonnet); Phase 2 finders = 6× Claude Sonnet (per cluster) + 2× Codex xhigh (cross-model, batched Rust / SDK+frontend); Phase 3 reduce + Phase 4 verify = Opus (this agent), anchors line-verified against source.
**Scope:** `packages/sdk` (TS) + `packages/accelerator` Rust backend (`src-tauri/src`, `server/src`), frontend (`src-tauri/frontend`), and `scripts/copy-bb.ts`. Excluded: `target/`, `gen/`, `dist/`, `node_modules/`, `icons/`, `binaries/`, and `*.test.ts`/`e2e` (non-finding-eligible unless production-wired).

## Executive summary

The codebase is **healthy** — clear module boundaries, no circular dependencies, strong test coverage (~122 Rust unit tests, ~29 SDK unit tests, Playwright + WebDriver E2E), and recent security/correctness hardening (the #98 audit, the keyless-CA TLS rework, and the #99 download fixes). This is a maintainability pass, not a correctness one: every finding below is about *future change cost*, and nothing here is a bug.

The dominant theme, flagged independently by **both** model families, is that the project's two biggest surfaces — `server.rs` (1335 LOC) and the SDK's `accelerator-prover.ts` (434 LOC) — have grown into **god-objects whose request/proof pipelines are long procedural methods**, and that **three core domain concepts are passed around as primitives**: Aztec *version strings*, the *HTTP error/header protocol*, and *server status* (which drives the tray animation via substring matching). A second theme is **boilerplate duplication** with one latent inconsistency baked in: the config "lock → mutate → save" sequence is copy-pasted across 6 command handlers, and one copy (`respond_update_prompt`) silently *swallows* a save error where the other five propagate it.

Recommended priority: (1) introduce two small value objects — `AztecVersion` and a typed server-status enum — which between them dissolve the largest cluster of findings; (2) extract the `/prove` + `authorize_origin` workflow and split `AppState` into headless-core + GUI-adapter; (3) sweep the cheap local duplications (shared 60s-timeout const, `mutate_config` helper). None of this is urgent; all of it compounds if the surfaces keep growing.

## Methodology

Map-reduce per the harden protocol. Two parallel `Explore` mappers (one per package) built the repo map; the main agent clustered into 6 bounded units (sdk / server.rs / versions+bb / certs+crash_recovery / app-shell / config+auth+ui). Each cluster got a Claude Sonnet finder with the quality rubric + negative list; cross-model Codex coverage ran as 2 xhigh passes (Rust modules; SDK+frontend) — a deliberate adaptation from per-cluster Codex to keep 6 concurrent xhigh sessions tractable, with cross-model coverage of every file preserved. Phase 3 deduped by root-cause+location, weighting impact by blast radius × change frequency, and marked **cross-model convergence** (both families flagging the same root cause independently) as the primary confidence signal — 13 of 15 primary findings converged. Phase 4 spot-verified the top converged anchors against source (guarding shared hallucination); all confirmed, including the `respond_update_prompt` error-swallow divergence. Inter-procedural context was capped per cluster; the update-flow finding (Q6) came from Codex's wider file set crossing the cap via the emit→handler handoff edge.

---

## Findings

### Architectural

#### [architectural] Q1: `AppState` conflates headless-server and desktop-GUI concerns
**Confidence:** high · **Mapping:** Temporary Field / Data Clump · **Found by:** both
**Instances:** `server.rs:33-43` (7 `Option` fields: `on_status`, `on_versions_changed`, `https_port`, `config`, `auth_manager`, `show_auth_popup`, `prove_semaphore`); runtime guards `server.rs:226,248,251,292,324,334,356,382,436,444,513`; partial constructors `main.rs:347,377`, `server/src/main.rs:43,62`.
One struct serves both the desktop app (callbacks present) and the headless CI server (callbacks `None`). Every handler re-derives the headless-vs-GUI distinction at runtime via `Option::is_some`, and every new capability adds another `Option<…>` field + another guard + another `..Default::default()` in tests.
**Why it matters:** highest change-frequency surface — every server feature threads through `AppState`. **Fix:** Extract Class — `AppState { base: HeadlessState, gui: Option<GuiCallbacks> }`; handlers that don't need GUI take `&HeadlessState`. **Effort:** ~1 day.

#### [architectural] Q2: `/prove` + `authorize_origin` are a distributed procedural script
**Confidence:** high · **Mapping:** Long Method / Large Class · **Found by:** both
**Instances:** `prove()` `server.rs:487-577`; `authorize_origin()` `server.rs:288-406` (6 ordered phases); version resolution `server.rs:409-470`. `server.rs` itself is 578 production LOC owning ~6 unrelated responsibility clusters (bind-retry, TLS accept loop, CORS setup, auth glue, version resolve, prove orchestration).
A single conceptual change ("queue proves per origin", "show an auth countdown", "support cancellation/progress") cuts across HTTP parsing, auth, popup lifecycle, status updates, and proving — all interleaved.
**Why it matters:** the app's core request path; also unit-untestable at phase granularity (needs full `AppState` + channels). **Fix:** Extract Class `ProveWorkflow` + Extract Method on the auth phases; split `server.rs` into `bind`/`tls`/`handlers` submodules. **Effort:** ~2-3 days.

#### [architectural] Q3: Aztec version is a raw `&str` at every seam (Primitive Obsession)
**Confidence:** high · **Mapping:** Primitive Obsession · **Found by:** both
**Instances:** `versions.rs:38` (`from_version` tier parse), `:133` (`version_sort_key`), `:148/:272/:84` (signatures), `:261` (`is_valid_version`), `bb.rs:18/:79`, `server.rs:418/:524`. `versions_to_evict` even re-parses tiers it already computed (`versions.rs:168-170`).
Tier classification and numeric-suffix sorting are two independent string parsers of the same concept; a new prerelease channel (`-staging`, `-alpha`) means editing multiple unrelated match sites consistently.
**Why it matters:** version semantics drive cache, retention, routing, validation — high fan-out. **Fix:** Introduce Value Object `AztecVersion { tier, sort_key, raw }` with `is_valid_version` as its constructor guard; raw `&str` survives only at the HTTP/command boundary. Dissolves Q3 + the `versions_to_evict` re-parse + makes the validated-string invariant type-enforced. **Effort:** ~1-1.5 days.

#### [architectural] Q4: Crash-recovery platform semantics leak out of the adapter
**Confidence:** high · **Mapping:** Divergent Change / Parallel Platform Impls · **Found by:** both
**Instances:** 6 disjoint `#[cfg]` blocks `crash_recovery.rs:23,76,93,163,216,281` with **divergent signatures** (Windows `disable`→`bool`, macOS/Linux→`()`); caller-side policy in `commands.rs:31`, `main.rs:275,294`, `updater.rs:92,99,109,117,125`.
Callers must know the platform to know whether `disable` reported success; adding a 4th platform or a uniform "verify-after-enable" contract touches every caller, not just the backend.
**Why it matters:** updater + autostart both depend on it; the signature divergence is already a real wart. **Fix:** `trait CrashRecovery { fn enable(&self); fn disable(&self) -> bool; }` + a facade with intent-named ops (`arm_for_autostart`, `disarm_for_update`). **Effort:** ~1 day.

#### [architectural] Q5: SDK `checkAcceleratorStatus` is a protocol multiplexer + phases have no model
**Confidence:** high · **Mapping:** Long Method + Temporal Coupling · **Found by:** both
**Instances:** `accelerator-prover.ts:202-319` (118 LOC: cache + dual-protocol probe+retry + multi-version-vs-legacy parse); phase emissions scattered across `createChonkProof` (`:331-404`) + `#proveLocally` (`:417,422`) — 14 sites, legal orderings enforced nowhere.
A third transport, a new denial reason, or a reordered phase means reopening one 118-LOC method and manually preserving a valid sequence across native/local/denied branches.
**Why it matters:** the SDK's public contract for all proof progress. **Fix:** Extract Method (`#probeHealth`, `#parseHealthResponse`) + a small `PhaseReporter` with declared transitions. **Effort:** ~1 day.

#### [architectural] Q6: Update flow is a temporal state machine spread across 4 modules
**Confidence:** moderate · **Mapping:** Temporal Coupling / Shotgun Surgery · **Found by:** codex (cross-model disagreement — Claude's app-shell cluster saw the pieces but not the cross-module whole)
**Instances:** shared `PendingUpdate` state `commands.rs:17`; poll/store/show `main.rs:150,157,168`; prompt window `windows.rs:88`; user response `commands.rs:206`; policy/install `updater.rs:14,61`.
"Skip this version" / retry / download-progress each require coordinated edits to state storage, UI, command handling, and updater policy. **Fix:** Introduce State Object `UpdateCoordinator` owning the transitions. **Effort:** ~1 day.

#### [architectural] Q7: Safari/HTTPS support is split between startup repair and settings commands
**Confidence:** moderate · **Mapping:** Shotgun Surgery · **Found by:** both (Codex framed the whole; Claude flagged the concrete HTTPS-spawn duplication)
**Instances:** startup preflight/start/repair `main.rs:54,71,83,95,105,373`; settings enable/disable `commands.rs:134,141,152,157,170`. The spawn-HTTPS sequence is duplicated (`main.rs:83-89` has a `reset_safari_support` recovery path; `commands.rs:153-162` omits it — a behavioural divergence).
**Fix:** Extract Class `SafariSupportManager` owning cert-load → spawn → repair; both call sites delegate. **Effort:** ~0.5-1 day.

### Structural

#### [structural] Q8: The HTTP contract is stringly-typed (error tuple + `json!` + magic headers)
**Confidence:** high · **Mapping:** Primitive Obsession · **Found by:** both
**Instances:** `ProveError = (StatusCode, String)` `server.rs:280`; **19** `json!(...)` error assemblies, 7 of which bypass the `json_error` helper (`server.rs:315,338,351,368,374,398,421,457,505,517,565`); magic header literals `server.rs:206,208,216,526,572` (`x-aztec-version`, `x-prove-duration-ms`).
Adding a field (`request_id`, `Retry-After`), renaming a header, or changing the error schema is a string-hunt instead of a typed edit. **Fix:** `ProveErrorBody { error: &'static str, message: String }` implementing `Serialize` + `IntoResponse`; header-name constants. **Effort:** ~0.5 day.

#### [structural] Q9: Config "lock → mutate → save" boilerplate ×6 — with one error-swallow divergence
**Confidence:** high · **Mapping:** Duplicate Code → Shotgun Surgery · **Found by:** both
**Instances:** `commands.rs:49,60,147,171,195,217` (+ `server.rs:382`, `main.rs:105`). **Latent inconsistency:** 5 sites do `config::save(&cfg).map_err(|e| e.to_string())?` (propagate); `respond_update_prompt` (`commands.rs:219-220`) does `if let Err(e) = config::save(&cfg) { tracing::warn!(...) }` — silently swallows the failure. Copy-paste drift.
**Fix:** `fn mutate_config(&ConfigState, impl FnOnce(&mut AcceleratorConfig)) -> Result<(),String>`; all 6 become one-liners and the swallow becomes a visible, deliberate choice. **Effort:** ~0.5 day. *(Worth doing first — cheap, and it surfaces the divergence.)*

#### [structural] Q10: Tray animation is driven by display copy, not state
**Confidence:** high · **Mapping:** Primitive Obsession (UI state in strings) · **Found by:** both
**Instances:** `main.rs:356` `text.contains("Proving") || text.contains("Downloading")`, coupled to the human-readable strings emitted at `server.rs:437,465,531`. The `StatusCallback = Arc<dyn Fn(&str)>` carries no structured payload.
Reword "Downloading bb…" or add an "Uploading" busy phase and the animation silently breaks. **Fix:** replace the `&str` callback with `enum ServerStatus { Idle, Proving, Downloading, Error(String) }` exposing `is_busy()` + `display_text()`. **Effort:** ~0.5 day.

#### [structural] Q11: `download_bb` owns too many phases
**Confidence:** high · **Mapping:** Long Method · **Found by:** both
**Instances:** `versions.rs:272-421` (149 LOC: guard/cache → bounded stream → digest verify → temp extract → atomic rename → chmod → macOS xattr+codesign, with two separate `remove_dir_all` cleanup arms).
Adding mirror-fallback, retry, progress, or another platform post-step means editing one long block with many exits. **Fix:** Extract `download_tarball`, `verify_digest`, `install_version_dir(bytes, version_dir)` (folding the duplicate cleanup), `postprocess_macos`. **Effort:** ~0.5 day. *(Note: this file was just touched by #99; do this as a separate refactor PR.)*

#### [structural] Q12: SDK `AcceleratorStatus` + phase events are primitives, not types
**Confidence:** high · **Mapping:** Primitive Obsession · **Found by:** both
**Instances:** `accelerator-prover.ts:46-59` (flat interface, 6 optionals = implicit discriminated union); phase events are `string + optional data?` bag (`:11-25,36-43`), payload only on `"proved"`.
Consumers re-derive which field combination they got; the first phase-specific payload (denial metadata, download progress) has nowhere typed to live. **Fix:** discriminated unions for both `AcceleratorStatus` and the phase event (`{ type: "proved", durationMs }`). **Effort:** ~0.5 day.

#### [structural] Q13: `copy-bb.ts` repeats the platform/arch matrix
**Confidence:** high · **Mapping:** Repeated Switches · **Found by:** both
**Instances:** `copy-bb.ts:31-46` (`getTargetTriple`), `:152-170` (platform/ext/arch/os routing), `:172-178` (darwin-only post-step). Plus the Windows path is an extracted function while the Unix path is inlined in `main()` (asymmetric depth).
Adding `windows-arm64` / `linux-musl` means editing multiple branch trees that drift. **Fix:** Replace Conditional with Table — one `TargetSpec` lookup; extract `copyUnixBb` to match `fetchWindowsBb`. **Effort:** ~0.5 day.

#### [structural] Q14: `is_auto_approved` re-implements `canonicalize_origin`'s parser
**Confidence:** high · **Mapping:** Duplicate Code / parallel parsers · **Found by:** claude (single-model)
**Instances:** `authorization.rs:126-147` (manual `strip_prefix` + IPv6-bracket find + `split(':')`) duplicates the `url::Url` parse at `:21-57`. The `is_approved` contract says "input is already canonical" yet `is_auto_approved` re-parses raw input.
Adding a localhost alias means updating two parsers in two idioms. **Fix:** canonicalize once, `matches!` on `url.host_str()`. **Effort:** ~2 hours.

#### [local] Q15: 60s auth-timeout duplicated across modules
**Confidence:** high · **Mapping:** Duplicate Code · **Found by:** both (3 finders)
**Instances:** `server.rs:361` (server-side `timeout`) + `windows.rs:72` (popup auto-deny `sleep`). Two halves of one UX contract; a mismatch produces a hung request or an orphaned popup. **Fix:** `pub const AUTH_DECISION_TIMEOUT: Duration` in the lib crate, imported by both. **Effort:** ~15 min. *(Cheapest win on the board.)*

---

## Findings NOT pursued (with reasoning)

- **`enable/disable_safari_support` `#[cfg]` stubs** (`commands.rs:179-190`) — both a Claude finder and Codex rejected this: the macOS impl and non-macOS stubs are genuinely different (real logic vs error, `async` vs sync); the `#[cfg]` split is correct, no shared shape to extract.
- **`tauri-bridge.js` "duplicated"** — refuted: all 3 HTML pages *do* call `wireToggle`/`wireButton`/`showErrorHint` from the shared bridge. The real (smaller) finding is partial: page *bootstrap* + the speed-control bypass the abstraction (folded into the minor bucket as frontend Global-Data coupling).
- **`style.css` per-popup blocks** — well-factored into shared utility classes; not a smell.
- **In-file Rust test blocks inflating LOC** — Rust co-location convention; not change-amplifying. (Two cosmetic test-quality nits noted: a tautological `assert!(x.is_ok() || x.is_err())` at `server.rs:1259` and two misleadingly-named `prove_*` tests — local, optional.)
- **`is_valid_version` doc-comment placement** (versions.rs) — too weak to action on its own; resolves naturally under Q3.

## Minor cleanups (local/cosmetic — batch into one sweep PR)

SDK WASM-fallback duplicated twice (`accelerator-prover.ts:336,384` → `#fallbackToWasm`); home-dir fallback inconsistent (`"."` `certs.rs:14` vs `"~"` `crash_recovery.rs:84` — the `"~"` literal is a latent bug-shaped smell) → shared `home_dir()`; 3 near-identical window-open helpers (`windows.rs:20,41,88`); `AppState` clone-stutter patching `https_port` twice (`main.rs:347,377,397`); `MAX_BODY_SIZE` named const defined 272 lines after the inline `50*1024*1024` it should replace (`server.rs:213,485`); `bb_asset_name` format duplicated (`versions.rs:121,331`); `write_pem_file` sets `0o600` twice across the rename (`certs.rs:177,193`); two cert tests rebuild CA+leaf verbatim with **stale constants** (`3650` not `CA_VALIDITY_DAYS`, `825` not the `824` production value — `certs.rs:414,448`); `VerifiedSite` carries 3 `#[allow(dead_code)]` fields across a 3-struct parallel hierarchy (`verified_sites.rs:24-34,43-51` + `commands.rs:84`); `default_config_version` serde-default indirection (`config.rs:69`); popup HTML scaffolding duplicated (`authorize.html` ≈ `update-prompt.html:11-30`).

## Cross-cutting observations

1. **Two value objects retire the most debt.** `AztecVersion` (Q3) and a `ServerStatus` enum (Q10) each dissolve a finding *and* several minor items (the `versions_to_evict` re-parse, the tray substring coupling, the validated-string invariant). Highest leverage per unit effort.
2. **The headless/GUI duality is the architectural seam.** Q1 (`AppState`), Q2 (`server.rs` split), and the `Option`-guard sprawl are all the same underlying tension: one type serving two runtimes. Addressing Q1 first makes Q2 cheaper.
3. **Copy-paste has started drifting.** Q9's error-swallow and Q7's missing-recovery-path are both cases where a duplicated block silently diverged. The duplications are individually cheap to fix and each removal closes a divergence class — do these opportunistically alongside feature work.
4. **This is a mature codebase past the risky-refactor window for free.** None of these block shipping. Sequence them behind feature work, leading with the cheap converged wins (Q15, Q9, Q8, Q10) and the two value objects.
