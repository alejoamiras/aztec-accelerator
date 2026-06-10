# Plan — quality-fixes-2026-06-10 (blueprint mid) — ✅ COMPLETE 2026-06-10

**All 15 findings resolved:** 13 implemented behavior-preserving across PR-1 #349 (SDK), PR-2 #350
(Rust seams), PR-3 #353 (day-scale) — every PR merged green (macOS+Windows legs) with `/code-review
max` + a codex post-impl audit (SHIP ×3, zero behavior deltas); 2 sub-items deferred as tracked
issues (#351 F-14 loopback dedup, #352 F-08 "unknown" sentinel). Characterization-first tests landed
BEFORE their refactors for F-03/F-01/F-06/F-10. Lessons: `lessons/phase-{1,2,3}.md`.

Implement all 15 findings from the `/harden-quality max` run `2026-06-10-max-q7e3`
(`audit/quality/2026-06-10-max-q7e3/report.md`). **Behavior-preserving refactors only** — every
change keeps the existing test/lint/E2E gate green; no public SDK API change; no security regression
on the paths the 2026-06-09 hardening re-audit confirmed closed.

## Locked decisions (from clarifying answers)
- **Scope:** all 15 findings (F-01..F-15).
- **PR shape:** fewer/bigger — **3 PRs** (SDK / Rust-seams / day-scale), respecting the report's ship-together sets.
- **F-08 depth:** both newtypes, full (thread `AztecVersion` + `CanonicalOrigin` through the hot paths).
- **Closure:** standard blueprint close (`/code-review max --fix` + codex post-impl audit); findings have exact file:line traces, so closure is checkable against the report. No extra `/harden` re-run.
- **Quality bar:** production (released app + published SDK).

## Success criterion
- All 15 findings ✓ in this plan; each maps to a merged-green PR.
- `bun run test` (lint + tsc + unit) exit 0; `cargo test` + `cargo clippy --all-targets -- -D warnings` on **all three crates (core + src-tauri + headless `server`)** exit 0; SDK `tsc --noEmit` green; playground + SDK E2E (HTTP + Safari HTTPS) green; **the macOS + Windows CI legs green** (F-01/F-10 are OS-gated) — all in the transcript.
- `public-contract.test.ts` still passes (proves SDK public surface + docs unchanged).
- `/code-review max --fix` applied+committed; codex post-impl audit clean or High/Critical addressed.
- No behavior change: the refactors are structural; the existing characterization tests (the SEC-/Q-/F- pinned ones) pass unchanged.

## Central thesis
The report's dominant theme is **invariants enforced by a `// must…` comment instead of by a type or
structure** (F-01/F-06/F-09/F-10/F-13). The plan's spine is converting the highest-traffic of those
into structural seams (template method, owned state transition, guard object, shared helper), plus the
mechanical extractions (F-04/F-05/F-11/F-12), the typed-error channel (F-03), the module split (F-07),
and finishing parse-don't-validate (F-08). Each is independently revertible.

---

## PR structure (3 PRs; sequence matters where files overlap)

### PR-1 ✅ MERGED #349 — SDK maintainability (one package, type-stable) — `packages/sdk`
*F-02 ✓ · F-06 ✓ (characterization-first) · F-05 ✓ · F-11 ✓. tsc + 45/45 unit; /code-review clean; codex post-impl SHIP (behavior-preserving confirmed); CI green; merged 2026-06-10.*
*Independent of the Rust work; can land first or in parallel. The 4 findings all live in the 440-LOC
hotspot file, so they ship together by necessity (the report's "ship together" set).*

- **F-02 — Move public types to `lib/types.ts`.** Cut the 6 published types (`AcceleratorConfig`,
  `AcceleratorPhase`, `AcceleratorPhaseData`, `AcceleratorProtocol`, `AcceleratorProverOptions`,
  `AcceleratorStatus`) from `accelerator-prover.ts:11-92` into a new `lib/types.ts`; re-export from
  `index.ts` **byte-identically** (consumers + `public-contract.test.ts` see no change). Fix the
  `accelerator-transport.ts:3` back-import to point at `types.ts` → kills the 2-way edge.
- **F-05 — Extract `#probeAndParseHealth` (`:233-328`) + status factory.** Pull parse / version-policy
  into named privates; build the `AcceleratorStatus` variants through one factory. **Naming:** don't
  shadow the existing Q5 helpers `#fallbackToWasm`/`#proveLocally`.
- **F-06 — Own the protocol-pin transition in transport — THREE states, not pin-from-discriminant**
  *(audit-blocking, both auditors)*. The pin rule is asymmetric: `response.ok` → **set**; malformed
  JSON (`:268`) → **clear**; `!response.ok` (`:245-253`) → **KEEP** the existing pin. A naive
  "derive pin from discriminant" would unify the two `reason:"error"` exits and silently change which
  endpoint a later `/prove` hits (only no-pin-on-`!ok` is test-pinned at `prover.test.ts:369`, not
  keep-vs-clear). `transport.commitStatus(status, {keepProtocol})` must encode set/clear/**keep**.
  **Add a keep-vs-clear characterization test BEFORE the refactor.**
- **F-11 — Split `createChonkProof` (`:330-398`)** into a remote-prove / decode-with-duration shape
  (reuse, don't shadow, the existing `#fallbackToWasm`/`#proveLocally`). Phase-emission sequence
  preserved exactly (the phase-order characterization test is the guard).
- **Validation:** `bun run --cwd packages/sdk test` (incl. `public-contract.test.ts` + the **27** prover
  tests + 10 transport + phase-order guard) + `tsc --noEmit`. SDK E2E (`connectivity`, `proving`) green.
- **Risk:** low — pure TS refactor, no API change. The dense existing unit suite is the safety net (F-06
  needs the new keep-vs-clear test added first).

### PR-2 ⬜ — Rust seams: comment-enforced-invariant → structural (core + src-tauri)
*The bulk. All hours-each, independent Rust changes; ordered as bisectable commits. **Internal
sequence:** F-12 (move tests) FIRST so later edits land in the relocated files; then the rest.*

- **F-12 — Move stranded tests** out of `core/src/server.rs:330-1424` into the modules they exercise
  (`server/prove.rs`, `server/auth.rs`, `server/host.rs`). **Not purely mechanical** *(both auditors)*:
  the shared fixture `auth_state_with_popup` (`server.rs:935`) serves ~10 tests, and several
  router-level tests (`server.rs:669`, `:1112`) span prove+auth+host through the full `router()` stack
  → extract a shared `#[cfg(test)]` support module and **keep cross-cutting/router-level tests in
  `server.rs`** (don't force them out). Submodule privacy moots the private-item risk (children see
  parent privates). Net `server.rs` ~1424→~700 LOC. **First commit** (shrinks F-03's diff surface).
- **F-03 — replace the `ProveError` wire representation** *(NB: `ProveError` already exists as a tuple
  alias at `server.rs:312`; this REPLACES it, not introduces it — audit fact)*. Give the 11
  `text/plain` `{error,message}` sites (`prove.rs` ×5, `auth.rs` ×6) one hand-written `IntoResponse`
  that **delegates to `(StatusCode, String).into_response()`** so the bodies stay `text/plain` (the
  SDK's `ky err.data` parsing + `prove_error_responses_stay_text_plain_json_string` at
  `server.rs:662-695` depend on it). **EXCLUDE `host.rs:69-72`** — keep its `403` +
  `application/json {"error":"invalid_host"}` (no `message`) **byte-identical** (give it a verbatim
  variant or leave it outside the enum). **No new dependency** (no `thiserror`/`anyhow`). **Add a
  content-type/body characterization test for `invalid_host` AND the text/plain sites BEFORE.**
- **F-01 — `mode`-aware `prepare_https(Launch|Settings)` in `src-tauri/certs.rs`** *(NOT core — certs
  uses `rcgen` + the macOS `security` CLI; audit fact)*. The two paths genuinely diverge — the `mode`
  must carry **≥4 differences** *(both auditors)*: **Launch** (`main.rs:55-94,424-428`) verifies trust
  only (NO Keychain prompt — deliberately off the startup path, `main.rs:86-93`), never generates,
  resets `safari_support` on missing-certs/load-failure but **skips-without-reset** on untrusted, and
  spawns background leaf renewal; **Settings** (`commands.rs:151-184`) generates, installs trust (the
  prompt), and saves `safari_support=true` **between** trust and load. Keep **SEC-08 migrate-first
  fail-closed** and the settings-path user-facing error strings (`commands.rs:162-177`, not test-pinned
  → don't drift). The shared part is `migrate` + the post-load spawn; the divergent steps stay
  mode-gated. **Add a launch-vs-settings behavior characterization test BEFORE.**
- **F-04 — Extract `.setup()`** (`main.rs:314-466`) into a `DesktopBootstrap` shape. **Two ordering
  hazards to preserve** *(audit)*: `status` is cloned (`:362-363`) BEFORE being moved into the
  `on_versions_changed` closure (`:370-384`) — keep the clone-before-move; and `app.manage::<SharedAppState>`
  (`:433`) MUST stay before the webdriver settings-window open (`:448`) and the HTTP spawn — a phase
  reorder breaks commands/webdriver E2E at runtime, not compile time. Escape hatch: keep any fighting
  binding inline.
- **F-09 — Close the dual-map Temporal Coupling** (`authorization.rs:172-241`). **Decide D-2 with F-08
  in view** *(audit)*: F-08's `CanonicalOrigin` threading (PR-3) also rewrites this exact range. If we
  extract `PendingAuthorizations`, key it by `CanonicalOrigin` from the start so PR-3 doesn't re-key;
  if we drop `by_origin` for a ≤10-entry scan, the "thread through `by_origin`" item vanishes. (See D-2.)
- **F-10 — RAII `suspend_for_update` guard** (`crash_recovery.rs`) that **re-arms (conditionally, via
  `rearm_crash_recovery_if_enabled`) on drop**; `updater.rs:146-189` uses it instead of the 3 hand-placed
  rearm sites. **NO `defuse()` on the restart path** *(audit-blocking, both)* — current code re-arms
  BEFORE `app.restart()` (`updater.rs:163-171`) precisely so a failed relaunch isn't left unprotected;
  defusing there inverts SEC-08-adjacent intent. So re-arm before restart, then proceed. **Windows-only**
  (`#[cfg(windows)]`). RAII adds rearm-on-panic that doesn't exist today — **acknowledge this is NOT
  strictly behavior-preserving** (it's strictly safer). **No test pins this ordering today** — add a
  rearm-before-restart test BEFORE (corrects the plan's earlier false "tests pin this" claim).
- **F-13 — generalize the lock-mutate-save helper into `core/config.rs`, RETURNING `Result`; each call
  site keeps its current disposition** *(audit-blocking both + final-codex clarification — resolves D-3)*.
  Precise current state: `commands.rs:10` `mutate_config` **is the existing helper** (the desktop
  commands already use it; it ALWAYS saves + propagates via `?`); the two HAND-ROLLED copies are
  `auth.rs:117-126` (saves only when `!approved_origins.contains`, then **warns + returns Ok** — a
  save failure must NOT fail a user-approved prove, SEC-04 availability) and `main.rs:98-104`
  `reset_safari_support` (always-mutate, `let _` swallow). The core helper takes `f: FnOnce(&mut Config)
  -> bool` (save iff `true`) and returns the save `Result`: **`mutate_config`'s callers pass closures
  returning `true` → always-save preserved byte-for-byte**; `auth.rs` returns the `!contains` result →
  its conditional save preserved; each caller still applies `?` / `warn!` / `let _`. **Zero behavior
  change.** (`mutate_config` stays a thin src-tauri wrapper over the core helper so its 6 command sites
  are untouched.)
- **F-14 — `is_loopback_host()` in core** — **2 sites, not 3** *(audit: `canonicalize_origin` has no
  loopback set)*: `host.rs` `host_is_trusted` and `authorization.rs` `is_auto_approved`. They **DO
  differ** today (`host_is_trusted` strips brackets → matches `"::1"`; `is_auto_approved` matches
  `"[::1]"` via `url::Url::host_str`) → **D-4 triggers**: this is NOT a silent unification. Surface the
  `::1` divergence as a flagged decision (route the correctness question to `/harden bugs`); if kept as
  a quality fix, the helper is parameterized for bracket-handling and each caller passes its current
  behavior. **DEFERRED by default (D-4)** — filed as a tracked follow-up; folded in only on gate override.
- **F-15 — Parameterize `config.rs` load/save** → `load_from(path)`/`save_to(path)` + thin
  hardcoded-path wrappers; point the roundtrip test at a tempdir so it stops re-implementing `save`.
- **Validation per commit:** `cargo test` + `cargo clippy --all-targets -- -D warnings` on the touched
  crate; full `cargo test` (core + src-tauri + **headless `server`**) before push. **The PR-2 CI gate
  MUST run the macOS AND Windows legs** *(audit)* — F-01 (macOS cert path) and F-10 (`#[cfg(windows)]`)
  aren't exercised by a green local one-OS run. WebDriver E2E green (auth/HTTPS paths).
- **Risk:** moderate — F-01/F-10 touch security-hardened paths. Mitigation: characterization-first
  (write the pinning test BEFORE each of F-01/F-03/F-06/F-10), behavior-preserving, the SEC-/F- tests
  are the guard, the 2026-06-09 re-audit is the baseline, and the cross-OS CI legs exercise the gated code.

### PR-3 ⬜ — Day-scale Rust (cuttable last) — `core`
*Sequenced AFTER PR-2 (both edit `prove.rs`/version signatures); rebase on PR-2. Either finding can be
dropped without stranding the others.*

- **F-07 — Split `versions/mod.rs`** (1006 LOC) into `version_policy` / `cache_layout` /
  `release_metadata`; `downloader.rs` depends on explicit helpers instead of the 9-item `use super`.
  Leave `NetworkTier::retention_limit` (already a clean table). Tests move with their code.
- **F-08 — Finish parse-don't-validate (both newtypes).** *(All threading is behavior-preserving:
  `Borrow<str>`/`Display` round-trip the exact strings — pinned by `aztec_version_parse_matches_is_valid_version`.)*
  - `AztecVersion`: thread `&AztecVersion` through `find_bb`, `bb::prove`, `version_bb_path`,
    `download_url`, `cleanup_old_versions`. **Borrow hazard** *(audit)*: collapsing `ResolvedVersion`'s
    double representation must keep the version available for `bb::prove` (`prove.rs:156-200`) after
    `to_download` is moved into the download arm → use `as_ref`/`as_deref`, don't move-then-need.
  - **The `"unknown"` sentinel → `Option` is NOT mechanical and is CARVED OUT** *(both auditors)*: the
    SDK keys on the literal (`accelerator-prover.ts:302`, `acceleratorVersion !== "unknown"`),
    `AztecVersion::parse("unknown")` currently *succeeds*, and `cleanup_old_versions` runs eviction with
    bundled=`"unknown"`; switching to `Option::None` changes eviction in unknown-bundled builds AND a
    wire contract the SDK reads. **`/health` keeps emitting `"unknown"` byte-identically.** This
    sub-item is **deferred** (it's a cross-boundary contract change, not a quality refactor — file as a
    follow-up if wanted). F-08 = signature threading only.
  - `CanonicalOrigin`: thread through `request()`, the popup callback, and the pending-map (keyed by
    `CanonicalOrigin` — coordinate with F-09/D-2). **`remove_approved_origin` (`commands.rs:64-72`)
    keeps EXACT-STRING match — do NOT canonicalize the removal input** *(final-codex: today a
    canonicalizable-but-non-canonical input is a silent no-op; parsing it would newly delete a stored
    entry = behavior change. Leave this command on raw exact-match against the stored string)*. Delete
    the "already canonical" comment-contracts elsewhere that the newtype was built to kill.
  - **Behavior-preserving:** the newtypes already exist + are validated at construction; this only
    moves the boundary inward. The auth-path threading must keep the SEC-04/06 canonicalization exactly
    (re-run the auth characterization tests).
- **Validation:** full `cargo test` + clippy; WebDriver E2E.
- **Risk:** F-08 is the widest blast radius. Mitigation: mechanical signature threading, behavior-
  preserving, characterization tests + clippy as guards; it's the last PR so it can be cut if CI churns.

---

## Competing outline (Alternative B — smell-typed, safest-first risk gradient)
A different grouping axis, for the audit to weigh against the package-split above. Instead of
package boundaries, order **mechanical→risky** and group by **smell type**:
- **PR-A "zero-logic moves":** F-12 (move tests), F-15 (config path param), F-02 (types.ts move), F-14 (loopback helper). Pure relocations/extractions — fastest confidence, mergeable same-day.
- **PR-B "Long-Method extractions":** F-04 (.setup), F-05 (probe), F-11 (createChonkProof), F-07 (versions split).
- **PR-C "comment-invariant → seam":** F-01 (prepare_https), F-06 (protocol pin), F-09 (PendingAuth), F-10 (suspend guard), F-13 (lock_mutate_save) — the behaviorally riskiest cluster, reviewed together.
- **PR-D "typed channels":** F-03 (ProveError), F-08 (newtypes) — widest blast, last.

**Why the main plan wins (but the audit decides):** Alt B gives a clean risk gradient + thematic review
coherence, BUT it **splits the one 440-LOC SDK file across 3 PRs** (F-02 in A, F-05/F-11 in B, F-06 in
C) — violating the report's ship-together constraint and forcing 3 rebases on one file — and each PR
crosses package boundaries, defeating CI path-scoping (every PR triggers both the accelerator and SDK
gates). The main plan keeps the SDK file in one PR and lets path filters skip unaffected gates, at the
cost of mixing risk levels inside PR-2 (mitigated by per-finding commits + safest-first internal order).
**Adopted from Alt B into the main plan:** the *internal* safest-first ordering (mechanical commits
first within each PR) and the explicit risk-gradient framing.

## Decision ledger
- **D-1 — 3 PRs, not 6 or 15 (user pick: fewer/bigger).** Split by package/risk boundary: SDK (type-
  stable, independent) | Rust seams (hours-each) | day-scale (cuttable). Respects the report's ship-
  together sets (SDK four; the two `main.rs` items F-01+F-04 in one PR). *Rejected:* 6 theme PRs (more
  CI/review overhead than the user wants); 1-per-finding (15 × ~15min CI). *Accepted cost:* PR-2 is
  large (9 findings) — mitigated by bisectable per-finding commits + the option to split if CI/review
  gets unwieldy.
- **D-2 — F-09 `PendingAuthorizations` vs drop-`by_origin`: decide WITH F-08 in view** *(audit-tightened)*.
  Both kill the Temporal Coupling AND both PR-2-F-09 and PR-3-F-08 rewrite `authorization.rs:172-241`.
  **Resolution:** if extracting `PendingAuthorizations`, key it by `CanonicalOrigin` from the start
  (so PR-3 doesn't re-key); if dropping `by_origin` for a ≤10-entry scan, the F-08 "thread through
  `by_origin`" item vanishes. Pick at impl-time on smaller-diff; either is behavior-preserving.
- **D-3 — F-13 save-failure policy: RESOLVED by both auditors → shared helper, per-site disposition, no
  unified policy.** The earlier "unify to propagate" was **wrong**: `auth.rs:117-126`'s warn-and-Ok is
  load-bearing (a config-save failure must NOT fail a user-approved `/prove` — SEC-04 availability), and
  `reset_safari_support`'s swallow is intentional. The helper returns `Result` + a `bool`-returning
  closure (for the conditional save); each caller keeps `?` / `warn!` / `let _`. **Zero behavior
  change** — D-3 is no longer a flagged behavior change.
- **D-4 — F-14: FIRM DECISION = DEFER to a tracked follow-up** *(final-codex: make it firm so the PR
  count is stable)*. The 2 sites genuinely differ (`host_is_trusted` matches `"::1"` brackets-stripped;
  `is_auto_approved` matches `"[::1]"`), so consolidation is NOT a behavior-preserving quality move —
  whether they *should* match is a correctness question that belongs in `/harden bugs` first. **Plan
  default: implement 14 findings now; file F-14 as a tracked follow-up issue** (a pure quality
  consolidation is only safe once the bug audit confirms the intended loopback semantics). **Gate
  override available:** if you'd rather fold it in now, it ships as a bracket-parameterized
  `is_loopback_host(s, brackets)` where each caller passes its current behavior (still no behavior
  change, just no de-duplication of the *difference*). (`canonicalize_origin` is NOT a third site.)
- **D-5 — Sequencing:** PR-1 independent. PR-2 internal order F-12→F-03→rest (F-12's relocation must NOT
  evict cross-cutting/router tests — keep those in `server.rs`). PR-3 after PR-2 (shared `prove.rs`/
  version signatures AND `authorization.rs:172-241` per D-2), rebased.
- **D-6 — F-numbering collision** *(audit)*: the codebase already carries `(F-01)`/`(F-03)`/`(F-08)`
  comments + a "F-05 doc-sync guard" test from EARLIER plans (e.g. `prove.rs` `(F-08)` = the 06-08
  plan). **All new code comments + commit subjects use the run-id prefix `q7e3-F-NN`** to avoid aliasing.

## Security & Adversarial Considerations
- **Threat surface touched:** F-01 (Safari-HTTPS cert bring-up), F-08 (`CanonicalOrigin` through the
  auth path), F-10 (Windows updater/crash-recovery), F-03 (the `/prove` + auth error responses). These
  are exactly the paths the 2026-06-09 security re-audit confirmed closed.
- **Invariant:** every change is **behavior-preserving**. The security properties (SEC-01 host guard,
  SEC-04 localhost prompt, SEC-06 request-id keying, SEC-08 fail-closed cert migration) are pinned by
  named characterization tests; those tests pass **unchanged** or the change is wrong. F-01 must keep
  the fail-closed-on-migrate-error semantics; F-08 must keep canonicalization byte-identical; F-03 must
  keep the error body shape + status codes the auth/prove tests assert.
- **Least privilege / supply chain:** no new dependencies (F-03 hand-writes `IntoResponse`; no
  `thiserror`/`anyhow`). No CI/token/publish surface touched. `bun.lock`/`Cargo.lock` only change if a
  dep is added — none planned; if F-anything needs one, it's a flagged decision.
- **No new error-message disclosure:** `ProveError`'s `IntoResponse` must emit the same `{error,
  message}` bodies — not richer internal detail (the host-guard body deliberately omits the offending
  host; F-03 must preserve that minimal shape, not unify it richer).

## Assumptions
**Facts (verified):**
- The 15 findings + exact file:line traces are in `audit/quality/2026-06-10-max-q7e3/report.md` (+ `findings/verified.md`), Fable↔Codex convergent.
- `json_error` is defined `core/src/server.rs:326` returning `String`; 11 call sites; `host.rs:69-72` uses a divergent `axum::Json` shape (re-read this session).
- The lock-mutate-save pattern is the existing `mutate_config` helper (`commands.rs:10`, always-saves + propagates, ~6 command callers) PLUS two hand-rolled copies that diverge: `core/server/auth.rs:117-126` (conditional save, warn + return Ok) and `src-tauri/main.rs:98-104` (always-mutate, swallow) — final-codex correction to the earlier "3 copies" framing.
- The 3 crates are deliberately NOT a Cargo workspace (per the Rust map + `core/Cargo.toml` comment) — refactors must not introduce a workspace.
- House convention: no `thiserror`/`anyhow`; `Box<dyn Error>` / `Result<_,String>` / `(StatusCode,String)` (Rust map).
- `public-contract.test.ts` gates the SDK barrel + doc sync — the F-02 re-export guard.
- main is branch-protected; PR-only, green CI per merge (project memory).
- `ProveError` already exists as a `(StatusCode, String)` tuple alias at `server.rs:312` (codex) — F-03 REPLACES its wire handling, doesn't introduce it.
- The 11 text/plain sites are pinned by `prove_error_responses_stay_text_plain_json_string` (server.rs:662-695); `invalid_host` host-guard test pins only status+substring (server.rs:1131-1139); SDK reads error bodies via `ky err.data`.
- `auth.rs:117-126` save-on-`!contains` warns + returns Ok (SEC-04: a save failure must not fail a user-approved prove). `is_auto_approved` matches `"[::1]"`, `host_is_trusted` matches `"::1"` — they differ.
- `AztecVersion::parse("unknown")` succeeds today; `/health` emits `"unknown"`; SDK keys on it (prover.ts:302); no updater/crash-recovery characterization test pins disarm/rearm ordering (only smoke + task_xml/plist + size_from_feed).

**Inferences (audit-attacked → resolved; residual risk noted):**
- F-03 preserves status+body **only if** `IntoResponse` delegates to `(StatusCode, String)` (text/plain) AND `host.rs` is excluded → folded into the plan. *Resolved.*
- F-12 is **mostly** mechanical but needs a shared `#[cfg(test)]` support module + keeping router-level tests in `server.rs` → folded. Submodule privacy moots the private-item risk. *Resolved.*
- F-08 threading round-trips exact strings (pinned) **except** the `"unknown"`→`Option` sub-item, which is a cross-boundary contract change → **carved out/deferred**. *Resolved.*
- F-04 clone/move preserved with two named ordering hazards (clone-before-move of `status`; `app.manage` before webdriver/HTTP spawn) → folded. *Residual: a borrow fight → keep the binding inline.*
- F-06 cannot derive pin-from-discriminant (keep-vs-clear asymmetry) → 3-state `commitStatus` + new test → folded. *Resolved.*

**Asks (resolved):** scope (all 15), PR shape (3 PRs), F-08 depth (both newtypes; `"unknown"` sentinel sub-item deferred), closure (standard). D-3 (swallow→propagate) **withdrawn** (shared helper + per-site disposition = zero behavior change). **Scope clarified by the audits → plan default = implement 14 now + defer 2 sub-items as tracked follow-ups: F-14** (loopback `::1`/`[::1]` — correctness question for `/harden bugs` first) and the **F-08 `"unknown"` sentinel→`Option`** (SDK wire contract). **Two gate decisions for you:** (1) accept those 2 defers, or fold either in now; (2) nothing else open.

## Test plan
- **Characterization-FIRST (write the pinning test BEFORE the refactor — these gaps let a regression pass CI today):**
  - **F-03:** assert `invalid_host` = 403 + `application/json` + body `{"error":"invalid_host"}` (no `message`); AND the text/plain `{error,message}` content-type for representative prove/auth error ids. *(Strengthens server.rs:1131-1139 which only checks status+substring.)*
  - **F-01:** a launch-vs-settings behavior test — launch path never prompts/never generates; both keep SEC-08 migrate-first fail-closed; settings persists `safari_support` at the right step.
  - **F-06:** keep-vs-clear — an already-pinned protocol survives a later `!response.ok`, and is cleared by malformed JSON. *(prover.test.ts pins only no-pin-on-!ok.)*
  - **F-10:** rearm-before-restart on the success path (Windows) — no test pins this today.
- **Per finding:** the existing characterization test that pins the touched behavior must pass unchanged. New coverage: F-15 (`save_to(tempdir)` roundtrip replacing the re-implemented `save`), F-04 (smoke per extracted bootstrap fn if cheap).
- **Per PR:** full `cargo test` (core + src-tauri) + `cargo clippy --all-targets -- -D warnings`; `bun run test`; the relevant E2E (SDK E2E for PR-1; WebDriver for PR-2/PR-3's auth/HTTPS paths).
- **Whole:** `public-contract.test.ts` green throughout (SDK contract unchanged); the SEC-/F- characterization suite green (security unchanged).

## Migration & docs
- No user-facing or API change → no README/MIGRATION edits expected. If F-02's `types.ts` changes an
  import path anyone documents, update it (shouldn't — `index.ts` re-exports identically).
- Update `implementations-plan/index.md` (new entry) + `CLAUDE.md` only if a module path it names moves
  (F-07 splits `versions/mod.rs` — update the one-line module description if present).
- Per-finding lessons in `lessons/phase-{1,2,3}.md` (one per PR).

## Post-implementation hardening
Not scheduled. The source was a `/harden-quality max` run 2 days ago and the security surface was
re-audited 2026-06-09; behavior-preserving refactors don't change the threat model. Standard blueprint
close (code-review + codex post-impl) is the verification. *(User picked "standard close only".)*

## Audit verdicts (dual audit — both CONDITIONAL APPROVE; conditions folded above)
- **Codex (round 1) — `conditional approve`** (`audit-codex.md`): conditions = keep `invalid_host` out of the `{error,message}` unification (F-03); F-01 mode-aware not one shared path; don't adopt D-3 propagate-by-default; remove F-10 `defuse()` on restart. Unique fact: `ProveError` already exists (server.rs:312). **All four folded.**
- **Fable subagent — `conditional approve`** (`audit-fable.md`): same four conditions + F-06 3-state transition; plus secondary catches (F-12 not purely mechanical, F-14 is 2 sites that differ, F-08 `"unknown"` sentinel is a contract change, F-09/F-08 both rewrite authorization.rs, gate omits headless `server`, F-numbering collision, `certs.rs`∈src-tauri). **All folded.**
- **Convergence:** both models independently produced the same four blocking conditions → highest-confidence signal; adoption was not a judgment call.
- **Final fresh codex (revised plan) — `conditional approve`** (`audit-codex.md`): **confirmed the 5
  folded conditions check out against current source** (server.rs:312-327, host.rs:66-72, main.rs:55-94/424-428,
  commands.rs:151-184, updater.rs:163-171). 3 refinements, all folded: F-08 `remove_approved_origin` =
  exact-string match (parse-or-no-op would still change behavior); F-13 = `mutate_config` is the
  existing always-save helper (preserve always-save via the `bool` closure); F-14 = make the defer
  decision firm (done — D-4). **No new blocking issues.**

## Standing implementation conditions (approved 2026-06-10)
- **Per-PR codex post-impl audit** on EACH PR's diff (not just one at the end) — the approved tightening, since the refactors touch security-hardened paths. `/code-review max --fix` then `/codex xhigh` on each PR's net diff before merge.
- **Characterization-test-FIRST** for F-01/F-03/F-06/F-10 (write the pinning test, watch it stay green through the refactor).
- **F-14 + F-08 `"unknown"` sentinel: filed as tracked follow-up issues** (deferred per gate decision), not implemented.
- **`q7e3-F-NN` prefix** in all new code comments + commit subjects (avoid aliasing prior plans' F-numbers).
- Never push to main directly; never merge without green CI; cross-OS (macOS+Windows) legs must be green for PR-2.

## Seeds (FINAL — approved scope)
**/goal (recommended — completion is transcript-observable via plan.md ✓-marks + green gates):**
```
/goal All 3 PRs (PR-1 SDK = F-02/05/06/11; PR-2 Rust seams = F-12/03/01/04/09/10/13/15; PR-3 day-scale = F-07/08-minus-sentinel) for quality-fixes-2026-06-10 merged-green; F-14 and the F-08 "unknown" sentinel filed as tracked follow-up issues (count ✓-deferred); every other finding ✓ in plan.md; the characterization-first tests (F-03 invalid_host + text/plain, F-01 launch-vs-settings, F-06 keep-vs-clear, F-10 rearm-before-restart) added BEFORE their refactor and green; for each PR printed LESSONS_FILE=implementations-plan/quality-fixes-2026-06-10/lessons/phase-N.md; /code-review max --fix applied+committed per PR AND a codex post-impl audit run on EACH PR's diff with high/critical addressed; bun run test + cargo test + cargo clippy --all-targets -- -D warnings (core+src-tauri+server) + SDK tsc --noEmit exit 0 and the macOS+Windows CI legs green in the transcript; public-contract.test.ts + the SEC-/F- characterization suite pass UNCHANGED (no behavior/security/API change). Never push to main directly; never merge without green CI.
```
**/loop 15m (fallback):**
```
/loop 15m Drive implementations-plan/quality-fixes-2026-06-10 forward, never idle. Each firing: read plan.md + lessons/ (authoritative); git status + log; PR open? gh pr view --json statusCheckRollup (no --watch); CI in-flight? gh run watch up to 10m. No task in hand? next pending finding in order (PR-1 F-02→F-06[test-first]→F-05→F-11; PR-2 F-12→F-03[test-first]→F-01[test-first]→F-04→F-09→F-10[test-first]→F-13→F-15; PR-3 F-07→F-08) → characterization-test-first where required → edit → cargo/bun test + clippy → commit (q7e3-F-NN prefix) → push. Stuck/decision? /codex xhigh, decide, log; never merge to main / never push to main / never expand scope. 5 fails same step? stop, reassess w/ codex. Phase green? mark ✓ in plan.md, file lessons, print LESSONS_FILE=…, advance. All ✓? per-PR already had /code-review max --fix + codex post-impl; write the wrap-up, surface, stop. Keep the ASCII checklist visible.
```

## Status (live — updated each firing)
- **PR-1 ✅ MERGED #349:** F-02 ✓ · F-05 ✓ · F-06 ✓ (characterization-first) · F-11 ✓.
- **PR-2 (branch `quality/pr2-rust-q7e3`, in progress):**
  - F-12 ✓ (7cb82f1 — server.rs 1424→331; 132/132)
  - F-03 ✓ (d02f914 test-first + 0c0d0d5 enum; text/plain + invalid_host pinned; 133/133)
  - F-15 ✓ (f31b203 — config load_from/save_to; 133/133)
  - F-09 ✓ (PendingState::insert/remove encapsulation; auth-flow tests + 133/133)
  - F-13 ✓ (core::config::lock_mutate_save + 3 callers; core 133/133 + src-tauri 19+3, both crates clippy-clean)
  - F-10 ✓ (CrashRecoveryGuard rearm-before-restart, test-first; macOS clippy+3 guard tests, Windows path CI-validated)
  - F-01 ✓ (codex-consulted LIGHTER-SHAPE: pure LaunchHttpsGate classifier, 4 characterization tests FIRST incl. short-circuits + reset-vs-skip; deviation AFK-logged in lessons/phase-2.md)
  - F-04 ✓ (build_tray + build_desktop_state extracted; status consumed-by-value = clone-before-move compiler-enforced; manage-before-webdriver/HTTP kept inline; clippy default+webdriver features clean)
  - ALL 8 PR-2 findings done — close sequence: /code-review max --fix → codex post-impl → push → cross-OS CI → merge
- **PR-3 (branch `quality/pr3-dayscale-q7e3`, code complete):**
  - F-07 ✓ (709c97a — versions/mod.rs 1006→18-line hub + version_policy/cache_layout/release_metadata; downloader absorbs its 9 stranded tests; exactly 133/133)
  - F-08 ✓ (ce83ca3 AztecVersion through find_bb/bb::prove/version_bb_path/download_url/cleanup + ResolvedVersion collapse [as_ref discipline]; 3439b78 CanonicalOrigin through request/pending-map/popup callback; remove_approved_origin untouched; "unknown" sentinel deferred → #352)
- **DEFERRED ✓-resolved (tracked issues FILED):** F-14 → **#351** (loopback dedup; `/harden bugs` first) · F-08 `"unknown"` sentinel→Option → **#352** (SDK wire contract).
- **Per-PR close still owed:** PR-2 + PR-3 each need `/code-review max --fix` + codex post-impl on the diff + green cross-OS CI before merge.
