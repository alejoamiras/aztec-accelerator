# Audit round 1 — fable (fresh-context planning subagent)

Ran in parallel with the codex audit; neither saw the other. Verdict: **conditional-approve**
(3 blocking). All three blocking findings independently overlap codex #1/#2 territory (the locked
re-check rule) plus the future-date contradiction — the convergence is what made the fold
unambiguous. Adjudication in plan.md §10.

---

Verdict: **conditional-approve.** The mapping is faithful to the settled design at three of four
suppression sites; two gate-placement TOCTOUs and one internal spec contradiction must be fixed
before implementation.

## Blocking

**1. Heal step 0 is an unlocked marker check with no re-check under the lock — Fork B resurrected.**
Plan §4 puts the marker check at step 0 of `heal_if_broken_at`, before the locks at
`autostart.rs:1327` (updater probe) and `:1332` (autostart.lock); the under-lock re-read at `:1336`
re-checks only the stored target. Concrete failure: Run entry is Broken (stale third path);
instance Q (Downloads copy) handles a `repair_autostart` click (`commands.rs:69`, ungated, any
time). Q passes step 0 (marker not yet created), passes step 3 (Broken); meanwhile N creates the
marker and `install()` exits — `updater.lock` dies with the process. Q then acquires BOTH now-free
locks, re-reads Broken, and writes Q's path inside the live NSIS window. New N's reconcile later
removes the marker (all four match), its heal sees Healthy-pointing-at-Q → NotNeeded; user deletes
Q → autostart broken. That is the exact Fork B counterexample the marker exists to kill. Fix:
re-check the marker after `:1332` (creation is under the same lock, so the re-check is race-free);
keep step 0 as fast path.

**2. The startup-rearm gate is the same TOCTOU at the second site.** The plan gates
`main.rs:630-640` on `startup_reconcile`'s outcome, but the rearm itself runs lock-free, after an
unlocked marker load. A second instance (the redundant-instance bow-out is async — `main.rs:270-303`
fires only on the later bind failure, so the whole `:603-641` block runs first) can load Missing ⇒
Proceed, then N disarms and creates the marker; the second instance's `enable_crash_recovery()`
then arms the repeating task inside the freshly-created window — the exact half-written-exe-spawn
hazard the disarm exists for. Fix: perform (or gate) the Proceed-path rearm under `autostart.lock`
with a marker re-check, mirroring fix 1.

**3. Future-dated marker: the plan contradicts itself.** §2 says `removal_decision` returns
**Expire** for "future-dated beyond clamp" (Expire ⇒ remove + **Proceed**); §5.3 says a >24h
deadline is "treated as **Corrupt**" (§5.2: delete + **Suppressed** this launch); `is_live`'s
"(clamped)" is a third, undefined semantics. Pick one (Corrupt is the better fit — "the window is
over" doesn't describe a malformed payload) and make all three surfaces agree.

## Non-blocking

- `startup_reconcile` loads the marker before taking the lock and never re-reads. In the documented
  logon dual-launch, the loser runs `removal_decision` on a stale payload whose token the winner
  already deleted ⇒ spurious "Suppressed" + one-launch heal/rearm skip. Fail-safe, but a re-read
  under the lock yields clean Missing ⇒ Proceed.
- T3 "expired ⇒ removed + Proceed" is a two-outcome test: Remove and Expire both end marker-gone +
  Proceed. Assert the discriminator — Expire performs no recovery reconciliation (seed intent ON +
  task disarmed; Remove arms, Expire doesn't).
- **Highest-value missing test:** nothing exercises the `main.rs` wiring — the exact recon §E.2 gap
  this piece closes. Add an L6-style CI step: seed a live valid marker + intent-ON Run value + no
  schtasks task, launch the installed app, assert the task was NOT created and the marker survives.
  Every planned test drives library functions; a wiring bug gating only the heal (not the rearm)
  passes all of §6. Also: T4 should assert marker-absent as a precondition.
- Path nit: "core/updater_state.rs"/"core/win_acl.rs" are actually the separate `accelerator_core`
  crate at `packages/accelerator/core/src/…`; cross-crate reuse works (`certs.rs` already uses
  `secure_create_file`).
- §E: the outline's rejection is honest — verified: tag `accelerator-v1.0.7` exists (schema-1
  installs in the wild) and `updater_state.rs:85` makes wrong-schema Corrupt/fail-closed. Add a
  sharper fifth reason: two writers under two lock regimes on one file is a concrete **lost
  update** (floor-tracker commit under `updater.lock` racing window-removal under `autostart.lock`
  can resurrect a removed window), not merely a coupling smell.

## Genuinely sound

NSIS handoff (§3/D): fresh-install no-op is by-construction and T2's positive case kills the
`!ifmacrodef` trap; double hook run is idempotent (rename consumes the handoff); kill-mid-install
and stale-handoff-months-later both resolve via suppress-then-expire. `perform_update`'s rearm
paths are marker-safe by lock exclusion (foreign creation requires the `updater.lock` it holds)
plus Err-path remove-before-guard ordering. The enable/disable census is complete. Leaving the
Quit-menu disable ungated is correct — disarm is the safe direction and removal reconciles [note:
codex #4 disagreed; adjudicated in plan.md §10 A4 — lock added, disarm stays unconditional].
§5.6 (store snapshot, reconcile to current) correctly implements r5. §5.5's no-NSIS-reparse
residual is proportionate given the same-user threat model [note: codex #5 disagreed on process
grounds; resolved as an explicit D23 amendment].
