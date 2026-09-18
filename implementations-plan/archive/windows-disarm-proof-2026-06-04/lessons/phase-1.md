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

## Codex post-impl (High) → reworked to a decoupled poller
The first cut coupled /Query sampling into the /health loop. Codex post-impl (verdict High, would not
trust as a blocking gate) flagged three real issues; all fixed:
1. **Cadence not really 500ms** — Invoke-RestMethod (≤2-3s) blocked each tick, so a /health stall
   spread samples and a fast install could be missed (false-RED/wedge); the "300s" comment was wrong
   (worst case ~25min). → FIX: a **decoupled `Start-Job` background poller** samples /Query every
   ~200ms independently; the /health wait loop (150×2s=300s) no longer gates sampling.
2. **False-GREEN on a transient schtasks error** — `$sawAbsent` flipped on ANY non-zero exit. → FIX:
   the poller tracks the longest run of CONSECUTIVE absent samples after first-present; assert
   `maxAbsentStreak >= 3` (~≥600ms) — a one-off blip is streak 1 (filtered); the real install-window
   absence is seconds (streak 10-40+).
3. **Hollow arming proof** — no pre-run cleanup, so a stale task could satisfy it. → FIX: delete any
   stale task before launch (the arming block); the poller's `sawPresent` only counts a real
   registration by THIS run's N-1, and the absent-streak is only counted AFTER first-present (so the
   post-cleanup pre-registration gap isn't miscounted as the disarm).

Mechanics: poller writes "`<sawPresent> <maxStreak>`" to `$Work/task-state.txt` each tick (file IPC);
parent Stop-Job → reads it → asserts sawPresent==1 AND maxStreak>=3, then the durable re-arm /Query.
Job is stopped in the success path and in Cleanup (`finally`) so it can't leak. The `>=3` threshold is
the one rc-tunable knob if a future install is unusually fast (the window is install-bound, seconds).

## Codex re-review (still-High) → 3 more fixes
1. **Poller started AFTER launch** → Start-Job latency could miss first-present + the absent window
   (false-RED). FIX: start the poller BEFORE Start-Process (in the positive arming block). The
   pre-registration absence isn't miscounted — the `sawPresent` gate only counts the streak after the
   task was first seen present.
2. **Stop-Job→Get-Content torn read** (fail-closed "0 0" flake). FIX: dropped file IPC; the job emits
   "<sawPresent> <maxStreak>" to its output stream; parent reads the LAST line via Receive-Job.
3. **Pre-clean delete unverified** (a surviving stale task could satisfy arming). FIX: /Query after
   /Delete; fail closed if the task survived. Combined with the verified-gone state + poller-before-
   launch, `sawPresent==1` now provably means THIS run's N-1 registered the task.
Conscious residual (NOT a defect): the `maxAbsentStreak >= 3` threshold isn't airtight against a 3+
consecutive transient schtasks failure, but raising it risks false-RED on a fast install. For a
BLOCKING gate, not-wedging is the priority + a 3-failure burst (while the app's own schtasks calls
succeed) is implausible. The rc reveals the real streak (expected 10-40) → tune the floor up then.

## Codex final confirm (minor — no new defects) + last touch
Fix 2 (Receive-Job) confirmed clean. Fixes 1+3 "improve but not fully airtight":
- #1 (poller-before-launch): added a best-effort BARRIER — after Start-Job, peek `Receive-Job -Keep`
  until the poller has emitted ≥1 sample (i.e. is actually sampling) before launching. -Keep is
  non-destructive so the final read still sees every sample. (The 5s disarm margin already covered it;
  this makes "before launch" meaningful rather than racy.)
- #3 (pre-clean verify treats any non-zero /Query as gone): ACCEPTED as-is — ephemeral CI runners have
  no stale task, and a false-green needs BOTH a silently-failed /Delete AND a /Query error at once;
  the exit-0-only-fails-closed pattern matches the Rust convention (crash_recovery.rs). Documented, not
  closed. Verdict was minor → shipping; the rc is the real integration proof.

## Phase 2 — rc.4 dry-run: GREEN (validated)
Owner-dispatched 1.0.4-rc.4 (run 26976538825). Positive smoke step log:
- "armed crash-recovery (stale task cleared + verified gone; autostart Run key set)"
- "(#97) armed + disarmed — task seen present, then sustained-absent (9 samples) across the install"
- "(#97) re-armed — full armed→disarm→re-arm cycle proven"
Both legs SUCCESS (positive + negative); Create Git Tag + Create GitHub Release SUCCESS; run
conclusion success. The state-proof works end-to-end: the disarm was PHYSICALLY observed (not inferred
from a log), and the blocking gate did not wedge.

**The real streak = 9 samples (~1.8s).** Decision: KEEP the `>=3` floor. 9 is only 3× the floor and the
install window varies by runner/bundle, so a faster install could approach a higher floor → false-RED
(wedge) risk. The transient-burst false-green that a higher floor would prevent (3+ consecutive schtasks
failures while the app's own schtasks calls succeed) is implausible. Not-wedge-first wins for a blocking
gate. #97 COMPLETE.
