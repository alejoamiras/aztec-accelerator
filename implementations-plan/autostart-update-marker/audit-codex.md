# Audit round 1 — codex (gpt-5.6-sol, xhigh, read-only)

Session `019fae22-16dc-7133-abfb-a421a35504c6`. Verdict: REJECT (6 blocking). Ran in parallel with
the fable audit; neither saw the other. Adjudication in plan.md §10 (A4 partially adopted with
rationale; #5 resolved as an explicit D23 amendment rather than implementation).

Reject.

## Blocking findings

1. **“Proceed” is stale before the protected mutations.** The plan reads/reconciles once, then separately heals and rearms [plan.md:107] ()). Interleaving: startup sees Missing; updater publishes the marker and exits, releasing `updater.lock`; heal either returns early at [autostart.rs:1320] () or acquires the now-free updater lock without rereading the marker; startup then heals or rearms at [main.rs:630] (). Marker checks must occur inside `autostart.lock` immediately before both the heal write and startup rearm.

2. **Expired replacement can be deleted by a stale reconciler.** `startup_reconcile` loads before taking `autostart.lock` [plan.md:113] ()). It can load expired marker A, wait while an updater replaces A with live B [plan.md:145] ()), then acquire the lock and delete B using A’s decision. Load/classify/delete must happen under one lock, with a locked reload or transaction-ID comparison. Corrupt deletion has the same issue.

3. **Failure cleanup is incomplete and decision 6’s snapshot claim is false.** The detailed sequence takes `autostart.lock` only after disarm [plan.md:100] ()), contradicting §1. Marker/handoff creation also precedes fallible `record_pending` [updater.rs:412] ()), yet cleanup is specified only for `install()` Err. Handoff or pending-write failure strands a live marker while the guard rearms. Worse, OFF is allowed during install; returned Err then deletes the marker and rearms from stale `was_recovery_enabled` [updater.rs:450] ()), leaving intent OFF/task ON. Every post-create exit needs cleanup plus reconciliation to current intent under `autostart.lock`, with removal success checked before rearm.

4. **Quit can invalidate removal rule #4.** The handler disables recovery without `autostart.lock` or a marker gate [main.rs:393] ()). It can disable after reconciliation arms ON but before the reconciler removes the marker. The marker is then removed with recovery no longer reconciled. Quit must participate in the same lock and skip disarm when the locked marker is live.

5. **Decision 5 contradicts settled D23.** The settled plan explicitly requires a reparse/junction check for the NSIS token destination; [plan.md:147] () omits it while claiming D23 is implemented as written. That requires either implementation or an explicit design reopening.

6. **T3 mutates unrestorable shared state.** Registry RAII does not protect the real scheduled task modified by reconciliation/OFF; the existing test deliberately avoids that at [autostart.rs:1353] (). Inject recovery closures/counters.

## Non-blocking

“Update-only by construction” is false: a failed update can leave `update-txn`, which a later manual installer consumes. Nonce/version/path binding keeps this safe. NSIS `Rename` fails when the destination exists, so Delete→Rename and double firing are safely idempotent; errors should still be logged. The T2 positive case genuinely pins the must-fire macro trap. [NSIS Rename documentation] (https://nsis.sourceforge.io/Reference/Rename)

Resolve the inconsistency where future-clamped markers are `Expire` at plan line 70 but `Corrupt` at line 144.

The highest-value missing test is one deterministic barrier test: heal/startup observes Missing, updater publishes a live marker and releases `updater.lock`, then the operation resumes and must assert exactly `Skipped/Suppressed` with zero heal/rearm callbacks—using only temporary paths and counters.

## Genuinely sound

Rejecting the folded updater-state outline is sound. A suppressed new build can become healthy and clear `pending` via the delayed floor tracker [updater_state.rs:195] ()) while its marker remains. The separate candidate copy therefore prevents real drift from destroying removal semantics. The nonce handoff, 15-minute/24-hour bounds, current-intent reconciliation, and the `set_enabled_at` ON gate are otherwise reasonable; `enable_transaction` has no other production entry point.