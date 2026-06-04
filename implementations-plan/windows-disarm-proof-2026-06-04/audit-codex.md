# Codex audit (xhigh, session 019e93b2) — #97 plan — verdict: needs-rework

## The reframe (most important)
Codex argues for **state-proof over log-proof**, which dissolves most of the plan's complexity:

> "If you want state-proof rather than log-proof, a background `schtasks /Query` poller that records
> 'task absent at least once during install' is stronger and removes the prod-log dependency."

This is **better** and I'm adopting it (pending owner re-confirm of decision #3):
- The disarm removes the task, THEN the NSIS install runs (seconds), THEN re-arm (updater.rs:86-110).
  So the task is **absent for the whole multi-second install window** — NOT "too brief" (my original
  dismissal was wrong). A ~200-300ms background `/Query` poller reliably samples that absence.
- Observing the task **actually absent** proves the disarm's *effect* directly — no trust in an
  instrumentation string, and it catches the regression log-proof misses (codex Medium: a stub that
  returns `true` + emits the exact log line without deleting would pass log-proof).
- **No prod-code change** → the correctness-critical `disable_crash_recovery()` stays untouched
  (eliminates codex's Critical + the whole stdout-vs-file fight).
- The 5s initial update delay (main.rs:432) guarantees the poller is up + has confirmed the task
  PRESENT before the disarm fires → clean present→absent→present ordering, no race.

## Findings (all dissolved by going state-proof)
- **Critical — up-front /Query must not short-circuit deletion.** My draft's "cleanly absent → return
  true without /Delete" is a NEW false-success path: a lying pre-/Query → skip delete → updater installs
  with the task still armed. Today the loop ALWAYS deletes. (Moot under state-proof: no Rust change.)
- **High — "read newest log file" is brittle for a blocking gate.** Daily UTC rotation (main.rs:185,
  lib.rs:18): a run crossing UTC midnight splits arm/disarm/re-arm across files → false-red. (Moot: no
  log read.)
- **Medium — "only residual is a lying /Query" overstated** for log-proof: trusts an instrumentation
  string; a regression emitting the line without deleting passes. (State-proof removes this.)
- **Low — rc-only validation:** the Windows unit test IS reachable in PR CI — `windows-build` runs
  `cargo test` on windows-latest (accelerator.yml:295). But end-to-end the smoke needs the rc.

## Codex vs opus on stdout-vs-file (now moot)
- Opus: assert on **stdout** (LineWriter, flushed per line; non-blocking file loses the tail on
  `app.restart()`→`std::process::exit`).
- Codex: the persisted **file** is better than stdout — the script doesn't capture app stdout today
  (ps1:177), and `app.restart()` (new process) makes single-process stdout capture "the wrong primitive."
- **Resolution:** state-proof reads NEITHER — it polls task state. Both concerns evaporate.

## What's fine (per codex)
- "A permanent `false` from `disable_crash_recovery()` already fails the smoke" is **correct** — the
  updater aborts before install (updater.rs:92) and the background updater only retries after 12h
  following the initial 5s check (main.rs:432). So the only gap is "returns true without removing,"
  which state-proof catches by observing real absence.
