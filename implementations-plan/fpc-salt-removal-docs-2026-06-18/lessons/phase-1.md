# Phase 1 — Remove SPONSORED_FPC_SALT from code (2026-06-18)

Removed the env read in 3 places, all now hard-coded to the canonical salt=0:
- `playground/src/aztec.ts` initializeFPC — `new Fr(0)` (dropped `process.env.SPONSORED_FPC_SALT`).
- `playground/vite.config.ts` — dropped the 2 SPONSORED_FPC_SALT env-baking lines (loadEnv map + the `process.env` define).
- `sdk/e2e/proving.test.ts` — `{ salt: new Fr(0) }`.

True no-op: all three already defaulted to `new Fr(0)` when the env was unset (the secret was set to `0x0` anyway).

**Gate:** PASS — no `process.env.SPONSORED_FPC_SALT` readers left in src/e2e/vite.config; no aztec.ts tsc errors; `bun run --cwd packages/playground build` clean; `bun run lint` exit 0; `bun run test` exit 0.

LESSONS_FILE=implementations-plan/fpc-salt-removal-docs-2026-06-18/lessons/phase-1.md
