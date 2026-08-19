# Phase 3 lessons — commit, PR, CI (2026-08-19, running log)

## PR event mystery (pre-CI)
- PR #467 opened as draft → NO workflows fired; close/reopen also produced nothing. Root cause:
  **`mergeable: CONFLICTING`** — #466 landed on main after our base and collided on
  `implementations-plan/index.md`; GitHub creates zero `pull_request` runs when it cannot build
  the merge ref, with no error surfaced anywhere. Lesson: a silent "no checks reported" on a
  fresh PR → check `gh pr view --json mergeable` FIRST, before suspecting Actions.
- Fix: rebase onto origin/main (one index.md append-collision, both entries kept), re-run local
  gate, `--force-with-lease`. Workflows fired normally after.

## CI round 1 (head `99cf55c`): 4 failures, all triaged
1. **SDK E2E ×2 + Local Network E2E** — `Error: Cannot find module 'bcrypto.node'` at
   `aztec start`. The Phase-2 watch-item materialized: npm 12's allowScripts default blocked
   bcrypto's `node-gyp rebuild` (no prebuilds). The snappy probe passed (snappy ships prebuilt
   napi bindings) — probe coverage was too narrow to catch this class.
   Fix (`57d3fd7`): reviewed six-package allowlist (bcrypto, leveldown, lmdb, msgpackr-extract,
   protobufjs, unrs-resolver — the complete blocked set; npm<12 ran ALL scripts, so strictly
   narrower). Mechanism: **user-level .npmrc lines** — npm 12 rejects `--allow-scripts`/env on
   project-scoped installs (`EALLOWSCRIPTS`); ephemeral runner makes $HOME/.npmrc job-scoped.
   Validated locally: zero residual blocked scripts, bcrypto.node builds + loads, snappy clean.
   **Cache salt bumped a second time** — because the install recipe changed again (the salt
   comment's rule). Correction from the codex post-impl audit: the original rationale ("failed
   runs cached the broken tree") was wrong — actions/cache's post-save skips on failed jobs, so
   round 1 likely cached nothing; the bump is still correct, for the recipe-change reason.
2. **Tarball Consumer** — "exact-host resolved 10 copies of @aztec/stdlib". The harness
   hardcoded the exact host at `5.0.1` (script postdates the updater's file list), so the bump
   manufactured the skew the singleton gate exists to catch. Fix (`57d3fd7`): derive the host
   pin from `packages/sdk/package.json`. Validated locally with a real packed tarball: exact
   host 1 (singleton), conflict host 10 (informational, as designed).
3. **Mocked E2E** — exit 124 inside the `playwright-cache` composite action: browser-download
   timeout (bun.lock change → new cache key → cold download). Infra flake, no code change; the
   round-2 push re-runs it. If it repeats, inspect the cache action/warm workflow.

## Wins in round 1 (unchanged by the fixes)
- Windows Prebuild Smoke + Windows Build Smoke PASS → the two-channel bb pin is correct.
- WebDriver ×4, Rust, Cert Trust, lint, typecheck, unit — all green. Zero source-code fallout.

## Round 2 (head `57d3fd7`): all round-1 failures FIXED
- SDK E2E ×2 SUCCESS (bcrypto allowlist works — sandbox boots on 5.2.0, accelerated proving path
  green against it), Local Network E2E SUCCESS, Tarball Consumer SUCCESS (exact host singleton),
  Mocked E2E SUCCESS.
- Two fresh infra flakes: Production Build Smoke (same playwright-cache exit-124 download timeout
  that hit Mocked E2E in round 1 — cold Playwright cache key after the bun.lock change) and a
  CANCELLED Linux WebDriver leg (spot-eviction shape, all siblings green). `gh run rerun --failed`
  cleared both.
- **FINAL: SDK Status, App Status, Accelerator Status, Actionlint Status (+Landing) all SUCCESS.**
  Phase 3 gate PASSED.

LESSONS_FILE=implementations-plan/aztec-5.2.0-2026-08-18/lessons/phase-3.md

LESSONS_FILE=implementations-plan/aztec-5.2.0-2026-08-18/lessons/phase-3.md
