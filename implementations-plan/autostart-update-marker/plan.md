# Plan — autostart update marker (piece 2)

`/blueprint mid`, **revision 3** — post dual-audit + final fresh-context pass (reject, 4 blocking — folded, §10 rounds 1–2) (fable: conditional-approve, 3 blocking; codex:
reject, 6 blocking; run in parallel, neither seeing the other). All findings folded or explicitly
adjudicated in §10. The DESIGN remains settled elsewhere
(`implementations-plan/autostart-self-heal/plan.md` §3.2 Fork B r4/r5, D18/D21/D22/D23, §11b);
this plan is its implementation mapping onto `main` @ `921d5ab`.

Success criteria (from the goal): D18/D21/D22 implemented; the `not(target_os = "windows")` gate on
the startup heal removed; L6 flipped from "seeded value unchanged" to "healed to the exactly quoted
installed exe". Quality bar: production.

**The one rule both audits enforced, now global:** every marker read that gates a mutation happens
**under `autostart.lock`, immediately before the mutation** — never check-then-act across a lock
boundary. Marker creation is under that same lock, which is what makes the locked re-checks
race-free.

---

## 1. Files & shape

| File | Change |
|---|---|
| `src-tauri/src/update_marker.rs` | **new** — pure state machine + IO: schema, classification, `removal_decision`, expiry; reconcile actions taken as **injected closures** (house `enable_transaction` pattern) so tests never touch real schtasks |
| `src-tauri/src/lib.rs` | +`pub mod update_marker;` |
| `src-tauri/nsis/hooks.nsi` | **new `NSIS_HOOK_POSTINSTALL`** — token handoff (§3): `Delete` stale dest → `Rename update-txn → update-txn-done`; no handoff file ⇒ no-op |
| `src-tauri/src/updater.rs` | Windows: post-lock live-marker reject (D22); `record_pending` BEFORE the critical section; `autostart.lock` spans intent-read → disarm → marker+handoff create only, dropped before `install()`; every post-create exit reconciles to CURRENT intent with removal-success-before-rearm and a defused guard, intent-sensitive ordering on the Err path (§4) |
| `src-tauri/src/autostart.rs` | `acquire_autostart_lock` → `pub(crate)`; heal: step-0 fast-path marker check **plus locked re-check** inside the existing critical section; new `startup_rearm()` seam (Windows: lock → marker re-check → intent → rearm; elsewhere: intent → rearm); `set_enabled_at` ON branch (Windows, inside held lock): live marker ⇒ Err |
| `src-tauri/src/main.rs` | startup: `startup_reconcile()` → gate heal AND `startup_rearm()`; remove `not(target_os = "windows"))` from the heal cfg + rewrite `:606-611` comment; Quit handler takes `autostart.lock` around its disarm (disarm stays unconditional — §10 A4) |
| `src-tauri/nsis/harness.test.nsi` + `scripts/nsis-hook-test.sh` | POSTINSTALL cases: positive must-fire (nonce round-trip — kills the `!ifmacrodef` typo trap) + no-handoff no-op |
| `.github/workflows/accelerator.yml` | cert-trust Windows step: run the POSTINSTALL harness cases; L6: marker-absent precondition, flip `:616` to the healed quoted path, **new wiring scenario** (live marker ⇒ task NOT created, marker survives — §6 T5), F-B1 cleanup + `exit 0` kept |
| `src-tauri/tests/autostart_heal.rs` | Windows: suppression + reconciliation steps via injected closures — **never the real scheduled task** (§6 T3) |
| `CLAUDE.md`, `implementations-plan/index.md` | counts + index entry |
| `implementations-plan/autostart-self-heal/plan.md` | append the D23 amendment note (final pass: the upstream ledger must stop contradicting this plan's §5.5) |

Unchanged: `crash_recovery.rs`; `core/updater_state.rs` (marker never touches floor semantics);
all frontend/e2e-mock surfaces. Core-crate paths: `packages/accelerator/core/src/…`
(`updater_state.rs`, `win_acl.rs`) — cross-crate reuse has precedent (`certs.rs` uses
`secure_create_file`).

## 2. The marker (`update_marker.rs`)

```rust
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkerPayload {
    pub schema: u32,                   // 1
    pub txn: String,                   // transaction nonce
    pub candidate: String,             // canonical SemVer — removal rule #1 ONLY (never floor logic)
    pub expected_install_path: String, // removal rule #2
    pub intent_at_disarm: bool,        // D18 snapshot (diagnostic; reconcile uses CURRENT intent)
    pub deadline_unix: i64,            // absolute, in-payload; mtime never trusted
}

pub enum LoadedMarker { Missing, Corrupt, Valid(MarkerPayload) }
// Corrupt INCLUDES a well-formed payload whose deadline is > 24h out (future-date clamp).
// Expire is ONLY now >= deadline on a well-formed, in-clamp payload.

pub enum Decision { Remove, Suppress(&'static str), Expire(&'static str) }

pub fn marker_path() -> Option<PathBuf>;   // ~/.aztec-accelerator/update-in-progress.json
pub fn load(path) -> LoadedMarker;         // bounded read, schema + canonical-SemVer round-trip
pub fn is_live(&LoadedMarker, now) -> bool; // Valid && now < deadline
pub fn create_new(path, &MarkerPayload) -> Result<(), CreateErr>; // compare-and-create; caller holds autostart.lock
pub fn remove_all(path) -> Result<(), String>;                     // marker + token + handoff
pub fn removal_decision(m, token_nonce, running, exe_canon, expected_exists, now) -> Decision;
```

`removal_decision` is pure: `Remove` iff token nonce == `txn` AND running == `candidate` AND
(exe_canon == expected_install_path OR (!expected_exists — the settled rename-tolerance clause));
`Expire` iff now ≥ deadline; else `Suppress(reason)`. Future-dating never reaches it (classified
`Corrupt` at load). The reconcile ACTIONS (arm/disarm) are performed by callers via injected
closures so the truth table and transaction tests run everywhere with counters.

Windows creation: `win_acl::secure_create_file` (CREATE_NEW, reparse-safe, owner-only, readback).
`create_new` on an existing file: reload **under the caller's held lock**; live ⇒ `CreateErr::Live`;
expired/corrupt ⇒ delete + one retry.

## 3. The completion token (D21) — NSIS mechanics

1. `perform_update` (Windows) writes `~/.aztec-accelerator/update-txn` — one line, the `txn` nonce —
   under the same lock hold as marker creation.
2. `NSIS_HOOK_POSTINSTALL` (fires after ALL copies + uninstaller + registry + shortcuts — recon §A):
   if `$PROFILE\.aztec-accelerator\update-txn` exists → `Delete` any stale `update-txn-done` →
   `Rename update-txn → update-txn-done` (NSIS `Rename` fails if dest exists — hence the Delete;
   double-fire is idempotent because the rename consumes the handoff). No handoff ⇒ no-op.
3. Rust reads `update-txn-done` (bounded, one line) and compares to `marker.txn`.

**Corrected claim (codex):** the hook is NOT strictly "update-only by construction" — a failed
update can leave a handoff file that a later MANUAL install consumes, writing a token. That is safe
by binding, not by construction: the stale token's nonce only matches its own marker, and removal
still requires version + path; the mismatch lands in `Suppress` → deadline expiry. Failure mode of a
failed `Rename`: handoff stays, no token, suppress-then-expire. Hook logs on failure (DetailPrint).

## 4. Control flow (rev 3 — every exit reconciles)

**`perform_update` (Windows-cfg), rev-3 ordering (final pass: the snapshot must live INSIDE the
lock, and `record_pending` OUTSIDE it — `autostart.lock` protects nothing there):**
1. After `acquire_updater_lock()` (`:275`): `load(marker)` — live ⇒ log + return (D22).
2. `record_pending` (moved BEFORE the critical section; it needs only `updater.lock` + precede
   `install()`. Failure ⇒ return — nothing else touched. A disarm failure later leaves pending
   recorded — the already-documented exact-version-retry semantics).
3. **Acquire `autostart.lock`** (bounded; timeout ⇒ return). The critical section is now only
   disarm→create — and owned intent mutations are frozen across it.
4. **Read current intent INSIDE the lock** (this becomes both `intent_at_disarm` and the guard's
   rearm input — it cannot go stale against owned mutations while held).
5. Disarm. Failure ⇒ rearm-if-enabled, drop lock, return (no marker exists).
6. Guard constructed.
7. `create_new(marker)` + write handoff. **Cleanup invariants (final pass #2):** the transaction
   tracks whether IT created the marker; `CreateErr::Live` is NEVER deleted (return, guard rearms —
   a foreign window stays intact); handoff-write failure after a successful create ⇒ `remove_all`,
   **checked**: if removal succeeds ⇒ reconcile to current intent under the held lock + defuse the
   guard; if removal FAILS ⇒ leave recovery disarmed (suppression until expiry is the safe
   direction) and defuse the guard so Drop cannot rearm into a live window.
8. Drop `autostart.lock`. `install()`:
   - Windows success: process exits inside; next launch reconciles.
   - **Err ⇒ under re-acquired `autostart.lock`, INTENT-SENSITIVE ordering (final pass #2): read
     current intent; if ON ⇒ `remove_all` first, checked, THEN arm (a failed removal leaves
     disarmed + suppressed — safe); if OFF ⇒ confirm disarm FIRST, then `remove_all` (removing
     first could leave intent-OFF/task-armed permanently if disarm confirmation fails). Guard
     defused on every branch.**

**`startup_reconcile()` (Windows; head of the startup block; AppHandle-free core):**
Everything under ONE `autostart.lock` hold — **load, classify, decide, act** (codex #2: a pre-lock
load can delete a marker it did not classify):
lock → `load(marker)` → Missing ⇒ Proceed. Corrupt (incl. future-dated) ⇒ delete + **Suppressed**
this launch (one-launch conservative window; §10 A2). Valid ⇒ current-intent read →
`removal_decision` → `Remove`: reconcile to current intent (ON ⇒ idempotent arm; OFF ⇒ confirmed
disarm) → `remove_all` ⇒ Proceed. `Suppress` ⇒ Suppressed. `Expire` ⇒ `remove_all`, **no
reconciliation** (the discriminator T3 asserts) ⇒ Proceed. Lock timeout ⇒ Suppressed. Runs
regardless of `AZTEC_ACCEL_NO_UPDATE` (recon §H). This transaction is the ONLY rearm path allowed
while a marker exists (r5 exemption).

**`heal_if_broken_at`:** step 0 (Windows) stays as the cheap fast path; **the authoritative check
is a marker re-load inside the existing locked section** (after `:1332`, beside the stored-target
re-read). Creation is under the same lock ⇒ race-free (fable #1 / codex #1).

**`startup_rearm()` (new seam, called by `main.rs` on the Proceed path):** Windows: `autostart.lock`
(bounded) → marker re-load — live ⇒ skip → `intent_enabled` → `enable_crash_recovery()` under the
lock (the hold-across-schtasks pattern `set_enabled_at` already uses). Non-Windows: plain
intent→rearm. Sequential with the heal's own lock hold — never nested (the lock is not reentrant).

**`set_enabled_at`:** ON branch (Windows, inside the held lock): live marker ⇒
Err("an update is finishing; try turning Start on Login on again in a moment"). OFF: untouched.

**Quit handler (`main.rs:393`):** acquires `autostart.lock` (bounded) around its
`disable_crash_recovery()`; the disarm stays UNCONDITIONAL (§10 A4, rationale corrected per the
final pass). **Timeout semantics defined (final pass #3): on lock timeout, log a warning and
disarm UNLOCKED anyway, then quit** — not cancel-the-quit (a quit that refuses to quit is worse
UX than either race), and not skip-disarm (relaunch-after-quit is user-visible). The unlocked
fallback's worst case is the pre-existing self-healing transient (§10 A5).

**Then:** remove `not(target_os = "windows")` from `main.rs:612`; rewrite `:606-611` and
`autostart.rs:1325-1326` comments.

## 5. Decisions this plan makes beyond the ledger (adjudicated in §10)

1. Plain-text nonce handoff + NSIS `Delete`→`Rename` (both audits: sound; claim corrected §3).
2. Corrupt marker (INCLUDING future-dated beyond 24h clamp) = delete + one-launch suppression —
   one semantics everywhere (fable #3 / codex non-blocking: r1 contradicted itself).
3. Deadline `now + 15 min`; clamp 24 h. Constants.
4. Compare-and-create replaces only expired/corrupt, reloaded under the caller's held lock (codex #2).
5. **D23's NSIS-side reparse check: explicitly REOPENED and amended, not silently skipped**
   (codex #5 — r1 claimed "as written" while omitting it, which was dishonest). Amendment: NSIS has
   no junction-refusal primitive; building one from `System::Call` is disproportionate for a
   same-user availability lever (piece-1 §9 stance). Rust-side hardening unchanged (bounded reads,
   nonce equality, `deny_unknown_fields`; a junction-redirected token cannot forge acceptance).
6. Store `intent_at_disarm`, reconcile to CURRENT intent (r5; both audits confirmed).
7. Quit: lock yes, conditional-disarm no (§4; §10 A4).

## 6. Test plan

- **T1 — marker state table** (unit, all OSes): round-trip + `deny_unknown_fields`;
  Missing/Corrupt/Valid; future-date ⇒ Corrupt (NOT Expire — pins §5.2); expiry boundary;
  `removal_decision` truth table (token missing / nonce mismatch / version mismatch / path
  mismatch / rename-tolerance / expired); compare-and-create: live loses, expired replaced,
  CREATE_NEW race (two writers, one wins).
- **T2 — NSIS harness** (`test:nsis` local + cert-trust Windows CI step): positive must-fire
  (handoff `abc123` → token contains `abc123`, handoff gone); no-handoff no-op; double-fire
  idempotence (second install run: token unchanged, no error).
- **T3 — reconciliation transaction** (unit, injected closures + counters — codex #6: NEVER the
  real scheduled task): Remove path arms (intent ON) / confirm-disarms (intent OFF) exactly once;
  **Expire: asserts the expired marker PRE-EXISTED, is removed, outcome is Proceed, AND zero
  reconciliation calls** (final pass: not merely zero callbacks); Suppressed paths make zero calls;
  lock-timeout ⇒ Suppressed.
- **T4 — barrier race test** (unit, both audits' highest-value ask): marker-absent asserted as a
  PRECONDITION; operation observes Missing; marker is published (temp path) before the re-check
  under the lock; operation must return exactly `Skipped`/`Suppressed` with **zero**
  heal-write/rearm callbacks. **Drives the PRODUCTION cores across the real lock boundary** (the
  `*_at` surfaces with injected paths/closures), not a duplicated helper (final pass). Covers the
  heal re-check AND `startup_rearm`.
- **T5 — CI wiring scenario** (fable's E.2 gap; L6 extension): seed a LIVE valid marker + intent-ON
  Run value + no schtasks task → launch installed app → assert task NOT created AND marker
  survives → clean marker → relaunch → assert normal heal path (flipped assertion: Run value ==
  exactly quoted installed exe) → F-B1 cleanup → `exit 0`. Marker-absent asserted as a
  PRECONDITION of the heal scenario.
- **T6 — Windows integration** (`tests/autostart_heal.rs`, temp marker paths, RAII registry guard):
  live marker ⇒ heal `Skipped` + `set_enabled_at(true)` Err; **the OFF-unaffected property is
  pinned WITHOUT calling `set_enabled_at(false)`** (final pass: that would disarm the developer's
  real scheduled task, which the registry RAII cannot restore) — artifact-level OFF via
  `remove_entry()` plus the T7 closure table proving the marker gate exists only in the ON branch;
  full Remove flow with matching token/version/path against injected reconcile counters.
- **T7 — updater cleanup-transaction table** (unit, injected closures — final pass #4, the central
  transaction none of T1–T6 reached): handoff-write failure AFTER a successful marker create
  (removal checked; success ⇒ reconcile + defused guard; failure ⇒ disarmed + defused guard, marker
  left live); `CreateErr::Live` never deletes; install-Err intent-sensitive ordering both
  directions (ON: remove-checked-then-arm; OFF: confirm-disarm-then-remove, incl. the
  disarm-confirmation-failure branch leaving the marker in place); guard defused on every branch
  (Drop fires zero stale rearms).

## 7. Phases & gates

| # | Work | Gate |
|---|---|---|
| 1 | `update_marker.rs` + T1 + T3 + T4 | `cargo test update_marker`; clippy `-D warnings`; fmt |
| 2 | hooks.nsi + harness (T2) + CI step | `test:nsis` green locally; `lint:actions`; `lint:shell` |
| 3 | updater.rs + autostart.rs + main.rs wiring (incl. ungate + comments + Quit lock) | `cargo test`; `bun run test` |
| 4 | T6 + L6/T5 flip | full local gate; push; cert-trust (windows) + Windows Build Smoke green |
| 5 | Review loop | ONE codex xhigh audit → fix → ONE resumed "fully closed or half-applied?" → merge |

## 8. Assumptions

**Facts:** recon.md (file:line throughout); NSIS `Rename` fails when dest exists (NSIS docs, codex);
`accelerator-v1.0.7` installs exist in the wild writing schema-1 `updater-state.json` (fable
verified) — the competing outline's migration risk is real.

**Inferences (challenge):** 15 min covers real installer runtimes incl. AV interference; holding
`autostart.lock` across disarm→create (seconds) does not starve the Settings toggle beyond its
bounded 10s wait; wine models the handoff Rename faithfully enough for T2, with CI's real-Windows
step as the authority.

**Asks:** none blocking.

## 9. Security

Unchanged from r1 (§9): same-user availability lever, not a boundary. Additions from the audits:
every marker read that gates a mutation is lock-protected (TOCTOU class closed at all four sites +
Quit); removal success is verified before any rearm; a failed removal leaves the SAFE direction
(recovery disarmed, window suppressed until expiry). D23 amendment recorded in §5.5. Suppression
remains deadline-bounded, so no forged file suppresses updates or recovery indefinitely.

## 10. Decision ledger (audit round 1)

**Both audits, independently — adopted:** locked re-check before every gated mutation (fable #1+#2,
codex #1); the future-date semantics contradiction (fable #3, codex NB) → Corrupt, one semantics;
the barrier test as highest-value missing (both, converging on the same test from different sites).

**Codex-only — adopted:** #2 load-classify-act under one lock (stale-reconciler delete);
#3 `record_pending` BEFORE marker creation + every post-create exit reconciles to CURRENT intent
under the lock + removal-success-before-rearm + defused guard; #5 D23 reopened explicitly (A3
below); #6 T3 via injected closures — the real scheduled task is never mutated by tests (piece-1
rule); NB: "update-only by construction" corrected to safe-by-binding.

**Fable-only — adopted:** the `startup_rearm` seam; T5 CI wiring scenario; T3 Remove/Expire
discriminator; T4/T5 marker-absent preconditions; core-crate path fix; outline rejection
strengthened (5th reason: lost-update between two lock regimes on one file — and codex added the
positive converse: the delayed floor tracker can clear `pending` while the marker legitimately
remains, so the separate candidate copy PREVENTS drift rather than causing it).

**Adjudicated (partial adoption):**
- **A4 (codex #4, Quit handler):** lock adopted (serializes vs reconciler/toggle); conditional
  disarm REJECTED — disarm is the safe direction (fable's audit and piece-1 both record this);
  skipping it on a live marker risks relaunch-after-quit, which is user-visible and worse than
  codex's transient (intent ON / task off at quit), which next launch's rearm fixes. The
  startup-rearm-vs-quit race codex's scenario reduces to is pre-existing and unchanged by this
  piece — documented, not expanded.
- **A3 (codex #5, D23 reparse):** amended openly rather than implemented — §5.5. This is a design
  reopening and is labeled as such; the final fresh-context pass and the post-impl audit both see it.

**Confirmed sound by both:** the NSIS handoff mechanics (incl. double-fire idempotence,
kill-mid-install, stale-handoff-months-later); `perform_update` rearm paths via lock exclusion;
the enable/disable call-site census (complete); the competing-outline rejection; §5.6
snapshot-vs-current.

## §10 addendum — round 2 (final fresh-context codex pass: reject, 4 blocking)

**Adopted in full:**
- **#1** — `record_pending` moved OUT of the critical section (it needs only `updater.lock`); the
  intent read moved INSIDE `autostart.lock`, becoming both the stored snapshot and the guard input,
  so it cannot go stale against owned mutations. (This also resolved the consolidator's own §-worry
  from rev 2 — the lock protected nothing in step 6.)
- **#2** — post-create cleanup invariants: transaction tracks whether IT created the marker;
  `CreateErr::Live` never deletes; removal success checked before any rearm; reconcile under the
  held lock; guard defused on every branch; install-Err ordering is INTENT-SENSITIVE (ON:
  remove-then-arm; OFF: confirm-disarm-then-remove — removing first could leave intent-OFF/
  task-armed permanently on a failed confirmation).
- **#4** — T3/T4/T6 tightened; **T7** added for the cleanup transaction; T6's real
  `set_enabled_at(false)` dropped (would have disarmed a developer's real scheduled task — the
  piece-1 unrestorable-shared-state rule, caught at plan time for the second occurrence).
- **A4 rationale corrected** as directed: with the invariant intact, a live marker normally means
  recovery is already disarmed, so conditional skipping does not itself leave the task armed; the
  decisive argument is monotone-safety of unconditional disarm + the lock closing the
  arm-before-remove race. Conclusion unchanged.
- **Upstream ledger amendment**: `autostart-self-heal/plan.md` D23 gets an explicit amendment note
  so "settled upstream" no longer contradicts §5.5 — rides in this PR.

**Adjudicated (A5 — Quit lock-timeout semantics, final pass #3 partially adopted):** the pass
demanded defined semantics and suggested cancel-the-quit. Defined here as: **log warning + UNLOCKED
disarm + quit**. Cancel-the-quit is rejected (a quit that refuses to quit is worse UX than either
race); skip-disarm is rejected (relaunch-after-quit is user-visible). The unlocked fallback's worst
case is the pre-existing self-healing transient. The 10s-contention observation stands and is why
the fallback exists at all.

**Round-2 close-out:** the remaining verification burden (does the implementation honour rev 3?)
transfers to the per-PR review loop mandated by the goal — one post-impl codex audit + one resumed
verification — rather than a fourth planning pass. Rationale: all round-2 findings were mechanical
mappings of settled invariants, none reopened design.
