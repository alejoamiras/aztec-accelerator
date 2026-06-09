# Harden Report: quality

**Repo:** aztec-accelerator
**Date:** 2026-06-08
**Effort:** ultra
**Run ID:** 2026-06-08-ultra-e094d8
**Models:** Phase 1 map — Opus ×2 · Phase 2 finders — Opus ×6 + Codex xhigh ×6 (cross-model per cluster) · Phase 2.5 rebuttal — Opus + Codex xhigh (both directions) · Phase 2.6 Round-2 — Codex xhigh (resumed self-critique) · Phase 3 coordinator — Codex xhigh (fresh) · Phase 4 verifier — Opus (this orchestrator, independent re-read)
**Scope:** `packages/accelerator` Rust crates (`core`, `src-tauri`, `server`) + `packages/sdk` (TypeScript public API, weighted). **Excluded:** `packages/playground`, `packages/landing`, generated/vendor/`target`/`node_modules`, `implementations-plan/**`, prior `audit/**`.

---

## Executive summary

The codebase is **healthy**. It recently went through a mega-deep quality refactor (Q1–Q15, merged #307–#322) and a Tauri-free `core` extraction, and it shows: across 6 clusters and 12 cross-model finder passes, only **9 findings** survived — none Critical/Blocker-equivalent, finding density ~1.5/cluster (at the healthy end of the literature's ~1.2 target). Finders repeatedly hit *already-fixed* targets (typed `AztecVersion`, extracted `download_tarball`/`verify_digest`, the discriminated `AcceleratorStatus` union) and logged them as non-findings.

The remaining debt is **concentrated in a few shared seams, not spread as rot** — and three of the top findings share one root pattern: *a critical invariant is enforced by convention/comments instead of by a type or constructor.*

**Top 3:**
1. **F-02 — Canonical origin modeled as raw `String`, with a live production bypass** (the headless server's `ALLOWED_ORIGINS` ingress skips canonicalization entirely). Highest-value: a `CanonicalOrigin` newtype turns a comment-only, bypassable invariant into a compiler-enforced one and deletes a migration pass.
2. **F-01 — `AppState`/`HeadlessState` is a nullable state bag** hand-assembled at every entrypoint with `..Default::default()`; every new shared server dependency is a multi-site parallel edit with a silent-`None` trap.
3. **F-03 — the 203-line Tauri `.setup` god-closure** — the desktop wiring spine for tray, callbacks, HTTPS/HTTP startup, error policy, and updater polling, all inline.

**Cheapest high-value wins (land these first — hours, not days):** **F-05** (fix the SDK doc-contract drift + add an export-sync test — the README currently teaches the *obsolete* status shape the Q12 refactor existed to kill) and **F-07** (`CertPaths` parameter object).

---

## Methodology

Map-reduce with parallel Claude + Codex agents and a coordinator-of-specialists reduce, per the `/harden` protocol. **Deviations from the literal ultra spec are documented honestly below — they were deliberate, to preserve ultra's methodological depth (iterated cross-model adversarial passes + verify-all) while staying tractable:**

- **Phase 1 (map):** 2 Opus mappers (accelerator-Rust, SDK) instead of the hierarchical outer+per-package fan-out — the scope is 2 packages I already knew structurally; substituted `general-purpose` for `Explore` because Explore is read-only in this harness and the mappers had to write their map files. Maps cross-checked against the actual `wc -l` recon.
- **Phase 2 (finders):** **1 Claude Opus + 1 Codex xhigh per cluster** (the cross-model pair) rather than ultra's literal 2 Claude + 2 Codex. Rationale (per the skill's own guidance): *two same-family finders add mostly redundancy; cross-model disagreement is the signal.* Finders were given the cluster file list + a dependency summary but **not** the map's pre-found smells, so cross-model + map convergence is real signal, not anchoring. 6 clusters: C1 core-server, C2 core-bb-versions, C3 core-config-auth, C4 tauri-app-lifecycle, C5 tauri-certs-crash-sites, C6 sdk-prover (weighted).
- **Phase 2.5 (cross-rebuttal):** consolidated the per-cluster rebuttal into **two cross-family passes** (Opus attacks all Codex findings; Codex attacks all Claude findings). This is better at catching *cross-cluster* patterns (it's how the `AppState` C1≡C4 dup and the headless-origin-bypass gap were found) at lower cost.
- **Phase 2.6 (Round-2 push-back, ultra-only):** one targeted Codex self-critique (resumed session) instead of a generic dual push-back — pointed at the highest-value question (Codex had marked 10 findings "Claude-missed," a count that risked padding). It honestly pruned those to keep 2 / fold 4 / drop 4. The one genuine cross-model disagreement (`CrashRecovery` trait) was resolved by orchestrator ground-truth grep before the push-back.
- **Phase 3 (reduce):** fresh Codex xhigh coordinator (cross-family judgment at the reduce stage, per ultra) → `findings/consolidated.md`.
- **Phase 4 (verifier):** the Opus orchestrator independently re-read the cited source for every finding before trusting it (`findings/verified.md`). This **corrected two coordinator inflations**: F-04's LOC (the module is ~582 prod LOC, not 1209 — the rest is inline tests) and F-01's instance list (5 cited `server.rs` sites are test helpers, not prod).
- **Phase 5 (report):** this file. **No HTML companion** — the skill scopes the HTML companion to security focus (its per-finding ELI5 phrasing is attacker-centric); a quality report needs "what gets harder to change" framing, deferred per the skill.
- **Context discipline:** finders capped at their cluster; cross-boundary checks (core↔server-binary state construction, the two `main.rs` files) explicitly allowed as handoff edges. Negative list applied throughout (test-pinned wire contracts, consistently-applied conventions, and intentional documented designs were held as non-findings).

**Cross-model agreement was high:** of 17 Codex finder findings, the Claude rebuttal rated 11 CONVERGE / 0 REFUTED; of 23 Claude finder findings, Codex rated 0 REFUTED. Convergence — not volume — is the confidence signal here.

---

## Findings

Sorted by priority (maintenance impact × blast radius × change frequency). IDs are stable from `consolidated.md`.

### [HIGH] F-02 — Canonical origin is modeled as raw strings, with a live ingress bypass
- **Smell:** Primitive Obsession (→ Replace Primitive with Object)
- **Impact:** structural, wide blast radius — spans `core` auth, config persistence, and headless ingress; touched on every `/prove` authorization check and every config load/save.
- **Scope note:** the bypass is in the headless `server` binary (a shipped release tarball, but documented for CI/e2e); the desktop app loads origins via the canonicalizing `config::load`, so this is a type-invariant/foot-gun gap, **not** an attacker-exploitable hole in the GUI product. The value is making the invariant un-bypassable for any *future* ingress.
- **Found by:** both (rebuttals explicitly adjudicated C3-finder over C5-finder; the headless bypass was a gap *neither* finder caught, surfaced by the Codex rebuttal and verified in source).
- **Confidence:** high.
- **Instances:** `core/src/authorization.rs:21-58, 77-79, 101-145` · `core/src/config.rs:41-55, 83-130` · `core/src/server/auth.rs:35-52, 111-116` · **`server/src/main.rs:43-57` (live bypass — raw `ALLOWED_ORIGINS` env strings written straight into `approved_origins` with no `url::Url` canonicalization)**.
- **Why it harms future change:** canonical origins are stored, compared, persisted, and queued as plain `String`s; correctness depends on every caller canonicalizing first and on `config::load()` migrating persisted strings. A new ingress (the headless binary already is one) can feed non-canonical origins into exact-match approval and compile cleanly. Reviewers must re-prove "is this string canonical?" at every call site.
- **Fix:** introduce `CanonicalOrigin(String)` with `#[serde(try_from = "String")]` whose constructor runs the canonicalization; store `Vec<CanonicalOrigin>` and thread the newtype through auth/config/ingress. **What disappears:** the comment-only invariants, the `migrate_approved_origins` repair pass, and the headless env bypass.
- **Effort:** days.

### [HIGH] F-01 — Server runtime state is a nullable bag every entrypoint hand-assembles
- **Smell:** Data Clumps + nullable Special-Case bag (→ Extract Factory / Replace Optional-field with required)
- **Impact:** structural→architectural, wide — spans `core`, the headless `server` binary, the Tauri binary, and the test suite; every new shared server dependency lands here.
- **Found by:** both (the dominant cross-cluster finding — C1 + C4 + the map all hit it; rebuttals merged them to one root).
- **Confidence:** high.
- **Instances (prod):** struct def `core/src/server.rs:83-119` · `server/src/main.rs:62-75` · `src-tauri/src/main.rs:345-367`. **(corroborating, tests):** ~5 hand-assembly sites in `core/src/server.rs` test module (>L210) — evidence that the missing constructor also taxes tests, *not* separate prod debt.
- **Why it harms future change:** `HeadlessState` exposes mostly-`Option` fields, `AppState` adds 3 optional callbacks, and each launcher/test assembles a different subset with `..Default::default()`. Adding one required proving dependency means parallel edits across both binaries and the tests; a missed site silently gets `None` and fails at runtime, not compile time.
- **Fix:** core-owned `HeadlessState::headless(...)` / `AppState::desktop(...)` constructors; make always-required deps (e.g. `prove_semaphore`, `app_version`) non-`Option`; isolate the GUI callbacks into a small desktop-extras type. **What disappears:** the repeated `Some(Arc::new(Semaphore::new(1)))` / `env!("CARGO_PKG_VERSION")` literals and the `..Default::default()` omission trap.
- **Effort:** days.

### [HIGH] F-05 — The SDK public contract is hand-copied across source, barrel, and docs (and has drifted)
- **Smell:** Duplicate Code + Divergent Change
- **Impact:** structural, public-facing — spans the published source, the barrel, README, MIGRATION.md, and the bundled skill doc; every public API change fans out across all five.
- **Found by:** both. **Cheapest high-value finding.**
- **Confidence:** high (core drift); the "skill omits `denied`" sub-item is about the separate `AcceleratorPhase` type — minor, lower-confidence.
- **Instances:** `sdk/src/lib/accelerator-prover.ts:45-92, 219-222` · `sdk/src/index.ts:1-8` · `sdk/README.md:88-101` · `sdk/MIGRATION.md:3-43, 83` · `sdk/.claude/skills/aztec-accelerator/SKILL.md`.
- **Why it harms future change:** the contract is restated by hand in 5 places and they've **already** diverged: README still documents the **obsolete flat `interface AcceleratorStatus`** (the exact pre-Q12 shape that lets illegal field combos typecheck — its own MIGRATION.md labels it "dead"); MIGRATION says `AcceleratorProtocol` is exported but the barrel omits it (the documented import fails); README's method table omits `setForceLocal` (which the playground actually consumes). One API edit needs synchronized prose+barrel+example updates, and missing one hands consumers contradictory guidance.
- **Fix:** make the barrel the single canonical surface; fix the current drift now; add an export/doc-sync test or a generated API snippet so docs can't silently diverge. **What disappears:** the manual contract duplication and the contradictory docs.
- **Effort:** hours.

### [HIGH] F-03 — Tauri startup is a 203-line god-closure
- **Smell:** Long Method + Divergent Change
- **Impact:** structural — one file, but it's the desktop wiring spine (tray, crash recovery, callbacks, HTTPS/HTTP startup, diagnostics, updater polling).
- **Found by:** both.
- **Confidence:** high.
- **Instances:** `src-tauri/src/main.rs:260-462` — esp. `266-305`, `316-367`, `369-379`, `396-441` (inline `AddrInUse`/redundant-instance policy), `448-459`.
- **Why it harms future change:** one `.setup` closure performs most bootstrap inline, with nested callback creation and ~8 `*_clone`/`*_for_*` rebindings of `tray`/`status`/`config_state`/`auth_manager`/`AppHandle` across unrelated concerns. Adding or reordering a startup step means editing a closure that captures everything.
- **Fix:** Extract Function into named bootstrap phases (`build_tray`, `wire_callbacks`, `spawn_servers`, `spawn_update_poller`) + small callback builders, leaving `.setup` as orchestration only. **What disappears:** most clone-stutter and the mixed-concern scope.
- **Effort:** days.

### [MED-HIGH] F-04 — `versions.rs` is a multi-responsibility module
- **Smell:** Large Class (→ Extract Module)
- **Impact:** structural — one `core` module with multiple direct consumers; every version/platform/cache/download/eviction change converges here.
- **Found by:** codex (Claude rebuttal upheld it; folded the smaller `download_bb` macOS-tail and platform-`cfg`-ladder items into it).
- **Confidence:** med-high. **LOC corrected by verifier: ~582 prod LOC (not the 1209 total — the rest is inline tests).**
- **Instances:** `core/src/versions.rs` — 18 prod functions spanning HTTP-client policy, version parse/classify, platform naming, URL construction, sort/eviction, hashing, digest fetch, validation, download orchestration, tarball extraction, install, and cleanup (notably the macOS `xattr`+`codesign` finalize tail bolted into `download_bb:342-420`).
- **Why it harms future change:** platform tweaks, retention changes, release-layout changes, and macOS finalization all reopen the same module and force retesting unrelated paths.
- **Fix:** Extract Module into ~`version_id`, `platform`, `artifact_layout`, `cache`, `downloader/install`. **What disappears:** the single-file collision point; platform-finalization stops piggybacking on cache/download orchestration.
- **Effort:** days.

### [MED] F-07 — Certificate artifact paths are an undeclared parameter object
- **Smell:** Data Clumps (→ Introduce Parameter Object)
- **Impact:** structural, concentrated in the Safari/TLS cert lifecycle; moderate change frequency on rotation/generation/load.
- **Found by:** both. **Cheap win.**
- **Confidence:** med.
- **Instances:** `src-tauri/src/certs.rs:20-33, 90-128, 201-205, 261-286` — the `{ca.pem, localhost.pem, localhost.key}` triple travels together everywhere, all as `&Path` (so an arg-swap is a compiler-invisible no-op), and `rotate()` re-declares the same triple as staged `*.new` files.
- **Why it harms future change:** renaming a file or adding a 4th TLS artifact requires synchronized edits across existence checks, writers, loaders, staging cleanup, and promotion.
- **Fix:** `CertPaths { ca_cert, leaf_cert, leaf_key }` with `live()`/`staged(dir)` constructors + `exists()` and `swap_into()`. **What disappears:** the positional 3-path calls, the hard-coded basenames duplicated in path accessors, and (folded in) the `rotate()` ordering coupling.
- **Effort:** hours.

### [MED] F-06 — `AcceleratorProver` transport + probe state split across two HTTP clients
- **Smell:** Divergent Change + Temporal Coupling
- **Impact:** structural, localized to one SDK file but governs every health probe and proof request.
- **Found by:** both (Round-2 folded the `#probeAndParseHealth` long-method and endpoint/cache-coupling concerns into this seam).
- **Confidence:** med.
- **Instances:** `sdk/src/lib/accelerator-prover.ts:163-168, 204-229, 235-374, 414-423` — `/health` uses native `fetch` + `Promise.any` + manual retry; `/prove` uses `ky`; negotiated `#acceleratorProtocol` and `#statusCache` are mutated in several places.
- **Why it harms future change:** adding headers, auth, proxying, path prefixes, or richer retry/error rules means touching two HTTP stacks with divergent timeout/error models and several cache/protocol invalidation points.
- **Fix:** Extract Class `AcceleratorTransport` owning URL construction, protocol negotiation, cached status, and normalized transport errors. **What disappears:** the duplicated URL/client policy; `#probeAndParseHealth` shrinks substantially.
- **Effort:** days.

### [MED] F-08 — `/prove` status ownership is split between `prove` and `resolve_version`
- **Smell:** Temporal Coupling
- **Impact:** local, but on a hot `core` path; every new proof phase must preserve a hidden contract.
- **Found by:** claude (Codex rebuttal + Round-2 both kept it standalone).
- **Confidence:** med.
- **Instances:** `core/src/server/prove.rs:64-96, 160-165` (characterization at `core/src/server.rs:617-685`).
- **Why it harms future change:** `prove()` sets `Proving`; `resolve_version()` may overwrite it with `Downloading`, then must restore `Proving` before returning so the caller's state machine still works. Adding a phase or moving the download step forces preserving a cross-function ordering rule neither signature encodes.
- **Fix:** let `prove()` own the full status sequence (it already installs a `StatusGuard`) and make `resolve_version()` return data only. **What disappears:** the restore-to-`Proving` side effect in `resolve_version`.
- **Effort:** hours.

### [LOW] F-09 — HTTPS startup duplicated across launch-time and settings-time entrypoints
- **Smell:** Duplicate Code (→ Extract Function)
- **Impact:** structural, two Tauri entrypoints; changes whenever HTTPS startup/reporting/shutdown behavior changes.
- **Found by:** claude (Round-2 kept it standalone).
- **Confidence:** low (small surface).
- **Instances:** `src-tauri/src/main.rs:72+86` and `src-tauri/src/commands.rs:156+161` — both `load_rustls_config()` then `spawn(start_https(state, tls))` with the same error logging; only the preamble differs.
- **Why it harms future change:** if HTTPS startup gains handle tracking, success reporting, or shutdown hooks, one path can drift while the other keeps old behavior.
- **Fix:** Extract Function into a shared GUI-side `spawn_https(state, tls)` (optionally `load_tls_or_log()`), leaving each entrypoint only its genuinely different preconditions.
- **Effort:** hours.

---

## Findings NOT pursued (with reasoning)
- **`compute_threads` Feature Envy** (`core/src/server/prove.rs:104-113`) — single-use config→`Option<usize>` adapter calling existing `Speed::is_full/to_threads`; not duplicated domain logic. (Codex rebuttal + verifier agree.)
- **`CrashRecovery` trait Speculative Generality** (`src-tauri/src/crash_recovery.rs:16-43`) — real (1 impl, 0 polymorphic uses, no mock) but local/minor; a ~10-line one-impl trait. Demoted to this one-line note rather than a finding.
- **SDK `catch ⇒ offline`** (`sdk/src/lib/accelerator-prover.ts:370`) — *false alarm*: non-OK and bad-JSON are already split to `reason:"error"` at `:291/:305`; the outer catch is the legitimate "both probes failed" bucket the type documents.
- **`rotate()` temporal coupling** — folded into F-07 (the path clump is the root).
- **xattr/codesign subprocess skeleton, `security` Keychain CLI scaffolding, auth-popup label/close split, duplicate doc-markers, boxed-error `AddrInUse` downcast** — real but below the final-report bar (same-file helper extractions / comment hygiene).
- Duplicate C1/C4 `AppState` reports — deduped into F-01.

## Cross-cutting observations
1. **One theme dominates the high-priority debt: invariants enforced by convention instead of by types/constructors.** F-02 (origin canonicality via comments), F-01 (required state via `Option` + `..Default::default()`), and F-05 (the public contract via hand-copied docs) are the same failure in three materials. The highest-leverage direction is uniform: *push each invariant into a type or a single constructor* (newtype / factory / canonical barrel + sync test). Doing all three would retire the bulk of this report.
2. **The `core`-extraction left parallel wiring.** F-01 and F-09 are both "the same assembly/startup spelled twice across the core↔src-tauri↔server boundary." The extraction was clean structurally (callback slots, no upward calls), but shared *construction* wants core-owned constructors the binaries call.
3. **The codebase is genuinely well-maintained.** Finder passes repeatedly hit already-remediated targets (typed `AztecVersion`, extracted download/verify/install helpers, the discriminated `AcceleratorStatus` union, `mutate_config`/`WindowConfig` dedup helpers) and correctly logged them as non-findings. The low finding density and zero refuted-on-verification rate reflect a healthy post-refactor baseline — this report is consolidation polish, not rescue work.

---
*Artifacts: `findings/consolidated.md` (Phase 3 reduce) · `findings/verified.md` (Phase 4 verification + corrections) · `raw/` (12 finder outputs + 2 rebuttals + Round-2 + maps).*
