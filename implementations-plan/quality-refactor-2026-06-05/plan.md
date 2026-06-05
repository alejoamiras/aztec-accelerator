# Quality-debt refactor — accelerator + SDK (behavior-preserving)

**Tier:** `/blueprint mega-deep` (3 parallel plans → consolidate + ledger → contradiction-check → double audit → split Round 2 → final codex). Verdicts inline.
**Status:** ✅ **APPROVED (2026-06-05)** — blueprint is the deliverable; implementation is the seeded `/goal` follow-on (not started). Owner decisions: **A** = standalone SDK semver + compat table (separate prerequisite gating Q12); **B** = refactor + ship the 3 behavior fixes, communicating each to the owner when reached; **C** = absorb minors; **D** = Windows-rc checkpoints; **E** = Q12 PR carries migration + publish-guard; **F** = keep everything (15 + minors).
**Source:** `audit/quality/2026-06-05-sdk-accel/report.md` (Q1-Q15 + minors). Research: `research/*.md`.

## Owner decisions (Phase 0)
1. **Scope = EVERYTHING** (15 + minors), incl. the risky core-path splits Q1/Q2.
2. **SDK = clean breaking change** (Q12 → discriminated unions, SDK major bump, migration doc).
3. **Testing = characterization-first** (pin current behavior before touching hot paths).
- Post-impl hardening: re-run `/harden quality` at the end to confirm debt reduction (recommended, not auto-scheduled).

## Tier rationale (Phase 0.5 rubric)
Novelty LOW (we wrote + just audited this code) but blast-radius HIGH (server.rs/AppState core path + crash-recovery/updater safety net), irreversibility MED (published SDK break), migration cost MED (SDK consumers + char tests), external coupling HIGH (published @alejoamiras/aztec-accelerator). 4 HIGH/MED dimensions + everything-scope → deep minimum; owner chose mega-deep for the split Round-2 hostile audit on the risky splits. Accepted.

---

## Guiding principles
- **Behavior preservation is the prime directive.** The app auto-updates real users (1.0.4 shipped). Every PR keeps `cargo test --lib` + `bun run test` + `bun run lint` + `bun run lint:actions` green; the risky core-path + safety-net changes additionally ride an **rc dry-run** (the blocking updater-smoke gate) before any stable cut.
- **One finding (or tight group) = one small, independently-reviewable PR.** No mega-branch. main is branch-protected (PR + auto-merge).
- **Characterization-first per phase**: before touching a hot path, land the golden tests that pin *current* behavior, so the refactor PR shows the tests staying green.
- **Order = risk-ascending**: foundation → cheap safe wins → value objects → mid refactors → architectural splits → SDK break → minor sweep.

## Dependency graph
```
Phase 0 (char-test harness) ── unblocks everything
  Q8 typed errors ──┐ (independent, low-risk)
  Q15 const ────────┤
  pure minors ──────┘
Q3 AztecVersion ── subsumes {versions_to_evict re-parse, bb_asset_name minor}
Q10 ServerStatus ── couples server.rs emit + main.rs consumer (one PR)
Q11 download_bb split ── after Q3 (operates on AztecVersion)
Q4 crash-recovery trait ── safety-critical, independent
Q1 AppState split ── PRECEDES ── Q2 server.rs split (handlers take AppState)
Q5 SDK extraction ── PRECEDES ── Q12 SDK break (clean internals, then retype)
Q6 UpdateCoordinator, Q7 SafariManager ── safety-critical, after Q1 (use AppState)
Q14 auto-approve dedup ── independent (tests guard the set)
Q12 SDK major bump ── LAST functional change; lockstep playground update
minor sweep ── last
```

---

## Phases

### Phase 0 — Characterization-test harness (FOUNDATION, no production change)
Pin current behavior where research found gaps, so later refactors are provably behavior-preserving. PRs add tests only.
- **Rust (server):** full `/prove` happy-path with the **REAL ordering (codex critical correction):** auth → **body-buffer → semaphore → initial `Proving` → resolve_version/download → bb::prove** → `{proof}`+`x-prove-duration-ms` (server.rs:493-537). Pin **all three orderings** — auth-before-body (DoS), body-before-semaphore, and **semaphore-before-resolve** (without it, two concurrent requests for the same uncached version race into `download_bb`'s fixed `.{version}.tmp` + `remove_dir_all`/rename — a real corruption risk Q2 must NOT reorder). Assert **exact error JSON bodies + Content-Type** for all 11 error sites (the SDK-facing wire contract — see Q8 note); `on_status` call **sequence** (`Proving`→`Downloading bb...`→`Proving...`→`Idle` on the uncached path) + `StatusGuard` reset on every exit (this is the Q10 timing pin).
- **Rust (versions/bb):** `download_bb` atomic-rename-cleanup (tmp not stranded on extract failure); `find_bb` search-chain order; `versions_to_evict` empty/all-bundled edges.
- **Rust (crash-recovery):** a disarm→install→rearm **sequence** test (assert order) to complement the updater-smoke CI gate.
- **SDK:** pin the exact phase SEQUENCE per path (offline/available/download/403) + the `AcceleratorStatus` field combinations (so Q12's union mirrors today's reachable states).
- **Harness (codex's concrete answer):** no live `bb`/network. Rust = in-process axum `router(AppState)` via `oneshot`, `tokio::time::pause` for the 60s timeout path, and a **fake `bb` executable via `BB_BINARY_PATH`** to characterize the `/prove` *success* path (today every shell-out is gated behind `find_bb`, so the happy path is unpinned — the fake bb closes that gap). SDK = existing mocked `fetch`/`ky` promoted to golden fixtures. The download paths stay gated behind `ACCELERATOR_DOWNLOAD_TEST`.
- **Validation:** all new tests green on unmodified code. **Rollback:** trivial (test-only). Ship as 2 PRs (`rust-hot-path-characterization`, `sdk-contract-characterization`).

### Phase 1 — Cheap safe wins (low risk, no behavior change)
- **Q15**: `pub const AUTH_DECISION_TIMEOUT: Duration = Duration::from_secs(60)` in the lib crate; import in server.rs:361 + windows.rs:72. *(Extract Constant.)*
- **Q8 [corrected — Content-Type matters]**: build the body via an internal `ProveErrorBody{error,message}: Serialize` but **keep returning `(StatusCode, String)` so the response stays `text/plain`** (struct → `serde_json::to_string` → existing String body). Codex caught that `axum::Json` would flip Content-Type → `application/json`, changing `ky`'s `HTTPError.data` undefined→parsed-object — an **observable SDK runtime change** even with identical bytes. So the DTO is internal only; status + body bytes + `text/plain` stay byte+header-identical. Collapse the 11 inline `json!`/`json_error`+`.unwrap()` sites onto the builder. **Char test asserts byte-identical body + status + Content-Type** (Phase 0). *(Replace Data Value with Object — internal.)* — now genuinely SAFE.
- **Pure minors:** `MAX_BODY_SIZE` const dedup (server.rs:213/485); `bb_asset_name()` helper (versions.rs:121/331); shared `home_dir()` fixing `"."`/`"~"` divergence; drop redundant `write_pem` 2nd `0o600`; `default_config_version` → assoc const; copy-bb twin size-guard helper; collapse `VerifiedSite`/`VerifiedSitesEntry` + drop 3 dead fields.
- **PR boundary:** 2-3 tiny PRs. **Validation:** `cargo test`/`bun run test`/lint green. **Rollback:** revert PR.

### Phase 2 — Config mutation helper (Q9) [ASK: swallow]
`mutate_config(&ConfigState, impl FnOnce(&mut AcceleratorConfig)) -> Result<(),String>` for the 6 commands.rs sites + server.rs:382. **The respond_update_prompt:219 swallow is a real behavior decision** — see Ask A. *(Extract Function.)* Char test: a config mutator persists + an injected save-error path. **Rollback:** revert.

### Phase 3 — ServerStatus enum (Q10, coordinated server↔tray)
Char test (Phase 0) pins the animation-active condition. Replace `StatusCallback=Arc<dyn Fn(&str)>` → `Arc<dyn Fn(ServerStatus)>`; emit `ServerStatus::{Idle,Downloading,Proving,Error}` (server.rs:437/465/531); main.rs:356 matches variants (+ `display_text()`/`is_busy()`). One PR (both sides). *(Replace Primitive with Object.)* Risk: animation timing — the char test + a manual tray smoke guard it. **Rollback:** revert (single PR).

### Phase 4 — Value object `AztecVersion` (Q3 + 2 minors)
`AztecVersion{raw,tier,sort_key}` with validation-as-constructor; `Deref<str>`+`AsRef<str>`. Thread `&AztecVersion` through internal APIs; `&str` boundary stays at server ingress (resolve_version constructs once). Subsumes `versions_to_evict` re-parse + `bb_asset_name`. *(Introduce Value Object.)* Char tests already strong (eviction/tier/sort); add ctor-rejects-==-is_valid_version. **Rollback:** revert (touches ~5 call sites + versions.rs/bb.rs).

### Phase 5 — `download_bb` split (Q11)
After Q3. Char test (atomic-rename-cleanup) first. Extract `download_tarball` / `verify_digest` / `install_version_dir` (folds the 2 `remove_dir_all` arms) / `postprocess_unix`+`postprocess_macos`; orchestrator keeps guard+cache fast-path + the digest→extract ordering. *(Extract Method.)* **Rollback:** revert.

### Phase 6 — Crash-recovery trait (Q4) [SAFETY-CRITICAL]
`trait CrashRecovery{enable(&self); disable(&self)->bool}` + per-platform ZSTs; mac/linux `disable` returns `true` (behavior-neutral); Windows keeps the query-gated bool. Free fns → thin dispatch. **Preserve the #96/#97 disarm-before-install ordering byte-for-byte** (Phase 0 sequence test + the updater-smoke CI gate are the proof). *(Extract Interface.)* **Rollback:** revert; re-run updater-smoke.

### Phase 7 — Architectural splits [RISKY CORE PATH — rc dry-run after]
- **Q1 AppState** → `AppState{core:Arc<HeadlessState>, gui:Option<Arc<GuiCallbacks>>}`; handlers borrow `core`/`gui`; fixes main.rs clone-stutter. Keep headless `server/src/main.rs` green. *(Extract Class.)*
- **Q2 server.rs** → split `bind.rs`/`tls.rs`/`handlers/prove.rs`; extract `authorize_origin` + prove core. server.rs = thin router+start. *(Extract Module / Extract Method.)* Depends on Q1.
- **Q5 SDK** `#probeAndParseHealth` + `PhaseReporter` (public API unchanged). *(Extract Method / Extract Class.)*
- **Q6 UpdateCoordinator** + **Q7 SafariSupportManager** — preserve the safety-critical lifecycle order (Q7 consolidates the missing recovery path). *(Extract Class.)* [SAFETY-CRITICAL — rc dry-run.]
- **Q14** `is_auto_approved` reuse `canonicalize_origin` (tests guard the localhost set). *(Substitute Algorithm.)*
- Each its own PR; the server/updater/safari ones gate on an **rc dry-run** before the next stable cut.

### Phase 8 — SDK type break (Q5 + Q12, PAIRED) [BREAKING — lockstep playground]
**REFRAMED per consolidation Fact A (opus + codex both found it):** there is **no independent SDK semver to "major bump"** — `@alejoamiras/aztec-accelerator`'s published version is *derived from the Aztec `@aztec/stdlib` version* at publish (`scripts/get-sdk-publish-version.ts`, `_publish-sdk.yml`); `package.json` is a `0.0.0` placeholder; consumers install by Aztec version / dist-tag. So owner-decision-2 ("major bump") is **not literally executable** — see Ask A.
**Working resolution (pending Ask A):** ship Q12 as a *clean type-break* — discriminated unions for `AcceleratorStatus` + the phase event (mirroring Phase-0-pinned reachable states; HTTP wire contract UNCHANGED, so decoupled from Q8 + from any server release) — landing on the post-1.0.4 dev line, published at the next Aztec-version bump with a `MIGRATION.md` + README update. **SPLIT Q5 and Q12 (adopted from codex's final audit — overrides the earlier pair decision):** land **Q5** (the `#probeHealth`/`#parseHealthResponse`/`PhaseReporter` extraction) first under the *existing* types, then **Q12** as the isolated breaking PR — preserves rollback granularity + clean review. The Q12 PR migrates `packages/playground` (aztec.ts + ascii-animation.ts) **and** the aztec-accelerator skill in the same PR (monorepo typecheck is the free in-repo consumer), **and adds a publish-guard / freezes `_aztec-update.yml` for the window** (Ask E — the SDK auto-publishes on upstream Aztec bumps; a half-migrated break must not auto-ship). *(Extract Method; Replace Primitive with Object.)* **Rollback:** revert before publish; after publish, patch forward (never unpublish).

### Phase 9 — Minor sweep
Frontend popup scaffolding dedup + global-bridge module boundary; SDK `#fallbackToWasm` dedup; `open_or_focus_window(WindowConfig)`; inline `dirs_next`; certs test dedup w/ correct consts; copy-bb `PLATFORM_MATRIX` table (Q13) + `copyUnixBb`. One sweep PR (or 2).

---

## Security & Adversarial Considerations
- **Threat surface touched:** auth glue (authorize_origin, is_auto_approved — Q14 must not widen the auto-approve set), the cert/TLS path (Q7 — must keep fail-closed recovery), the prove wire contract (Q8 — must not weaken validation or leak in error bodies), the version→path sink (Q3 — AztecVersion ctor must enforce the #99 traversal guard identically), the crash-recovery/updater (Q4/Q6 — must not break disarm-before-install, a privilege/race boundary).
- **Least privilege / supply chain:** no CI-token or workflow changes in scope; the SDK major bump (Q12) publishes via the existing trusted-publisher + 7-day-min-age path (owner publishes; not in this plan). No new deps except possibly none.
- **Crypto:** untouched (rcgen/rustls/sha2 stay); Q7 only restructures call sites, not cert generation.
- **Input validation:** Q3 centralizes (strengthens) version validation; Q8/Q14 must preserve exact validation + canonicalization behavior (char tests are the guard).
- **Adversarial review ask for auditors:** could any "behavior-preserving" refactor silently (a) widen auto-approval, (b) weaken version-path validation, (c) change the error wire shape the SDK trusts, (d) break the disarm-before-install race protection, or (e) serve an untrusted cert? Attack each.

## Assumptions
### Facts (verified — research/*.md, file:line)
- Error JSON `{error,message}` is parsed by the SDK (accelerator-prover.ts:375-378); 403→WASM fallback. Header names `x-aztec-version`/`x-prove-duration-ms` are wire contract.
- AppState = 7 Option fields (server.rs:32-43); headless `server/src/main.rs` reuses the router with None callbacks.
- `download_bb` order cache→digest→extract; #99 traversal guard in `is_valid_version`.
- `respond_update_prompt:219` swallows the save error; 5 sibling sites propagate.
- Disarm-before-install ordering is safety-critical (#96/#97), only exercised by the updater-smoke CI gate.
- Q12 in-repo consumers: `packages/playground/{aztec.ts,ascii-animation.ts}`.

### Inferences (post-final-audit — corrected)
- ~~Q8 content-type is invisible~~ **FALSE (codex):** `axum::Json` flips Content-Type → changes `ky`'s `HTTPError.data`. **Corrected:** Q8 keeps `(StatusCode,String)`/`text/plain`; the DTO is internal. Phase-0 test asserts Content-Type too.
- mac/linux `disable()→bool` always-true is behavior-neutral — holds, but see next.
- ~~rc dry-run + updater-smoke is sufficient proof for the safety-critical refactors~~ **FALSE on macOS (opus):** disarm-before-install is `#[cfg(windows)]`; the macOS dev box + macOS rc exercise a no-op `disable()`. **Corrected:** the Q4/Q6/Q7 proof gate is concretely the **green Windows `_e2e-updater-windows.yml` run on the rc**; the macOS rc proves only the wire contract. The macOS `codesign`-cleanup arm (versions.rs:398-416) is **un-pinnable** by fake-bb → Q11 leaves it gated/manual + documents the hole.
- Q12 is TS-only at the wire level, BUT **the SDK auto-publishes** (`_aztec-update.yml` + `get-sdk-publish-version.ts`) on upstream Aztec bumps — so a half-migrated break can auto-ship (see Ask E).

### Asks (surface to owner — BLOCKING at the gate)
- **Ask A — RESOLVED (owner): standalone SDK semver + compatibility table.** The root defect: today the SDK version = the Aztec version, so the SDK cannot signal its OWN breaking changes (Q12 has no version to announce with). **Resolution:** a separate **"SDK versioning redesign" prerequisite project** (its own `/blueprint light`) that (1) gives `@alejoamiras/aztec-accelerator` a standalone semver line, (2) adds a `peerDependencies` range on `@aztec/*` so npm enforces compatibility at install, (3) maintains a generated SDK↔Aztec compatibility table in the README + release notes. **This prerequisite GATES Phase 8/Q12** — Q12 ships as a clean SDK major once standalone semver exists. (Replaces the earlier "ship on dev line" working assumption.)
- **Ask E [new, opus — auto-publish race] — guard the Q12 window.** `_aztec-update.yml` auto-publishes the SDK on upstream Aztec releases with no human in the loop. If upstream Aztec ships between the Q12 merge and the playground+skill migration completing, the broken-typed SDK auto-ships. Mitigation: land the Q12 PR *with* the skill-doc migration + a publish-guard, **or freeze `_aztec-update.yml` for the Q12 window.** Confirm the mitigation.
- **Ask F [new, opus — scope cut] — cut Q6 (and maybe Q13)?** Q6 (UpdateCoordinator) is the only moderate-confidence single-model architectural finding, it's safety-critical, sits on the least-testable path (Windows-only disarm), and its payoff is speculative (skip-version/progress). Cut it for the best risk/value ratio? Q13 (copy-bb matrix) is cosmetic build-script churn — fold or drop?
- **Ask B — the 3 "refactors" that are secretly behavior changes.** Q9 (respond_update_prompt save-swallow), Q14 (auto-approve parser unification — changes `http://LOCALHOST` handling), Q7 (Safari settings-path missing the startup recovery). The plan **preserves current behavior** and splits each *actual fix* into a labeled opt-in PR. Do you also want the fixes shipped (behavior changes the auto-updater pushes to users), or strict behavior-preservation (keep the quirks)?
- **Ask C — minors:** absorb into their owning PRs (Q3/Q5/Q1/Q4/Q7 — codex's recommendation, less conflict) vs one separate cosmetic sweep?
- **Ask D — release cadence:** batch the safety-critical refactors (Q4/Q6/Q7 + the Q1/Q2 splits) behind ONE rc dry-run, or one rc per risky PR?

---

## Decision ledger (3 parallel plans → consolidated)
- **Source plans:** main (this doc), codex (`research/_codex-plan-draft.md`), opus (transcript). Strong convergence on phasing + dependency edges.
- **Adopted from opus:** **Fact A** (no SDK semver → Ask A); the **Q1 granularity** invariant (auth_manager on base, show_auth_popup on gui — preserves the headless-with-auth deny case server.rs:1128); **Q8 needs no coordinated release** (SDK reads error body for logging only); **Q14 isn't strictly behavior-preserving** (`http://LOCALHOST` lowercasing) → labeled opt-in.
- **Adopted from codex:** the **fake-`bb` (BB_BINARY_PATH) + tokio::time::pause** harness; **absorb minors into owning PRs** (→ Ask C); **two helpers** for Q9 (`mutate_config` propagates / `mutate_config_best_effort` logs) making the swallow a named choice; **Q8 before Q2+Q12** to stabilize the wire protocol; **negative tests first** for Q14 (path/query/userinfo/IPv6/extension/localhost-alias).
- **Adopted across all three:** char-harness first; cheap wins → value objects → structural → coordinators → splits LAST; Q1≺Q2, Q3≺Q11, Q10≺splits, Q4≺Q6, Q5+Q12 paired.
- **Disputed (for the audits to resolve):** **Q3 vs Q8 order** — opus: Q3 first (resolve_version is a shared site, avoids a re-touch); codex/main: Q8 first (it's the cheapest/safest win). Both touch `resolve_version`; the re-touch is small. *Leaning Q8-first (cheap-wins principle); flagged for the final audit.* **Q14**: codex (do-with-negative-tests) vs opus (defer) → adopt do-it-with-the-guard, but the LOCALHOST test is the decider (red ⇒ defer).

## Audit verdicts
- **Three parallel drafts (main + codex `b1nuxfku8` + opus `a6c8d690`):** complete; consolidated above with ledger.
- **Final adversarial audit:** **codex `reject` (blocking)** + **opus `conditional approve`** — both transcripts in `audit-codex.md` / `audit-opus.md`. **All blocking findings ADOPTED below** (the gate's high/critical-addressed requirement):
  1. **[codex critical] `/prove` ordering** mis-stated → **FIXED** in Phase 0 (real order auth→body→semaphore→resolve; pin semaphore-before-resolve to block the `download_bb` `.tmp` race).
  2. **[codex high] Q8 Content-Type** → **FIXED** in Phase 1 (keep `text/plain`; DTO internal-only).
  3. **[codex high] Q10 tray timing** → **FIXED** in Phase 0 (pin the exact `Proving`→`Downloading`→`Proving`→`Idle` sequence + strings; the enum's `is_busy()`/`display_text()` reproduce them byte-identically).
  4. **[codex high] Ask A overstated** → **FIXED** (revision semver exists; constraint = `@aztec/stdlib` base).
  5. **[opus high] Q14 trailing-dot/IDNA widening** → **ADOPTED**: add `http://localhost.`, `http://LOCALHOST`, IDNA/punycode to the Q14 negative set; **red-or-defer**.
  6. **[opus high] Q4/Q6 hollow on macOS** → **ADOPTED** (Inferences corrected): the safety-critical proof gate is the **Windows `_e2e-updater-windows.yml` rc run**; macOS codesign arm stays gated + documented.
  7. **[both med] Q8/Q10/Q3 ≺ Q2 are HARD prerequisites** (seam-stabilizers), not soft cheap-wins → **ADOPTED** (re-tag in the dependency graph). Q3-vs-Q8 dispute = noise; **Q8 first**.
  8. **[codex med] Split Q5 and Q12** (rollback granularity) instead of pairing → **ADOPTED** (overrides the earlier pair decision): land Q5 under existing types, then Q12 as the isolated breaking PR.
  9. **[opus med] Auto-publish race** → **Ask E**. **[opus low] Cut Q6/Q13** → **Ask F**.
- **Approval gate:** all blocking findings addressed above; remaining open items are owner decisions **Asks A-F**.

## Seeds
**/goal (recommended — long multi-PR program, survives `--resume`):**
```
/goal All phases ✓ in implementations-plan/quality-refactor-2026-06-05/plan.md (each phase: characterization tests landed FIRST then the named refactor, shipped as a small behavior-preserving PR merged green; disguised-behavior fixes Q9/Q14/Q7 only if Ask B approved; Q14 gated on the trailing-dot/IDNA negatives; Q5 then Q12 split per Ask A/E with playground+skill migrated + publish-guard); cargo test --lib + bun run test + bun run lint + bun run lint:actions exit 0 throughout; Q1/Q2 splits + Q4/Q6/Q7 validated by the WINDOWS _e2e-updater-windows.yml rc run (not just macOS); per phase LESSONS_FILE printed; /code-review max --fix applied + committed; codex post-impl audit clean (or high/critical addressed). NEVER merge to main without green CI; NEVER cut a stable release autonomously.
```
**/loop (fallback — per-session):** see `eli5.html`.
