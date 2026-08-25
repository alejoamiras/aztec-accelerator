# Phase 1 lessons — 1.3-compatible wave (2026-08-25)

**Outcome: ✓ GREEN, linker RETAINED (gate outcome i). Commits `65fa9a7` · `3d60271` · `c80cb41` ·
`f4680a5`. Gate binary: bun 1.3.14 (scratch-pinned; `bun --version` = 1.3.14 recorded per run).**

- **Publish pin** (`65fa9a7`): `_publish-sdk.yml` off floating `latest` — first, separable.
- **Centralization** (`3d60271`): `.bun-version` + `bun-version-file` at all 23 sites (verified 23
  converted, zero inline pins remain); `scripts/bun-pin.test.ts` guard 2/2.
- **Prophylaxis** (`c80cb41`): `JEST_WORKER_ID ??= "1"` in the three preloads;
  `scripts/aztec-logger-contract.test.ts` verifies the actual branch structure (consequent reaches
  `pino.destination`, transport stays outside it), resolved via `Bun.resolveSync` from
  packages/sdk (isolated-linker-proof). Consumer inventory (installed deps with JEST_WORKER_ID
  readers): `@aztec` (the intended consumer); `undici` (`client.js:492` — eagerly requires the
  bundled llhttp-WASM variant; benign, and bun's fetch doesn't route through undici);
  `playwright`/`webdriver` (their own processes — bun-test preloads never apply); `cheerio` (no
  dist-reachable use found). In-tree causality re-verified before this phase: playground under
  scratch 1.4.0 crashes bare, passes 8/1-skip/0 with the env.
- **Isolated linker** (`f4680a5`) — evidence (full log in the validation task output):
  - 4a: `bun.lock` byte-identical after the flip. PASS.
  - 4f timings: hoisted fresh install 1.12s → isolated 0.27s (fresh AND warm store) — **~4×**;
    disk NEUTRAL (1.3G both — the store is per-project under `node_modules/.bun`, as the
    corrected F8 framing predicted; no cross-worktree disk win, the win is install time).
  - 4c (the real hoist risk): packed SDK → `.github/scripts/packaged-e2e-swap-sdk.sh` swap →
    `createRequire` probe anchored INSIDE the materialized dir: `@alejoamiras/aztec-accelerator`,
    `@aztec/bb-prover/client/lazy`, `@aztec/stdlib/kernel`, `@aztec/simulator/client`, `ky` ALL
    resolve. PASS — the swap-sdk comment's hoist dependency is not load-bearing under isolated.
  - 4d: two concurrent scratch-worktree installs, both exit 0. PASS.
  - 4e: NO root-hoisted vite under isolated → `packages/playground/tsconfig.e2e.json`'s override
    is unnecessary (Arc C cleanup candidate; untouched in this arc).
  - Layout note: `node_modules/.bun` exists; top-level entries materialize as real dirs (not
    per-package symlinks at the root level) — resolution semantics, not layout cosmetics, were
    the acceptance test.
- **Gate**: `bun run test` exit 0 (76 tests / 9 files incl. both new guards) && `lint:actions`
  exit 0 && guard tests 4/4.

LESSONS_FILE=implementations-plan/bun-1-4-migration/lessons/phase-1.md
