# Phase 4 lessons — post-bump tooling adoptions (2026-08-25, Arc C branch `bun14-arc-c`)

**Commit `ec44b66`. Binary for all measurements: scratch bun 1.4.0.**

## Measurements → adoption decisions (evidence over recon estimates)
- **sdk `bun test src/`**: baseline 14675/14666ms → `--parallel` 8488/8470ms → `--no-isolate`
  8477/8478ms (two runs each; pass counts byte-identical). **ADOPTED bare `--parallel`** (42%;
  implied isolate proved equivalent in the spike canary and here — simpler flag wins).
- **playground/accelerator/root-scripts suites**: sub-second — worker spawn overhead exceeds any
  gain. NOT adopted.
- **root `test:unit` chain**: sequential 19299ms vs workspace-parallel 14718ms — but with sdk now
  internally parallel the sequential chain drops to ~13s anyway, and the parallel form needs
  alias-script surface for root `test:scripts`. NOT adopted (proportionality).
- **root `lint` chain**: 520ms TOTAL (biome via bun run; parts: pkg 59ms, shell 228ms, rust
  122ms). Recon's "biome is the long pole / real CI win" did not survive measurement — the chain
  is spawn-dominated. Chain parallelization CUT.
- **root `test:typecheck` chain**: 8593ms sequential; parallelizing needs 4 alias scripts and
  carries the CI CPU-contention caveat for a local-only ~4s win. NOT adopted.

## Adopted set
1. sdk `test:unit` → `bun test --parallel src/`.
2. `{ retry: 1 }` on the SIX pure-connectivity live-network tests (sdk e2e connectivity ×2,
   remote-network ×3, playground live-node probe ×1) — same flake class Playwright projects
   already budget retries for. The two PROVING tests carry NO retry (codex Arc-C blocker,
   adopted): their phase-trail asserts are the silent-fallback discriminator, and a retry could
   mask an intermittent path-selection regression — exactly the bug class the discriminator
   exists to catch. (Initial draft had eight placements and mis-counted them as seven; both
   corrected.) NOT touched: `release-contract.test.ts` (unit-tests retry logic itself).
3. `serve` devDep DELETED → `packages/accelerator/scripts/serve-static.ts` (Bun.serve dir routes,
   loopback-only) + `serve-static.test.ts` contract test (5/5 first run: index, Content-Type,
   ETag/304, raw-socket traversal rejection — fetch normalizes `../` so the test speaks raw HTTP
   via Bun.connect — loopback binding). Desktop-UI Playwright webServer swapped.
4. `--no-orphans` on both Playwright webServer commands.

## Surprise finding — 4e inverted
Removing `tsconfig.e2e.json`'s vite `paths` override (Phase-1 4e had inferred "unnecessary")
BROKE the e2e typecheck: under the isolated linker, vite's plugin packages resolve their `vite`
peer from the store (`node_modules/.bun/vite@7.3.1…`) while the graph's own imports resolve the
playground's package-level copy — same version, two file trees, tsc sees two declarations and
their private `_pluginContextMap` conflicts. Override RETAINED with its comment rewritten to the
store-vs-copy mechanism. Lesson: 4e's check ("no root-hoisted vite") tested the OLD failure mode,
not the new layout's.

## Gate (fast layers)
`bun run test` exit 0 (76 tests / 9 files, 874 asserts — includes the new contract test) +
`lint:actions` exit 0. Biome round-trips: my hand-formatted retry-object edits needed
`biome check --write` on 4 files + import-order on the new test (fixed; the pre-push gate catches
this class, but format-on-save discipline would have avoided the loop).

Heavy layers (packaged-e2e leg + live-testnet smoke) run at Arc C PR time per the plan gate.

LESSONS_FILE=implementations-plan/bun-1-4-migration/lessons/phase-4.md
