# Phase 3 lessons — bump-only wave + Arc A+B CI (2026-08-25)

**Outcome: ✓ GREEN — PR #475 all five Status checks SUCCESS on `eaf7a06`, the first fully green
fleet-wide bun 1.4.0 run. Bump commit `82ca06b` (lockfile untouched); @types/bun still inside
min-age until ~08-28 (trailing commit pending).**

## The isolated-linker undeclared-dependency family (five members, one lesson)
CI's clean topology exposed, one per round, every place hoisting had been silently satisfying an
undeclared dependency. Local repros only worked in ancestor-free worktrees (this session's
nested worktree walks up into the root clone's hoisted node_modules — the same masking, one
level removed):
1. `@types/node` (tsc `types` entry; caught by the spike's fresh-install gate) — declared at the
   hoisted version.
2. `msgpackr` (Vite dev resolves kv-store's `#msgpackr` imports-map TARGET from the app root) —
   declared in playground devDeps at the locked version.
3. Root `bunx playwright` FETCHED A REMOTE playwright (no root bin under isolated) — composite
   gained `package-dir`; every bunx runs from the consuming package (`c5562e9`). Symptom chain:
   wrong browser revision cached/installed vs the workspace's 1.58.2.
4. `vite-plugin-node-polyfills` shims injected into TRANSFORMED ../sdk sources resolve from the
   SDK's package — declared in sdk devDeps (`eaf7a06`). Blind alley worth remembering: aliasing
   the shims to absolute paths in dev SKIPS prebundling and the CJS shim dies in Vite's interop
   wrapper ("Cannot access '__vite__cjsImport0…' before initialization") — the build-only alias
   gate exists for exactly that reason; dev needs bare-specifier resolvability, not aliases.
5. (Phase-1's 4e inversion, same family: the vite `paths` override's justification changed from
   root-hoist dedup to store-vs-copy dedup.)
Diagnosis pattern that worked every time: reproduce on a scratch worktree OUTSIDE the clone
(`git worktree add $SCRATCH/...`), read the DEV-SERVER/WebServer log (not just build), then the
browser-side jsErrors (the mocked spec's safety net earned its keep).

## Also in this phase
- `.bun-version` → 1.4.0: zero lockfile churn (verified empty diff), bunfig comments re-stamped.
- `packaged-e2e-swap-sdk.sh` consumer-level swap validated by CI implicitly (App green includes
  the mocked/local-network legs that exercise playground resolution; the dedicated packaged leg
  runs at Arc C's gate).

LESSONS_FILE=implementations-plan/bun-1-4-migration/lessons/phase-3.md
