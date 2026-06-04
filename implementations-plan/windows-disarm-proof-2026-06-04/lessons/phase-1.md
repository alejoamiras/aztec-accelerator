# Phase 1 — state-proof full-cycle assertion (updater-smoke-windows.ps1)

## Design pivot (dual audit → owner)
Drafted as log-proof (tighten disable_crash_recovery's log + grep it). Both audits killed it:
- Codex (needs-rework): proposed STATE-proof — a background schtasks /Query poller asserting "task
  absent at least once during install". Stronger (observes the disarm's real effect, not a log string)
  + removes the prod-log dependency. My "absence too brief to observe" premise was WRONG: the task is
  gone for the ENTIRE NSIS install (disable → install → re-arm, updater.rs:86-110) = seconds.
- Opus (approve-with-changes): the log-proof file read could false-RED a blocking gate (non-blocking
  appender loses N-1's tail on app.restart()→std::process::exit). Moot under state-proof (no log read).
- Owner chose "A" (the watcher) after an ELI5.

## What shipped
Positive leg now proves the full cycle from OBSERVED TASK STATE (no Rust change, no log parsing):
1. ARMED: poll schtasks /Query until PRESENT before the update (bounded ~20s) — proves N-1 registered
   the task. Safe to run pre-disarm: first update check is 5s post-launch (main.rs:436).
2. DISARMED: tightened the /health poll to 500ms; each tick samples schtasks /Query FIRST (cheap, never
   blocks) and sets $sawAbsent on absence. On /health==N, assert $sawAbsent — the task was physically
   removed during the install. A non-disarming regression → never absent → FAIL (the end-state re-arm
   check alone would hide it — the exact #96 gap).
3. RE-ARMED: kept the #96 durable end-state check (task PRESENT after the update).
Negative leg untouched (tampered artifact rejected before install → no disarm → no absence assertion).

## Why 500ms sampling is reliable (not flaky)
Absence window = the NSIS install = seconds, and N-1's /health server stays up DURING the install
(separate axum task), so the loop keeps a tight cadence exactly when it matters. The slow /health
timeout only bites later (during the restart), after absence is already recorded. Sampling /Query
before /health each tick guarantees the sample isn't lost to a /health stall.

## Residual (documented, accepted)
A watcher can't prove ordering beyond present→absent→present; a pathological remove-then-instantly-
re-arm-before-install would read as a valid cycle — but that's not the regression class in scope (a
NON-disarming guard) and wouldn't prevent the race anyway. Out of scope.

## Validation
- bun run lint:actions exit 0 (workflow untouched). pwsh not installed locally → no PS lint; careful
  review + the rc are the gates.
- Phase 2 = owner-dispatched rc dry-run (the smoke runs only in the release pipeline since P5 dropped
  workflow_dispatch). SURFACE + STOP for that.
