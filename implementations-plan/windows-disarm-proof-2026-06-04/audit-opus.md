# Opus subagent audit (Plan, model opus) — #97 plan

**Verdict: approve-with-changes.** Core insight confirmed sound: a `false`-returning disarm already
fails the smoke (updater.rs:92 aborts → `/health` stays N-1 → positive leg dies at ps1:222), so the
only uncaught regression is a disarm that returns `true` WITHOUT removing — and a "removed (was armed)"
log assertion is the right tool. Prod-guard integrity preserved.

## Ranked concerns

**1. (WEDGE RISK — load-bearing) Flush across `app.restart()` is NOT covered by "read the file."**
The disarm line is written by N-1's `non_blocking` appender (main.rs:192, background-thread flush).
`app.restart()` resolves to `std::process::exit` (code=Some(i32::MAX), main.rs:475 path), which does
NOT run the `_guard` destructor → N-1's tail buffer (which may include the `removed` line if written
late) can be lost, and N never re-writes it. The NSIS install gives time but there's no flush barrier.
A bounded poll can't recover a never-flushed line → a hard assertion on the FILE can **false-RED a
blocking gate**. → ADOPTED FIX: assert on N-1's **stdout** (captured via `Start-Process
-RedirectStandardOutput`), not the non-blocking file. The stdout layer (main.rs:198) is RAW
`std::io::stdout()` = Rust `LineWriter`, flushed per newline → the `removed` line is on disk before the
NSIS install even starts. Sidesteps the flush-race without a prod flush change.

**2. (FACTUAL) Wrong line for the load-bearing log.** Disarm-success `info!` is **crash_recovery.rs:298**
(verified), not :303 (that's the `sleep`). Listed under "Facts (verified)" but wasn't. → ADOPTED: fix
the cite to :298.

**3. (FALSE-GREEN gap) "registered after removed" ordering for re-arm is unreliable.** Re-arm is
conditional on `app.autolaunch().is_enabled()` (updater.rs:127 — true here, Run key set ps1:171), and
the post-restart N `registered` may not be captured / the pre-restart one may be the lost line. →
ADOPTED: the HARD re-arm assertion stays the durable post-update `schtasks /Query` PRESENT check; the
log ordering is SOFT corroboration (warn, never `Write-Error`). With the stdout-capture fix, N-1's
pre-restart re-arm `registered` is also reliably captured.

**4. (CONFIRMED FINE) `.unwrap_or(true)` for the new up-front `/Query` is the correct safe default** —
unreadable Query → treat as present → go through delete loop → never short-circuit `return true`.
Verified against the existing `still_present` default (crash_recovery.rs:296). No new
`true`-while-present path. Skipping the no-op `/Delete` in the genuinely-absent case is safe.

## What's fine (per opus)
- Refactor preserves the `bool` contract + 3-attempt retry; updater abort path (updater.rs:92-101) untouched.
- `workflow_dispatch` genuinely absent from `_e2e-updater-windows.yml` → rc-only validation claim holds.
- Security/least-privilege: no new secrets/net/deps; `permissions: contents: read` unchanged; fixed-string log.
- Residual "lying /Query" correctly identified as the only escape and correctly scoped out.
- `Dump-Logs` reads the exact dir (ps1:80); TASK_NAME (:200), arm log (:267), test block (:361) accurate.
- `mid` tier is right.
