# Final fresh-context pass — codex (new session), plan revision 2 → 3

Session `019fae2f-2c49-7ba0-9d0d-d9257cafb387`. Verdict: REJECT (4 blocking). All folded in rev 3;
adjudications A4-rationale-corrected and A5 recorded in plan.md §10 addendum.

**Verdict: reject.**

### Blocking

1. [`plan.md` §4 steps 2–6]: the intent snapshot is still taken before `autostart.lock`. ON can be read, then the user completes OFF, then `record_pending` fails and the stale guard rearms ON. Move `record_pending` before the autostart critical section; then lock → read current intent → disarm → create marker/handoff. Step 6 is protected by `updater.lock`; `autostart.lock` protects nothing there.

2. [`plan.md` §4 step 7 and install-Err branch]: “any failure → best-effort remove → stale guard rearm” violates the claimed post-create invariant. If marker creation succeeds but handoff creation fails, failed removal leaves a live marker with recovery rearmed. Cleanup must track whether this transaction created the marker, check removal, reconcile current intent under the held lock, and defuse the guard on every branch. Never delete on `CreateErr::Live`. For install `Err`, ordering must be intent-sensitive: ON = remove successfully, then arm; OFF = confirm disarm, then remove. Removing first can permanently leave intent OFF/task armed when confirmation fails.

3. [`plan.md` §4 Quit]: lock-timeout behavior is undefined. Unlocked disarm reopens the race; exiting without disarm causes relaunch. Specify that timeout cancels the quit and reports/logs failure. The 10-second bound is reachable: `schtasks` subprocesses have no execution timeout, and Settings, startup reconciliation/rearm, Quit, and an updater can all contend. Heal alone normally bows out on `updater.lock`.

4. [`plan.md` §6]: T1–T6 do not test the central updater cleanup transaction—especially handoff failure after marker creation, removal failure, stale/current intent, and guard defusing. T3 must assert expired-marker preexistence, removal, and `Proceed`, not merely zero callbacks. T4 needs explicit marker-absent preconditions and must drive the production cores/lock boundary, not a duplicated injected helper. T6’s “OFF still works” through `set_enabled_at(false)` mutates the real scheduled task without RAII restoration, contradicting the test-safety rule.

### Non-blocking

The locked rechecks correctly close dual-logon, ordinary mid-install suppression, and startup-rearm/heal TOCTOUs. Toggle-during-download is safe until the pre-lock snapshot gap above. Quit and reconciliation serialize once timeout semantics are defined.

A4’s conclusion is right, though its rationale should be corrected: with the invariant intact, a live marker normally means recovery is already disarmed, so conditional skipping does not itself “leave the task armed.” The decisive point is that unconditional disarm is monotone-safe and the lock closes Codex’s arm-before-remove race; reverse-order rearm-after-quit remains the pre-existing multi-instance race either way. A3 is a sound, proportionate amendment for a current-user installer and same-user availability threat model; nonce/version/path binding preserves acceptance integrity. Explicit reopening is the right process, but the authoritative upstream D23 ledger should also be amended so “settled upstream” no longer contradicts this plan.