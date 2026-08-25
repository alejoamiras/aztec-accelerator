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
