# Planning brief — quality-fixes-2026-06-08 (blueprint deep)

**Goal:** implement all 9 findings from the `/harden quality ultra` audit (run `2026-06-08-ultra-e094d8`) as a set of behavior-preserving refactors **except F-02**, which deliberately closes a live gap. Add tests that *validate* each change and *improve* coverage at the new seams. Repo root: `/Users/alejoamiras/Projects/aztec-accelerator`.

**Full finding detail:** read `audit/quality/2026-06-08-ultra-e094d8/report.md` (+ `findings/verified.md` for the verifier's confidence/corrections). Summary of the 9:

| ID | Smell | Fix | Effort |
|----|-------|-----|--------|
| F-01 | nullable `AppState`/`HeadlessState` bag, hand-built ×3 | Extract Factory `headless()`/`desktop()`; make always-required deps non-`Option` | days |
| F-02 | canonical origin as raw `String` + headless ingress bypass | `CanonicalOrigin` newtype w/ `serde(try_from)`; route ALL ingress (incl. headless `ALLOWED_ORIGINS`) through it | days |
| F-03 | 203-line Tauri `.setup` god-closure (`src-tauri/src/main.rs:260-462`) | Extract Function into named bootstrap phases + callback builders | days |
| F-04 | `versions.rs` Large Class (~582 prod LOC, 18 fns, 8+ responsibilities) | Extract Module: `version_id`/`platform`/`artifact_layout`/`cache`/`downloader` | days |
| F-05 | SDK contract hand-copied across source/barrel/README/MIGRATION/skill, drifted | barrel = canonical; fix drift (flat-status README, missing `AcceleratorProtocol` export, `setForceLocal` doc); add export/doc-sync test | hours |
| F-06 | `AcceleratorProver` two HTTP stacks (`fetch` /health, `ky` /prove) | Extract Class `AcceleratorTransport` (URLs, protocol negotiation, cached status, one error model) | days |
| F-07 | cert path triplet Data Clump (all `&Path`, swap-silent) | Introduce Parameter Object `CertPaths { ca_cert, leaf_cert, leaf_key }` w/ `live()`/`staged()`/`swap_into()` | hours |
| F-08 | `/prove` status ownership split prove↔resolve_version | move status sequence into `prove()`; `resolve_version()` returns data only | hours |
| F-09 | HTTPS startup duplicated (`main.rs:72+86` ↔ `commands.rs:156+161`) | Extract Function shared `spawn_https()` | hours |

## Decisions already made (by the user — these are FIXED, not open)
1. **F-02 → close the gap, with tests.** The `CanonicalOrigin` newtype makes the canonical-origin invariant un-bypassable; the headless `server/src/main.rs:43-57` `ALLOWED_ORIGINS` path MUST be routed through it (behavior change: env origins now canonicalized). All *other* 8 findings stay strictly behavior-preserving.
2. **Test strategy — NO blanket characterization tests** (explicit user steer; this codebase already has ~90 Rust + ~96 TS unit tests + 9 WebDriver E2E exercising real startup/settings + Playwright). Regression safety = the **existing suite + WebDriver E2E + Rust compiler**. Write **new** tests ONLY for: new types/constructors (`CanonicalOrigin`, the `AppState`/`HeadlessState` factories, `CertPaths`, `AcceleratorTransport`), F-02's new canonicalization behavior, and the F-05 doc-sync test. Add a characterization test ONLY for a specific refactor that is both risky AND has zero existing coverage — and FLAG it for the user rather than doing it by default.
3. **Delivery = 4 package-coherent themed PRs** (one CI gate each):
   - **PR-1 Rust core invariants:** F-01 + F-02 (`accelerator.yml` gate)
   - **PR-2 Rust structural:** F-03 + F-04 (`accelerator.yml`)
   - **PR-3 Rust local:** F-07 + F-08 + F-09 (`accelerator.yml`)
   - **PR-4 SDK:** F-05 + F-06 (`sdk.yml`)
   (Refinement of the user's "themed PRs": F-05 moved into the SDK PR for CI-gate/test-runner coherence. User may veto at the gate.)
4. **Post-impl:** schedule **`/harden security`** at the end (F-02 touches the origin-auth trust boundary).

## Hard constraints
- **SDK public API: NO breaking changes.** The team just did the Q12 semver break — they are break-sensitive. F-06 is an INTERNAL extraction (same public methods/types). F-05 is ADDITIVE (export `AcceleratorProtocol` from the barrel; fix docs). If any change would alter the published type surface, it must be called out as an Ask.
- **`main` is branch-protected** → each PR via branch + `gh pr create` + auto-merge after green CI. Never push to main.
- **Each PR keeps the FULL existing suite + lint + its relevant E2E green** before merge ("make very sure they are valid"). `bun run test` + `bun run lint` + (for Rust) `cargo test`/`cargo clippy` + the WebDriver E2E gate.
- **Behavior-preserving** for all findings except F-02.
- **Don't break the release pipeline:** `versions.rs` (download/cache/evict), `certs.rs`, and the server startup are release-critical (consumed by `release-accelerator.yml`, the updater-smoke gates). F-03/F-04/F-07/F-09 must not change runtime behavior of those paths.

## Known sharp edges the plan MUST address (validity)
- **F-02 serde migration:** persisted `approved_origins` in existing user configs are ALREADY canonical strings (written by today's `config::load` migration). `CanonicalOrigin`'s `try_from` MUST accept them losslessly (idempotent canonicalization) so existing configs deserialize. Removing `migrate_approved_origins` must not strand old data.
- **F-02 e2e:** the e2e/WebDriver harness sets `ALLOWED_ORIGINS`; confirm those values are already canonical (so canonicalizing them is a no-op and doesn't break origin matching), or adjust the harness.
- **F-01 non-`Option`:** before making a field non-optional, verify NO runtime path depends on it being `None` (e.g. a "no config" headless mode). The headless binary sets `config: None` when `ALLOWED_ORIGINS` is unset — so `config` must STAY optional; only make truly-always-present deps (`prove_semaphore`, `app_version`) required.
- **F-08:** the status sequence is pinned by a characterization test at `core/src/server.rs:617-685` — keep it green (net emitted statuses identical).
- **F-04 module split:** `versions.rs` is `pub`-consumed by `bb.rs` + the server + src-tauri; keep the public fn paths stable (re-export from `versions` or update callers) so it stays a pure move.

## What each planner must produce
A full implementation plan: per-finding concrete approach (the exact new type/constructor/module shapes), the 4-PR phase structure with intra-PR step ordering, a per-finding **test plan** (which existing tests cover it + which new tests to add), a **validity argument** per finding (how we are SURE it's correct — compiler, existing E2E, new unit test), risk + rollback, a **Security & Adversarial Considerations** section (esp. F-02 origin canonicalization: punycode/IDN, case, trailing-dot, port, scheme — what an attacker could smuggle past exact-match approval), and an **Assumptions** section (Facts/Inferences/Asks). Be specific and attack your own plan.
