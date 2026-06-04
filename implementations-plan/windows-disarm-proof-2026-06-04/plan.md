# Windows updater-smoke: prove the disarm-before-install actually ran (#97)

**Tier:** `/plan mid` (codex + opus dual audit — both done; verdicts + the design pivot below).
**Status:** ✅ approach locked (**state-proof**, owner chose "A") → implement → owner rc dry-run.

## North star
The Windows updater-smoke is a BLOCKING release gate. Today its crash-recovery check is
**end-state-only** (asserts the Task Scheduler task is PRESENT after the update) — so a regression that
never disarms would still pass green, shipping the half-written-binary install race. Make the smoke
**prove the disarm actually happened** by observing the task **physically absent during the install**,
so the guard can't silently rot.

## The design pivot (what the dual audit changed)
The draft was *log-proof* (tighten `disable_crash_recovery()`'s log + grep it). Both audits killed it
in favour of **state-proof**, and the owner chose it ("A — the watcher"):
- **Codex (needs-rework → proposed state-proof):** a background `schtasks /Query` poller that records
  "task absent at least once during install" is stronger and removes the prod-log dependency. My
  "absence is too brief to observe" premise was **wrong** — the task is gone for the **whole NSIS
  install (seconds)**: `disable_crash_recovery()` → `update.install()` → re-arm (updater.rs:86-110).
- **Opus (approve-with-changes):** the log-proof file read could false-RED a blocking gate (the
  non-blocking appender can lose N-1's tail on `app.restart()`→`std::process::exit`). State-proof reads
  no log, so this evaporates.
- **Net:** observe real task state, not a log string a regression could fake; **no change to the
  correctness-critical `disable_crash_recovery()`**; no log flush/rotation/capture fragility.

## Decisions (owner)
1. **Validation:** one owner-dispatched **rc dry-run** after merge (the smoke runs only in the release
   pipeline — P5 removed its `workflow_dispatch` trigger).
2. **Assertion strength:** prove the **full armed → disarm → re-arm cycle**.
3. **Mechanism:** **state-proof** (watch the task go absent), **not** the prod-log change. The prod fn
   is left untouched.

---

## Phase 1 — PowerShell: state-proof full-cycle assertion (positive leg)
**File:** `packages/accelerator/scripts/updater-smoke-windows.ps1` (positive leg, L193-221). No Rust.

Replace the end-state-only check with a three-part observed-state proof:
1. **Armed (pre-update):** after launching N-1, poll `schtasks /Query /TN "Aztec Accelerator Crash
   Recovery"` until PRESENT (bounded ~20s). Fail if never present → N-1's autostart Run key / startup
   `enable_crash_recovery()` regressed. (Run before the disarm can fire — safe because the first update
   check is 5s post-launch, main.rs:436.)
2. **Disarmed-during-install (the new teeth):** tighten the `/health` poll loop to ~500ms and, each
   tick, sample `schtasks /Query` *first* (cheap, never blocks); set `$sawAbsent` if it reports absent.
   On `/health == N`, assert `$sawAbsent` — the task was physically removed during the update. If it
   was armed the whole time, `disable_crash_recovery()` never ran → fail loudly. (~500ms sampling
   reliably catches a seconds-long, install-bound absence; N-1's `/health` server stays up *during* the
   install, so the cadence is tight when it matters; the slow `/health` timeout only bites later, during
   the restart, after absence is already recorded.)
3. **Re-armed (post-update, durable):** keep the existing #96 check — task PRESENT again after the
   update (the re-arm end-state).

Each failure `Write-Error`s a specific cause + `Dump-Logs` + `exit 1`. **Negative leg unchanged** (the
tampered artifact is rejected *before* install → no disarm → must NOT assert absence). The
`$Mode -eq "positive"` arming guard stays symmetric.

**Validation:** `bun run lint:actions` (workflow untouched but reachable); careful PS authoring (no repo
PowerShell linter); end-to-end only via the rc (Phase 2).

---

## Phase 2 — rc dry-run validation (owner-gated)
After Phase 1 merges, **surface + stop** for the owner to dispatch `1.0.4-rc.N`. Verify:
- `Updater Smoke (windows-x86_64 / positive)` **green** with the three signals exercised (the step log
  shows "arming confirmed" / "disarm confirmed — task observed absent" / "re-arm confirmed").
- `Updater Smoke (windows-x86_64 / negative)` still green (no absence assertion on that leg).
- tag + release still produced (the blocking gate didn't wedge).

No STABLE release. rc dry-run only, owner-dispatched.

---

## Assumptions

### Facts (verified against files)
- Disarm → install → re-arm order, install aborts on `!disable_crash_recovery()`: `updater.rs:86-110`.
- First update check is **5s** after launch, then every 12h: `main.rs:432-439`. → the watcher confirms
  PRESENT before any disarm can fire.
- `TASK_NAME = "Aztec Accelerator Crash Recovery"` (`crash_recovery.rs:200`); `schtasks /Query` exits
  non-zero when the task is absent (locale-independent — already relied on at `crash_recovery.rs:291`).
- The smoke runs only in the release pipeline (`workflow_dispatch` removed from
  `_e2e-updater-windows.yml` in P5) → integration validation needs an rc.
- Current positive leg is end-state-only (`updater-smoke-windows.ps1:193-221`); `Dump-Logs` already
  surfaces the app log dir for debugging (`:80`).

### Inferences (attack these)
- The absence window (disarm → re-arm) spans the entire NSIS install (seconds) → ~500ms sampling
  reliably observes it. *If wrong* (sub-500ms install): tighten the interval; the window is install-
  bound so this is conservative.
- N-1's `/health` server stays responsive during `update.install()` (separate axum task), so the
  combined loop keeps a tight `/Query` cadence during the absence. *If wrong:* sampling `/Query` first
  each tick still records absence before any `/health` stall.
- No prod-code change ⇒ zero risk to the disarm guard itself (only the test gets stricter).

### Asks (resolved)
- Owner chose state-proof ("A"), full cycle, one rc validation. No open asks.
- Residual (documented, accepted): a watcher can't prove *ordering* beyond present→absent→present; a
  pathological disarm that removes-then-instantly-re-arms before install would still read as a valid
  cycle — but that is not the regression class in scope (a *non-disarming* guard), and it would not
  actually prevent the race anyway, so it's out of scope.

---

## Security & Adversarial Considerations
- **Threat model.** Test-only change (one PowerShell file). No prod code, no secrets, no network, no
  deps, no crypto. The updater trust chain (minisign, pubkey, feed) is untouched.
- **Wedge / false-RED (the only real risk).** The new assertions run **only** in the positive leg,
  **only** after a successful update. The single flake vector is "didn't observe absence" → mitigated
  by tight install-window sampling + the 5s pre-disarm margin + sampling `/Query` before the blocking
  `/health` call. The negative leg is untouched.
- **False-GREEN closed.** Observing the task physically absent proves the disarm's *effect*, not a log
  claim a regression could emit without acting (the log-proof gap codex flagged).
- **Least privilege.** `permissions: contents: read` on the smoke job unchanged. `schtasks /Query` is a
  read-only local call. No attacker-influenced inputs.

---

## Audit verdicts
- **Opus subagent (Plan, opus):** approve-with-changes — caught the file-flush wedge risk (→ moot under
  state-proof). Full transcript: `audit-opus.md`.
- **Codex (xhigh, 019e93b2):** needs-rework — proposed the state-proof pivot now adopted; confirmed a
  permanent `false` disarm already fails the smoke, so the only gap was "returns true without removing,"
  which state-proof catches. Full transcript: `audit-codex.md`.
- **Final fresh-context codex pass:** waived — the design was *converged on by the audits themselves*
  (codex proposed state-proof; opus's concerns are moot under it) and the owner chose it; a fresh pass
  would re-audit a design both auditors already endorsed. Post-impl codex audit still runs on the diff.

---

## Seeds

### /goal
```
/goal Phase 1 (state-proof full-cycle assertion in updater-smoke-windows.ps1) ✓ in implementations-plan/windows-disarm-proof-2026-06-04/plan.md with `LESSONS_FILE=…/lessons/phase-1.md` printed; the positive leg asserts armed (present pre-update) + disarmed (task observed ABSENT during install) + re-armed (present after), negative leg untouched; `/code-review max --fix` applied + committed; codex post-impl audit clean (or high/critical addressed); `bun run test` + `bun run lint:actions` exit 0 in transcript; then SURFACE+STOP for the owner-dispatched rc dry-run (Phase 2) — no autonomous release.
```

### /loop
```
/loop Each turn: (1) read implementations-plan/windows-disarm-proof-2026-06-04/plan.md + lessons/; git status; open PR? gh pr view --json statusCheckRollup. (2) CI on HEAD? gh run watch ≤10min. (3) Failed? triage+fix, /codex xhigh if non-trivial, commit+push; stop after 5 fails on one step. (4) Phase green? mark ✓, file lessons/phase-1.md, print LESSONS_FILE=…, advance. (5) Nothing in flight? next pending step (edit → bun run lint:actions → commit → push). (6) Phase 1 ✓? /code-review max --fix → commit → codex post-impl audit (/codex xhigh, adversarial+security) → address high/critical → SURFACE+STOP for the owner rc dispatch (Phase 2). NEVER merge to main or cut a release autonomously.
```
