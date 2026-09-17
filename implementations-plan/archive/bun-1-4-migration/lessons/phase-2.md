# Phase 2 lessons (started early — upstream-issue task, 2026-08-25)

## The upstream issue: already filed AND fixed (A3 closed with nothing to file)
Owner's approval condition was "create the issue (check if it's already created)". Checked:
**oven-sh/bun#40268** — "node:worker_threads: new Worker() reads the mutable global MessagePort,
so replacing it (happy-dom) throws `port.on is not a function` on 1.4.0" — CLOSED/COMPLETED
2026-08-24, **fixed by PR #40271** (commit `b746c078`): worker_threads now takes
MessagePort/MessageChannel/BroadcastChannel/Worker from intrinsics, not mutable globals. Fix ships
in the next bun release (1.4.0 is still latest today).

## Diagnosis correction (recorded against recon/plan framing)
Reduction matrix under scratch 1.4.0 (all in lessons-grade detail):
- plain `new Worker` under bun test: PASS
- SharedArrayBuffer workerData (thread-stream's shape): PASS
- pino single/multi-target/custom-levels transports (pino 9.14+ts 3.1.0/3.2.0, pino 10+ts 4.2.0):
  ALL PASS in isolation
- happy-dom(20.8.4) + plain Worker in a scratch project: PASS (registrator alone didn't trip it
  there)
- OUR playground (happydom preload) + aztec logger: CRASH; same suite with `JEST_WORKER_ID=1`:
  **8 pass / 1 skip / 0 fail** — workaround verified in-tree.
Upstream's dependency-free repro (replace `globalThis.MessagePort` with an EventTarget subclass →
`new Worker()` throws from bun internals) identifies the REAL trigger: the global replacement, not
pino/thread-stream — pino was simply the first Worker constructor to run after happy-dom's
registration in the playground. Consequences:
- The SDK suites (no happy-dom) were never exposed to THIS bug — recon agent C's "sdk unit would
  crash" was an import-trace inference, now corrected by evidence (the 1.4 gate run had sdk green,
  playground crashing).
- Phase-2.2's bb.js-leg NO-GO tail risk is much smaller than planned: the e2e context has no
  happy-dom, and plain Worker passes under 1.4.0. The spike still runs — evidence over inference.
- The JEST_WORKER_ID prophylaxis remains correct for the 1.4.0 window (proven above) and turns
  belt-only once the fix ships; A4 sunset now has a concrete trigger (bun release containing
  `b746c078`).
- Phase 3 gains an option: target the fix-carrying release (1.4.1+) directly if it ships before
  the bump lands — decided at Phase 3 with the ledger updated.

LESSONS_FILE=implementations-plan/bun-1-4-migration/lessons/phase-2.md

## Spike results (2026-08-25) — VERDICT: GO
Binary: scratch 1.4.0 (`bun --version` = 1.4.0 recorded per run).
1. **Fresh-install gate**: pristine worktree + `bun install --frozen-lockfile` (lockfile
   untouched) + FULL `bun run test` → **exit 0** (76 tests/9 files) + `lint:actions` exit 0.
   THE GATE EARNED ITS KEEP: first pristine run failed TS2688 — `@types/node` was an UNDECLARED
   transitive consumed via hoisting; 1.4's isolated linker (correctly, stricter than 1.3.14's)
   stops exposing it at root, and the migration worktree had masked it via tsc typeRoots walking
   up to the root clone's hoisted node_modules. Fix: declared `@types/node@20.19.37` (the exact
   version the hoisted root had been exposing; already in the lock — no new resolution, min-age
   moot). Verified in the failing scenario: fresh worktree typecheck exit 0.
2. **bb.js Worker (decisive)**: SDK e2e under 1.4.0 vs live testnet + headless accelerator —
   **10/10 pass** including proving.test.ts's WASM fallback leg (constructs bb.js's node
   `worker_threads.Worker`). The #40268 bug does not bite this pattern (no happy-dom in that
   context; plain-Worker repro also passes). **GO.**
3. **TLS**: openssl IP-SAN self-signed + `Bun.serve({tls})` + `fetch https://127.0.0.1` with ca →
   200 ✓. The accelerator-leaf semantics survive 1.4's tightening.
4. **--parallel three-way**: baseline / `--parallel` / `--parallel --no-isolate` per suite —
   pass/fail counts IDENTICAL in all modes (playground incl. a canary asserting preload effects
   per worker: addEqualityTesters patched, JEST_WORKER_ID present, document registered). The
   apparent baseline "fails" (sdk 3, accelerator 5) were spike-harness artifacts: bare `bun test`
   swept sdk's e2e/ (fail-fast guard without services — the real e2e is 10/10 with services) and
   accelerator's @playwright/test specs (excluded by its real `test:unit`). Both modes adoptable;
   Phase 4 will use `--parallel` (implied isolate proved equivalent here, but `--no-isolate` is
   the conservative choice consistent with preload-time global mutation — decide per suite at
   adoption with one more per-suite confirmation).
5. **Upstream**: nothing to file — oven-sh/bun#40268 already fixed by #40271 (see earlier
   section).

LESSONS_FILE=implementations-plan/bun-1-4-migration/lessons/phase-2.md
