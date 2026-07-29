# Plan — autostart update marker (piece 2)

`/blueprint mid`, **revision 2** — post dual-audit (fable: conditional-approve, 3 blocking; codex:
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
| `src-tauri/src/updater.rs` | Windows: post-lock live-marker reject (D22); `autostart.lock` acquired **before disarm**, held through `record_pending` → marker+handoff creation, dropped before `install()`; **every post-create exit** cleans up under the lock and reconciles to CURRENT intent (§4) |
| `src-tauri/src/autostart.rs` | `acquire_autostart_lock` → `pub(crate)`; heal: step-0 fast-path marker check **plus locked re-check** inside the existing critical section; new `startup_rearm()` seam (Windows: lock → marker re-check → intent → rearm; elsewhere: intent → rearm); `set_enabled_at` ON branch (Windows, inside held lock): live marker ⇒ Err |
| `src-tauri/src/main.rs` | startup: `startup_reconcile()` → gate heal AND `startup_rearm()`; remove `not(target_os = "windows"))` from the heal cfg + rewrite `:606-611` comment; Quit handler takes `autostart.lock` around its disarm (disarm stays unconditional — §10 A4) |
| `src-tauri/nsis/harness.test.nsi` + `scripts/nsis-hook-test.sh` | POSTINSTALL cases: positive must-fire (nonce round-trip — kills the `!ifmacrodef` typo trap) + no-handoff no-op |
| `.github/workflows/accelerator.yml` | cert-trust Windows step: run the POSTINSTALL harness cases; L6: marker-absent precondition, flip `:616` to the healed quoted path, **new wiring scenario** (live marker ⇒ task NOT created, marker survives — §6 T5), F-B1 cleanup + `exit 0` kept |
| `src-tauri/tests/autostart_heal.rs` | Windows: suppression + reconciliation steps via injected closures — **never the real scheduled task** (§6 T3) |
| `CLAUDE.md`, `implementations-plan/index.md` | counts + index entry |

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

## 4. Control flow (rev 2 — every exit reconciles)

**`perform_update` (Windows-cfg), replacing the r1 sequence:**
1. After `acquire_updater_lock()` (`:275`): `load(marker)` — live ⇒ log + return (D22).
2. Intent snapshot (`:365-376`, existing).
3. **Acquire `autostart.lock`** (bounded; timeout ⇒ return — nothing mutated yet). Nesting order
   updater→autostart matches the heal; the held lock also freezes owned intent mutations across
   the disarm→create span, which is what keeps the snapshot honest inside it.
4. Disarm (`:383-393`). Failure ⇒ rearm-if-was-enabled, drop lock, return (no marker exists — clean).
5. Guard constructed (`:399-401`, existing).
6. `record_pending` (`:412-428`). Failure ⇒ drop lock, guard rearms, return (**still no marker** —
   codex #3's ordering fix: the fallible floor write happens BEFORE marker creation).
7. `create_new(marker)` + write handoff. Any failure ⇒ `remove_all` (best-effort), drop lock, guard
   rearms, return.
8. Drop `autostart.lock`. `install()`:
   - Windows success path: process exits inside — marker lives, NSIS runs, next launch reconciles.
   - **Err ⇒ under re-acquired `autostart.lock`: `remove_all` FIRST; if removal succeeded,
     reconcile recovery to CURRENT intent (not the stale snapshot — the user may have toggled OFF
     via the still-allowed OFF path); defuse the guard (`rearm_now`-equivalent marking) so Drop
     cannot double-fire a stale rearm. If removal FAILED, leave recovery disarmed and the marker
     live — suppression until expiry is the safe direction (codex #3: removal success is checked
     before any rearm).**

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
`disable_crash_recovery()`; the disarm stays UNCONDITIONAL (§10 A4 — skipping it on a live marker
would leave the task armed through a user quit: relaunch-after-quit is strictly worse than the
transient codex described, which self-heals at next launch).

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
  **Expire performs ZERO reconciliation calls** (the Remove/Expire discriminator — fable);
  Suppressed paths make zero calls; lock-timeout ⇒ Suppressed.
- **T4 — barrier race test** (unit, both audits' highest-value ask): operation observes Missing,
  marker is published (temp path) before it re-checks under the lock, operation must return exactly
  `Skipped`/`Suppressed` with **zero** heal-write/rearm callbacks — deterministic via injected
  closures, temp paths, counters. Covers heal re-check AND `startup_rearm` re-check.
- **T5 — CI wiring scenario** (fable's E.2 gap; L6 extension): seed a LIVE valid marker + intent-ON
  Run value + no schtasks task → launch installed app → assert task NOT created AND marker
  survives → clean marker → relaunch → assert normal heal path (flipped assertion: Run value ==
  exactly quoted installed exe) → F-B1 cleanup → `exit 0`. Marker-absent asserted as a
  PRECONDITION of the heal scenario.
- **T6 — Windows integration** (`tests/autostart_heal.rs`, temp marker paths, RAII registry guard):
  live marker ⇒ heal `Skipped` + `set_enabled_at(true)` Err + OFF still works; full Remove flow
  with matching token/version/path against injected reconcile counters.

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
