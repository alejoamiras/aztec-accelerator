# Harden-Quality Report

**Repo:** aztec-accelerator
**Date:** 2026-06-10
**Effort:** max
**Run ID:** 2026-06-10-max-q7e3
**Models:** Phase 1 map — Fable Explore ×3 (+2 harness-duplicated, folded in) · Phase 2 finders — Fable ×6 (per cluster) + Codex xhigh ×3 (per crate) · Phase 3 reduce + Phase 4 verify — Fable orchestrator (see Methodology deviation) · cross-model convergence used as the primary confidence signal
**Scope:** `packages/accelerator` Rust crates (`core`, `src-tauri`, `server`) + `packages/sdk` (TypeScript public API). **Excluded:** `packages/playground`, `packages/landing`, generated/`target`/`node_modules`/`gen`, `implementations-plan/**`, prior `audit/**`, webview UI assets, test-only e2e/scripts dirs. Same surface as the 2026-06-08 ultra run — a fresh pass.

## Executive summary

Quality-only audit (named code smells, change-cost; zero security/correctness scoring). The codebase is in good shape — it has been through a mega-deep quality refactor (2026-06-05), a core-extraction (#325), and a security-hardening pass (the last two weeks), and the agents repeatedly **credited prior paydown rather than inventing work**: the big "cross-crate server/AppState/gating duplication" hypothesis came back *largely already resolved* by the F-01 core extraction, and several size-based leads were correctly refuted (`server.rs` 1424 LOC and `versions/mod.rs` 1006 LOC are both ~60% colocated tests, not Large Classes).

The headline theme is **invariants enforced by comment-and-discipline instead of by a type or structure**. Five of the top findings are the same shape: a multi-step ordering or a two-place state that "must stay in sync," held together only by a `// must…` comment. This is not academic — the strongest finding (F-01, the Safari-HTTPS bring-up sequence) **already failed exactly this way**: the SEC-08 "codex-M1" bug fixed earlier today was the Settings-toggle path drifting out of sync with the launch path because the ordering lived in caller discipline, not a shared seam.

Recommended order of attack: land the four **hours-each, multi-model-confirmed structural** wins first — **F-01** (extract the HTTPS bring-up sequence; it has a proven failure mode), **F-02** (move SDK public types out of the hotspot file), **F-03** (`ProveError` type), **F-04** (extract the `.setup()` blob). Then the SDK trio (F-05/F-06/F-11, all in the same 440-LOC hot file, best done together), then the warm core structural items (F-07/F-08/F-09).

## Methodology

6 clusters by package boundary + similarity: `core-server`, `core-versions`, `core-auth-config`, `tauri-app` (incl. headless `server/main.rs` for cross-crate dup), `tauri-platform`, `sdk`. Per cluster: 1 Fable finder (cluster-scoped) + crate-level Codex xhigh coverage (3 sessions: core / src-tauri+server / sdk), so every cluster got cross-model eyes. The verbatim quality prompt + DO-NOT-FLAG list were used; inter-procedural tracing capped ~4 functions.

**Deviations from the `max` spec (stated honestly):**
- **Codex ran crate-level (3 sessions), not per-cluster (6).** Every file still got Codex coverage; grouping was by crate boundary to halve session cost on a surface that was ultra-audited 2 days ago. Convergence was near-total, so the grouping cost nothing in signal.
- **Phase 2.5 cross-rebuttal was folded into Phase 3.** Rather than 9 extra rebuttal agents, the orchestrator reconciled the two model families directly (the rebuttal's purpose — surfacing agree/disagree — is captured per-finding under "Found by" + the cross-model-disagreement notes).
- **Phase 3 reduce + Phase 4 verify run by the Fable orchestrator, not a separate Codex coordinator.** The 3 Codex passes already supplied per-crate cross-family reduction; convergence made dedup mechanical. The orchestrator independently re-read source for the top findings (F-01, F-03, F-13 all re-read against the actual lines before accepting the claim — anchoring guard honored).
- **Density:** 15 findings / 6 clusters = 2.5 per cluster, above the ~1.2 target. This reflects a freshly-churned surface at max effort; 6 of 15 are Low. No filter failure — the DO-NOT-FLAG list held (the agents refuted ~9 leads outright).

## Findings (sorted by computed priority = scope × blast radius × change frequency)

### [STRUCTURAL ★] F-01: Safari-HTTPS bring-up sequence is comment-enforced, replicated across launch + settings paths
**Impact:** structural · blast radius: 3 files (`main.rs`, `commands.rs`, `certs.rs`) · change frequency: **hot** (main.rs/commands.rs are the #1/#3 hottest files)
**Confidence:** high · **Smell:** Temporal Coupling + Duplicate Code · **Found by:** both (tauri-app Fable + tauri-platform Fable + tauri Codex — triple-sourced)
**Instances:** `src-tauri/src/main.rs:55-94` (`try_start_https`), `main.rs:424-428` (startup gate), `src-tauri/src/commands.rs:151-180` (`enable_safari_support`), stepwise API at `certs.rs:170-182,257-345,427-433`
**Evidence:** the cert bring-up ordering `migrate_legacy_ca_key → (generate_and_save) → verify-trust → load_rustls_config → spawn_https` is hand-replicated in two orchestrators (launch vs Settings-toggle), with the order enforced only by comments.
**Why it harms future change:** any change to the sequence (a new cert step, a reordering, a new precondition) must be mirrored in every caller, and a missed mirror compiles fine. **This already happened:** the SEC-08 codex-M1 bug (fixed 2026-06-10) was precisely the Settings path missing the `migrate` gate the launch path had.
**Smallest safe refactoring:** Form Template Method / Extract Function — `certs::prepare_https(mode) -> Result<TlsConfig>` (or `HttpsBootstrap::{prepare_at_launch, enable_from_settings}`) owning the ordering; callers invoke one function.
**What disappears:** the replicated sequencing knowledge and the whole class of "one path forgot a step" bugs.
**Effort:** hours.

### [STRUCTURAL ★] F-02: SDK public types live inside the 14-commit hotspot implementation file
**Impact:** structural · blast radius: medium-wide (every consumer + transport) · change frequency: **hot**
**Confidence:** high · **Smell:** Misplaced-Shared-Types + latent Cyclic Dependency · **Found by:** both (sdk Fable + sdk Codex)
**Instances:** `sdk/src/lib/accelerator-prover.ts:11-92` (all 6 published types), `accelerator-transport.ts:3` (`import type` back-import), `index.ts:1-9`
**Evidence:** `AcceleratorTransport` depends on `AcceleratorProtocol`/`AcceleratorStatus`, which are owned by the prover *implementation* file → a 2-way edge that is runtime-acyclic only because the reverse leg is `import type`.
**Why it harms future change:** transport-only edits and public-API/contract edits churn the same 440-LOC file, mixing npm-contract diffs into implementation diffs and making the published-type history hard to read.
**Smallest safe refactoring:** Move shared types to `lib/types.ts`; re-export unchanged from `index.ts`.
**What disappears:** the back-import, the latent cycle, and contract-vs-impl churn collision.
**Effort:** hours.

### [STRUCTURAL] F-03: Stringly error channel — `(StatusCode, String)` + `json_error` ×11, with a divergent third shape
**Impact:** structural · blast radius: `server.rs` + `server/{prove,auth,host}.rs` · change frequency: **hot**
**Confidence:** high (re-read: `json_error` def `server.rs:326`; `host.rs:69-72` confirmed to use a different `axum::Json(json!{…})` shape with no `message`) · **Smell:** Duplicate Code / Data Clump (Primitive-Obsession-on-errors) · **Found by:** both (core-server Fable + core Codex)
**Instances:** `server.rs:326` (def); 11 call sites — `server/prove.rs:63,122,134,182,222`, `server/auth.rs:41,67,79,99,105,132`; divergent shape `server/host.rs:69-72`
**Evidence:** every error path hand-builds a `(StatusCode, json_error(code, msg))` tuple; `host.rs` builds a structurally different JSON body. The handler knows serialization details at every site.
**Why it harms future change:** adding a field (e.g. `request_id`), standardizing status↔body mapping, or changing the media type requires editing every branch and keeping shape aligned by hand.
**Smallest safe refactoring:** Replace Error Tuple with Type — a plain `enum ProveError` implementing `IntoResponse` (preserves the pinned `text/plain`/JSON contract; **no new dependency** — respects the deliberate no-`thiserror`/`anyhow` house convention).
**What disappears:** repeated tuple/`json_error` construction and per-site serialization knowledge; the host.rs divergence folds in.
**Effort:** hours–1 day.

### [STRUCTURAL] F-04: `.setup()` is a 154-line Long Method with 9 concerns and zero test coverage
**Impact:** structural · blast radius: 1 file but the wiring hub · change frequency: **hot**
**Confidence:** high · **Smell:** Long Method + Divergent Change · **Found by:** both (tauri-app Fable + tauri Codex)
**Instances:** `src-tauri/src/main.rs:314-466` (clone/callback wiring concentrated at `362-417`)
**Evidence:** one closure mixes tray construction, crash-recovery policy, callback assembly, HTTPS bootstrap, diagnostics, HTTP start, and update polling, with ~10 `X_for_Y` clone bindings; only integration-testable through Tauri.
**Why it harms future change:** any startup tweak reopens the whole blob; the clone-stutter obscures what each task captures; nothing is unit-testable.
**Smallest safe refactoring:** Extract Class `DesktopBootstrap` with `init_tray` / `build_state` / `bootstrap_https` / `start_background_tasks`.
**What disappears:** the 150-line inline closure and the clone-stutter; each phase becomes nameable + testable.
**Effort:** hours.

### [STRUCTURAL] F-05: SDK `#probeAndParseHealth` Long Method hiding a missing status factory
**Impact:** structural · blast radius: the SDK's core decision path · change frequency: **hot**
**Confidence:** high · **Smell:** Long Method + Duplicate Code / missing Factory · **Found by:** both (sdk Fable + sdk Codex)
**Instances:** `accelerator-prover.ts:233-328`; manual `AcceleratorStatus` construction ×6 at `248,269,291,308,317,326`
**Evidence:** one ~96-line method probes, parses JSON, distinguishes legacy vs multi-version protocol, applies version policy, logs, mutates protocol state, caches, and hand-builds six union variants.
**Why it harms future change:** adding one union field or status variant forces branch-by-branch edits; an omission type-checks if the field is optional.
**Smallest safe refactoring:** Extract Method (parse / version-policy steps) + Introduce Factory for `AcceleratorStatus` variants.
**What disappears:** repeated object literals and branch-local knowledge of the public union shape.
**Effort:** hours.

### [STRUCTURAL] F-06: SDK protocol pinning relies on scattered branch-order side effects
**Impact:** structural · blast radius: medium (probe + every `/prove`) · change frequency: **hot**
**Confidence:** high · **Smell:** Temporal Coupling · **Found by:** both (sdk Fable + sdk Codex)
**Instances:** pin `accelerator-prover.ts:256`; clear `268`, `325`; non-pin error branch `245-253`; consumed in `accelerator-transport.ts:57-63,121-133` via `baseUrl`
**Evidence:** the "don't pin on error / clear on malformed JSON / clear on probe failure" rule is spread across exit paths; `/prove` later reads that hidden state indirectly through `baseUrl`.
**Why it harms future change:** any new health branch must remember the pin/clear policy or silently bake in stale negotiation state.
**Smallest safe refactoring:** Move Method — let `AcceleratorTransport` own the protocol-state transition behind one API (`commitStatus(status)` deriving the pin from the discriminant).
**What disappears:** scattered `setProtocol(...)` calls and the comment-driven invariant.
**Effort:** hours.

### [ARCHITECTURAL] F-07: `versions/mod.rs` is a policy + cache + release-metadata hub — Divergent Change
**Impact:** architectural · blast radius: `versions/mod.rs` + `downloader.rs` + callers `bb.rs`, `server/prove.rs` · change frequency: warm (cold post-#325, but central)
**Confidence:** high · **Smell:** Divergent Change / Large Module (NOT Large Class — both models: ~62% is tests) · **Found by:** both (core-versions Fable + core Codex)
**Instances:** `versions/mod.rs:11-18,23-68,80-156,165-197,218-274,297-377`; `downloader.rs:8-58,116-212` (9-item `use super` back-import)
**Evidence:** version identity, network-tier retention policy, GitHub release metadata, and cache-dir layout all live behind one module boundary; `downloader` reaches broadly into `super::{…}` for unrelated concerns.
**Why it harms future change:** a retention tweak, a cache-layout change, or a GitHub-API change all collide on the same module; reviewers reload the whole subsystem.
**Smallest safe refactoring:** Extract Module → `version_policy` / `cache_layout` / `release_metadata`, with `downloader` depending on explicit helpers. (Tier retention is already a clean table at `NetworkTier::retention_limit` — leave it.)
**What disappears:** the broad `use super` coupling and concern-mixing.
**Effort:** ~1 day.

### [STRUCTURAL] F-08: Parse-don't-validate newtypes abandoned past their first edge — Primitive Obsession
**Impact:** structural · blast radius: wide (prove/version path; auth path) · change frequency: hot on the prove path
**Confidence:** high · **Smell:** Primitive Obsession (partial Replace-Primitive-with-Object) · **Found by:** both (core-versions + core-auth-config Fable + core Codex)
**Instances:**
- `AztecVersion`: `server.rs:37,87`, `server/prove.rs:38-40,74-89,140-159`, `bb.rs:18,27-32,75-81`, `versions/mod.rs:154-156,191-197,356-371`, `downloader.rs:21-22,31,37,54,116,156`; `ResolvedVersion` (prove.rs:38-41) carries the value twice; `"unknown"` magic sentinel collapses `Option`.
- `CanonicalOrigin`: enforced only at `is_approved`; raw `&str` in `request()`, `by_origin`, popup callback, `remove_approved_origin`; `authorization.rs:249-253` `is_auto_approved` still carries the comment-only "already canonical" contract the newtype was built to kill.
**Why it harms future change:** every `&str` seam must be re-audited whenever version/origin handling grows (display tag vs cache key vs release tag; stricter origin rules) — the newtype's guarantees stop at the boundary.
**Smallest safe refactoring:** Change Function Signature to thread `&AztecVersion` / `&CanonicalOrigin` through `find_bb`, `bb::prove`, `version_bb_path`, `download_url`, `cleanup_old_versions` (and the auth request/lookup path).
**What disappears:** repeated raw↔typed conversions, string comparisons, caller-managed invariants, and the "already canonical" comments.
**Effort:** ~1 day per newtype.

### [STRUCTURAL] F-09: `AuthorizationManager` keeps two indexes in sync by discipline — Temporal Coupling
**Impact:** structural · blast radius: `authorization.rs` + `server/auth.rs` · change frequency: warm
**Confidence:** high · **Smell:** Temporal Coupling · **Found by:** both (core-auth-config Fable + core Codex)
**Instances:** `authorization.rs:172-176,213-217,224-231,240-241` (defensive desync `if let` at `214`)
**Evidence:** `by_origin` and `by_request` must be mutated in lockstep by every mutator; one missed path leaves stale piggybacking or unresolvable requests.
**Why it harms future change:** adding cancellation, expiry, or alternate lookup means touching every mutator in lockstep.
**Smallest safe refactoring:** Extract Class `PendingAuthorizations` with single-purpose mutators — OR delete `by_origin` and linear-scan (the map holds ≤10 entries; the secondary index buys little).
**What disappears:** the manual two-map sync logic and the defensive desync guard.
**Effort:** hours.

### [STRUCTURAL] F-10: updater ↔ crash_recovery disarm/rearm is a manual paired-call protocol (Windows)
**Impact:** structural · blast radius: updater + recovery + quit paths · change frequency: warm (Windows-only)
**Confidence:** high · **Smell:** Temporal Coupling + Inappropriate Intimacy · **Found by:** both (tauri-platform Fable + tauri Codex)
**Instances:** `updater.rs:146-189` (disarm→install→rearm; 3 rearm sites at ~159/169/177) ↔ `crash_recovery.rs:12-18,26-29,322-349`, quit at `main.rs:338-347`
**Evidence:** `updater.rs` must know the exact suspend/rearm contract and rearm on *every* early-return path, guarded only by the comment "every path that leaves the app running must end armed."
**Why it harms future change:** a new early-return in the update flow silently skips rearm; the policy leaks across two modules.
**Smallest safe refactoring:** Introduce Guard Object — RAII `CrashRecoverySuspendGuard` / `suspend_for_update(app, |…| { install })` that rearms on drop, with an explicit `defuse()` on the restart path.
**What disappears:** the 3 hand-placed rearm calls and the comment-enforced contract.
**Effort:** hours.

### [STRUCTURAL] F-11: SDK `createChonkProof` mixes six reasons-to-change — Divergent Change
**Impact:** structural · blast radius: the SDK's main orchestration path · change frequency: hot
**Confidence:** high · **Smell:** Divergent Change + Long Method · **Found by:** both (sdk Fable + sdk Codex)
**Instances:** `accelerator-prover.ts:330-398` — phase emission `340,350,357,362-363,381,393,396`; fallback routing `343-346,369-383`; response/header handling `387-398`
**Evidence:** UI phase policy, availability fallback, 403-denial semantics, serialization, prove-duration extraction, and proof decoding all change the same method for different reasons.
**Smallest safe refactoring:** Extract Method into `#proveRemote` / `#fallbackUnavailable` / `#decodeWithDuration`.
**What disappears:** mixed concerns in the orchestration path. **Effort:** hours. (Best landed with F-05/F-06 — same file.)

### [STRUCTURAL] F-12: ~715+ LOC of prove/auth/host tests stranded in `server.rs` (Q2-extraction residue)
**Impact:** structural · blast radius: `server.rs` navigability · change frequency: hot (the file is edited often; the tests aren't where the code is)
**Confidence:** high · **Smell:** Shotgun Surgery residue / misplaced tests · **Found by:** core-server Fable (Codex noted the colocation but didn't score it — minor cross-model disagreement on whether it's a *smell*)
**Instances:** production code `server.rs:1-328`; tests `server.rs:330-1424` exercise `prove.rs`/`auth.rs`/`host.rs`, which have ~zero inline tests
**Evidence:** the Q2 extraction moved code to submodules but left their characterization tests in `server.rs`.
**Why it harms future change:** editing `prove.rs` means hunting its tests two files away; `server.rs` reads as a 1424-LOC monolith when it's really ~328 LOC of code.
**Smallest safe refactoring:** Move Function — relocate each test module to its owning file. Drops `server.rs` to ~700 LOC. Pure test move, zero production risk.
**What disappears:** the apparent-monolith and the code↔test distance. **Effort:** hours.

### [LOCAL] F-13: lock-mutate-save replicated with three *diverged* save-failure policies
**Impact:** local · blast radius: 3 sites across 2 crates · change frequency: warm
**Confidence:** high (re-read all three) · **Smell:** Duplicate Code · **Found by:** core-auth-config Fable + Rust map (**cross-model disagreement:** tauri Codex called `mutate_config` "already resolved, 6 sites" — true for the command sites, but it missed the two hand-rolled copies; resolved in favor of the finding)
**Instances:** `commands.rs:12-19` `mutate_config` (propagates the save error) · `core/server/auth.rs:117-126` (warns) · `src-tauri/main.rs:98-104` `reset_safari_support` (swallows via `let _ =`)
**Evidence:** the lock→mutate→`config::save` shape appears three times with three different error policies; `mutate_config`'s doc claims "single source of truth," but core `auth.rs` can't import it (it lives in `src-tauri`).
**Why it harms future change:** a dev reading one site can't assume the others behave the same on save failure; adding fsync or retry to persistence means finding all three.
**Smallest safe refactoring:** move a `lock_mutate_save(cfg_lock, f) -> Result` helper to **`core/config.rs`** so both crates share it + one error policy.
**What disappears:** two hand-rolled copies and the policy divergence. **Effort:** hours.

### [LOCAL] F-14: Duplicated loopback-literal sets across three matchers (drift risk)
**Impact:** local · blast radius: 3 functions · change frequency: warm
**Confidence:** moderate · **Smell:** Duplicate Code · **Found by:** Rust-crates map (single-source; the per-cluster finders didn't span all three)
**Instances:** `server/host.rs` `host_is_trusted`, `authorization.rs` `is_auto_approved`, `authorization.rs` `canonicalize_origin` — each carries its own `{127.0.0.1, localhost, ::1}` handling (the `::1` bracket treatment differs between them).
**Why it harms future change:** adding a loopback form (or changing IPv6 bracket handling) must be done in three places, kept consistent by hand.
**Smallest safe refactoring:** Extract a single `is_loopback_host(&str) -> bool` in `core` and call it from all three.
**What disappears:** the triplicated literal sets. **Effort:** hours.
**[Out-of-scope note:** whether the `::1` bracket handling *semantically* diverges between the host guard and the auth check is a correctness question — not scored here; route to `/harden bugs` if you want it checked.]

### [LOCAL] F-15: `config.rs` load/save hardcode the path — Global Data + untested atomic write
**Impact:** local · blast radius: `config.rs` + its test · change frequency: warm
**Confidence:** moderate · **Smell:** Global Data (hidden global path) · **Found by:** core-auth-config Fable
**Instances:** `config.rs` `load()`/`save()` hardcode `config_path()`; the roundtrip test re-implements `save`; the atomic write-tmp-rename block has no direct coverage.
**Why it harms future change:** can't unit-test load/save against a temp path without env gymnastics; the test duplicates production logic so it can't catch a save regression.
**Smallest safe refactoring:** Parameterize — `load_from(path)`/`save_to(path)` with the hardcoded path as a thin wrapper; point the test at a tempdir.
**What disappears:** the test's re-implementation of `save`; the atomic block becomes testable. **Effort:** hours.

## Findings NOT pursued (refuted leads — with reasoning)
- **`server.rs` is a 1424-LOC Large Class** — refuted by both models: production is `:1-328`; the rest is colocated tests (→ F-12 instead).
- **`versions/mod.rs` is a 1006-LOC Large Class** — refuted: ~62% tests (→ F-07 Divergent Change instead).
- **`crash_recovery.rs` 3 platform impls = copy-paste Parallel Inheritance** — refuted by both: three materially different adapters behind one trait surface; correctly factored.
- **`server/tls.rs` smell** — refuted: it consumes the rustls config, doesn't build it; nothing to extract.
- **Network-tier retention = Switch Statements** — refuted: already a table (`NetworkTier::retention_limit`).
- **bb-resolution ladder duplicated** — refuted: centralized and short in `bb.rs:18-63`; the smell nearby is the version-string seam (F-08), not the ladder.
- **`mutate_config` is a Middle Man** — refuted: it's a legitimate extraction for its 6 command sites (the real issue is the *cross-crate* copies it can't reach — F-13).
- **config fields are a Data Clump (as stated)** — refuted as framed; the real clump is the `(approved_origins, auto_approve_localhost)` pair at the `is_approved` seam (folded into F-08/F-13 context).

## Out-of-scope observations (one line each — not maintainability)
- The `::1` bracket handling may semantically differ between `host.rs` and `authorization.rs` — a correctness question (`/harden bugs`), not scored here.
- Security was intentionally not audited (the surface had a dedicated `/harden security` re-audit on 2026-06-09).
- SDK offline-cache test uses a hardcoded 50 ms wall-clock threshold — potential CI flakiness (bug/test-reliability, not a maintainability smell).

## Cross-cutting observations
- **"Comment-enforced invariants" is the dominant theme.** F-01, F-06, F-09, F-10, and F-13 are all the same root pattern: an ordering or a two-place state held in sync by a `// must…` comment instead of a type/structure. The codebase's (excellent) habit of heavy why-comments tagged `SEC-xx`/`Q-xx`/`F-xx` is doubling as the *enforcement* mechanism — and F-01 proves that fails (the M1 bug). The systemic fix is to convert the highest-traffic of these into structural seams (guard objects, owned state transitions, template methods).
- **Prior refactors genuinely paid down the big-ticket duplication.** The core extraction (#325, F-01-era) unified AppState/gating across the desktop and headless crates; what remains cross-crate is a ~6-line tracing bootstrap (Low) and the stranded tests (F-12). Don't re-litigate the crate split — it's deliberately not a workspace and that's working.
- **Parse-don't-validate is adopted but not finished.** `AztecVersion` and `CanonicalOrigin` exist and are correct at their construction edge, then decay to `&str` downstream (F-08). The pattern is right; the follow-through is the work.
