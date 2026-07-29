# Plan — autostart update marker (piece 2)

`/blueprint mid`. The DESIGN is settled and is not re-derived here: it lives in
`implementations-plan/autostart-self-heal/plan.md` — §3.2 Fork B (r4 design + r5 corrections),
D18 (marker), D21 (completion token), D22 (marker-aware updater), D23 (marker hardening),
§11b (piece split). This plan is the **implementation mapping** of that design onto the code at
`main` @ `921d5ab`, closing the gaps recon found between the design and its own file map
(`recon.md` §E), plus the small number of decisions the ledger left open (§5 below — the part
audits should attack hardest).

Success criteria (from the goal): D18/D21/D22 implemented as written; the
`not(target_os = "windows")` gate on the startup heal removed; the L6 smoke assertion flipped from
"seeded value unchanged" to "healed to the exactly quoted installed exe". Quality bar: production.

---

## 1. Files & shape

| File | Change |
|---|---|
| `src-tauri/src/update_marker.rs` | **new** — pure state machine + IO for the marker and token (schema, classification, removal decision, expiry); `#[cfg(test)]` state-table tests compiled on every OS |
| `src-tauri/src/lib.rs` | +`pub mod update_marker;` |
| `src-tauri/nsis/hooks.nsi` | **new `NSIS_HOOK_POSTINSTALL`** — completion-token handoff (§3) |
| `src-tauri/src/updater.rs` | Windows: reject live foreign marker after lock acquisition (D22); disarm→marker-create under `autostart.lock`; marker+handoff cleanup on the `install()` Err path |
| `src-tauri/src/autostart.rs` | `acquire_autostart_lock` → `pub(crate)`; heal step 0 (Windows): live marker ⇒ `Skipped("update in progress")`; `set_enabled_at` ON branch (Windows, inside the lock): live marker ⇒ Err (D-r5: "explicit ON rejected; OFF stays allowed") |
| `src-tauri/src/main.rs` | marker reconciliation at the head of the startup block; heal+rearm both gated on its outcome; remove `not(target_os = "windows")` from the heal cfg; rewrite the `:606-611` comment |
| `src-tauri/nsis/harness.test.nsi` + `scripts/nsis-hook-test.sh` | POSTINSTALL cases: positive must-fire (token appears with the right nonce — catches the `!ifmacrodef` silent-skip trap) + no-handoff no-op |
| `.github/workflows/accelerator.yml` | cert-trust Windows step: run the POSTINSTALL harness cases (the CI enforcement point — `test:nsis` is local-only); L6: flip `:616` to assert the healed quoted path, keep F-B1 cleanup order and the trailing `exit 0` |
| `src-tauri/tests/autostart_heal.rs` | Windows test: new numbered steps for suppression + reconciliation (§4) |
| `CLAUDE.md`, `implementations-plan/index.md` | counts + index entry |

Explicitly unchanged: `crash_recovery.rs` (the two existing primitives ARE the reconciliation
mechanism — no armed-state query exists and none is added); `core/updater_state.rs` (the marker
never touches floor semantics — recon §C); all frontend/e2e-mock surfaces (no UI change; the
`set_enabled` rejection surfaces through the existing toggle-error path).

## 2. The marker (`update_marker.rs`)

```rust
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkerPayload {
    pub schema: u32,                 // 1
    pub txn: String,                 // unique transaction ID (nonce)
    pub candidate: String,           // canonical SemVer — removal rule #1 ONLY, never floor logic
    pub expected_install_path: String, // canonical installed-exe path — removal rule #2
    pub intent_at_disarm: bool,      // snapshot per D18 (diagnostic + guard decisions)
    pub deadline_unix: i64,          // absolute; IN-PAYLOAD (mtime is forgeable)
}

pub enum LoadedMarker { Missing, Corrupt, Valid(MarkerPayload) }

pub fn marker_path() -> Option<PathBuf>;        // ~/.aztec-accelerator/update-in-progress.json
pub fn load(path) -> LoadedMarker;               // bounded read; schema/semver round-trip like updater_state
pub fn is_live(&LoadedMarker, now) -> bool;      // Valid && now < deadline (clamped, §5.3)
pub fn create_new(path, &MarkerPayload) -> Result<(), CreateErr>;  // compare-and-create (§5.4)
pub fn remove(path) -> Result<(), String>;       // marker + token + handoff file
pub fn removal_decision(m: &MarkerPayload, token_nonce: Option<&str>, running: &Version,
                        exe_canon: &Path, expected_exists: bool, now: i64) -> Decision;
```

`removal_decision` is a PURE function returning
`Remove | Suppress(&'static str) | Expire(&'static str)` — the whole four-part rule plus the
rename-tolerance clause as one unit-testable truth table:

- `Remove` iff token nonce == `txn` AND running version == `candidate` AND
  (exe_canon == expected_install_path OR (!expected_exists AND version already matched — the
  settled rename-tolerance clause)) — recovery reconciliation is then performed by the CALLER
  under the lock (it is an action, not a predicate).
- `Expire` iff now ≥ deadline, or the payload is future-dated beyond clamp (§5.3).
- else `Suppress(reason)`.

Windows creation goes through `win_acl::secure_create_file` (CREATE_NEW, reparse-safe, owner-only,
readback — already in the tree); Unix writes reuse the `write_state` tempfile+fsync pattern shape.
The marker file itself is only ever CREATED on Windows (`perform_update` windows-cfg), but the pure
layer compiles and tests everywhere, matching the autostart.rs precedent.

## 3. The completion token (D21) — NSIS mechanics

NSIS cannot parse JSON, so the nonce crosses via a plain-text handoff:

1. `perform_update` (Windows), after confirmed disarm and marker creation, writes
   `~/.aztec-accelerator/update-txn` — a single line: the `txn` nonce.
2. **`NSIS_HOOK_POSTINSTALL`** (fires after ALL file copies + uninstaller + registry + shortcuts —
   recon §A): if `$PROFILE\.aztec-accelerator\update-txn` exists → `Delete` any stale
   `update-txn-done` → `Rename update-txn → update-txn-done`. No handoff file ⇒ no-op — the hook is
   update-only BY CONSTRUCTION (fresh installs never have a handoff file), no `$UpdateMode`
   inspection needed.
3. Rust removal reads `update-txn-done` (bounded, single line) and compares to `marker.txn`.

Rename preserves content without NSIS FileRead/FileWrite loops, and its failure mode is safe: the
handoff file stays, no token appears, removal suppresses until deadline expiry (the documented
stranded exit). Both harness cases in §4 pin this.

## 4. Control flow changes

**`perform_update` (Windows-cfg additions, in the existing sequence):**
1. Right after `acquire_updater_lock()` (`:275`): `load(marker)` — live ⇒ log + return (D22: no new
   update while a marker is live).
2. After confirmed disarm (`:384-393` success): take `autostart.lock` (bounded; timeout ⇒ rearm via
   guard + return) → `create_new(marker)` (exists-and-live ⇒ rearm + return; exists-but-expired ⇒
   replace, §5.4) → write the handoff file → drop the lock BEFORE `install()`.
3. `install()` Err path: remove marker + handoff under the still-held updater lock, THEN let the
   guard rearm (r5: the writer is N−1 and can never satisfy its own removal rule; removal is exempt
   from no-rearm).

**`main.rs` startup (Windows; the head of the existing `:603-641` block):**
```
match update_marker::startup_reconcile()        // AppHandle-free core, *_at style
  Proceed        => heal_if_broken → intent-keyed rearm   (today's block, unchanged)
  Suppressed(r)  => log warn; SKIP heal AND rearm this launch
```
`startup_reconcile` (the removal transaction): load marker → Missing ⇒ Proceed. Valid ⇒ take
`autostart.lock` and hold it CONTINUOUSLY across: current-intent read → `removal_decision` →
if `Remove`: reconcile recovery to CURRENT intent (ON ⇒ idempotent `enable_crash_recovery()`;
OFF ⇒ `disable_crash_recovery()` confirmed) → remove marker+token+handoff ⇒ Proceed. If
`Suppress` ⇒ Suppressed. If `Expire` ⇒ remove files, log loudly ⇒ Proceed (the window is over —
liveness heuristic, r5). Corrupt ⇒ §5.2. Lock timeout ⇒ Suppressed (retry next launch).
This transaction is the ONLY rearm path allowed while a marker exists (the r5 exemption).
Runs regardless of `AZTEC_ACCEL_NO_UPDATE` (recon §H — L6 sets that var and a leftover marker
still needs resolving).

**`heal_if_broken_at`:** new step 0 (Windows): live marker ⇒ `Skipped("update in progress")` —
covers both `main.rs` and `repair_autostart` in one place.

**`set_enabled_at`:** ON branch, Windows, inside the already-held lock: live marker ⇒
Err("an update is finishing; try turning Start on Login on again in a moment"). OFF branch:
untouched (OFF always works — D17).

**Then:** remove `not(target_os = "windows")` from `main.rs:612`; rewrite the `:606-611` comment;
update the heal's step-4 comment (`autostart.rs:1325-1326`).

## 5. Decisions this plan makes that the ledger left open (attack these)

1. **The nonce handoff file** (§3). Alternative was teaching NSIS to parse JSON (fragile string
   scanning — rejected) or a registry value (a second registry surface for no gain — rejected).
2. **Corrupt-marker exit: delete + one-launch suppression.** A corrupt marker has no readable
   deadline, so "expire on the in-payload deadline" cannot apply. Codex's original fail-closed was
   withdrawn (bricks recovery forever); silently proceeding would let a torn write erase the
   window. Chosen: log loudly, delete the corrupt marker, suppress heal+rearm for THIS launch only.
   Same-user forgery is out of scope (D23: availability lever, documented).
3. **Deadline: `now + 15 min` at creation; future-date clamp at 24 h.** NSIS on this app installs
   in seconds; 15 min absorbs AV interference and slow disks. A payload deadline further than 24 h
   out is treated as Corrupt (a bug or forgery must not suppress for years). Constants, not config.
4. **Compare-and-create replaces an EXPIRED existing marker** (delete + one `create_new` retry);
   a LIVE one always wins (D22). Two racing updaters: one `CREATE_NEW` wins, the loser aborts.
5. **No NSIS-side reparse check** on the token destination, despite the D23 note. NSIS has no
   junction-refusal primitive; building one from `System::Call` is exactly the over-engineering
   this piece must avoid. The threat is same-user-only (they can write the token directly anyway —
   piece 1 already documents the marker family as a same-user availability lever, not a boundary).
   The RUST side stays hardened: bounded reads, `deny_unknown_fields`, nonce equality. Documented
   residual, not silent.
6. **`intent_at_disarm` is stored but the removal reconciles to CURRENT intent** — r5's lock-graph
   fix demands the current read; the snapshot is diagnostic (and keeps the guard's in-process rearm
   decisions coherent). Stated so nobody "fixes" the mismatch later.

## 6. Test plan

- **T1 — marker state table** (`update_marker.rs` unit, all OSes): payload round-trip +
  `deny_unknown_fields`; Missing/Corrupt/Valid classes; deadline expiry; future-date clamp;
  `removal_decision` truth table — token missing / nonce mismatch / version mismatch / path
  mismatch / rename-tolerance (expected absent + version match ⇒ Remove) / expired ⇒ Expire;
  compare-and-create: live loses, expired replaced, CREATE_NEW race (two writers, one wins).
- **T2 — NSIS harness** (`test:nsis` locally AND the cert-trust Windows CI step): positive
  must-fire (seed handoff `abc123` → install → `update-txn-done` contains `abc123`, handoff gone);
  no-handoff no-op (fresh install ⇒ no token file). The positive case is the `!ifmacrodef`
  typo-trap guard (recon §A).
- **T3 — Windows integration** (`tests/autostart_heal.rs`, new numbered steps in the existing
  lifecycle test): live marker ⇒ heal `Skipped` + `set_enabled_at(true)` Err + OFF still works;
  marker+token+version+path all matching ⇒ `startup_reconcile` removes, reconciles to intent (both
  ON and OFF directions), then heal proceeds; nonce-mismatched token ⇒ Suppressed; expired ⇒
  removed + Proceed. All behind the existing RAII registry guard; files under the test's temp
  `.aztec-accelerator` — the marker path helper must be injectable-by-parameter like the piece-1
  `*_at` surfaces (path parameter, not env).
- **T4 — L6 flip** (`accelerator.yml`): seed stale spaced Run value → launch installed app (no
  marker exists on a fresh install) → assert Run value == `"<installed exe>"` exactly quoted →
  F-B1 cleanup unchanged → `exit 0` kept.
- **Updater-side pure tests** (house style, closure-injected): the D22 reject-if-live check and
  Err-path cleanup ordering factored as small pure fns where practical; the full async
  `perform_update` path stays covered by T3 + L6 (no AppHandle mocking — over-engineering).

## 7. Phases & gates

| # | Work | Gate |
|---|---|---|
| 1 | `update_marker.rs` + T1 | `cargo test update_marker`; `cargo clippy --all-targets -- -D warnings`; `cargo fmt --check` |
| 2 | hooks.nsi POSTINSTALL + harness cases + CI step | `bun run --cwd packages/accelerator test:nsis` green locally (both new cases); `bun run lint:actions`; `lint:shell` |
| 3 | updater.rs + autostart.rs + main.rs wiring (incl. ungate + comment rewrites) | `cargo test`; `bun run test` |
| 4 | T3 integration steps + L6 flip | full local gate; push; **cert-trust (windows)** + **Windows Build Smoke** green on the PR |
| 5 | Review loop | ONE codex xhigh audit → fix → ONE resumed "fully closed or half-applied?" pass → merge |

## 8. Assumptions

**Facts (recon-verified, file:line in recon.md):** POSTINSTALL exists at `installer.nsi:709-711`
and fires post-copy; old uninstaller runs pre-Section; `record_pending` owns version semantics;
lock nesting updater→autostart is established; no armed-state query exists; the heal/rearm/enable
call-site census in recon §F is complete; L6's flip point is `accelerator.yml:616`.

**Inferences (challenge):** NSIS `Rename` on same volume preserves content and is
effectively atomic for this purpose; 15 min covers real installer runtimes; a corrupt marker is
rare enough that one-launch suppression is an acceptable cost; the harness's wine `$PROFILE`
faithfully models the token-handoff file semantics CI's real-Windows step re-verifies.

**Asks:** none blocking. (The goal pre-authorizes proceeding through the gate; residuals are
documented, not asked.)

## 9. Security

Same-user threat model unchanged from piece 1 (§9 there): the marker family is an availability
lever, not a privilege boundary. Controls: owner-private creation (`win_acl` CREATE_NEW,
reparse-safe) for everything RUST writes; bounded reads everywhere; `deny_unknown_fields` +
canonical round-trip parsing; in-payload deadline (mtime never trusted) with future-date clamp;
nonce equality binding token→transaction; compare-and-create so no writer clobbers a live window;
the NSIS-side reparse residual documented in §5.5. The suppression window is bounded (deadline) so
a forged marker cannot suppress updates/recovery indefinitely — and D22's "no new update while
live" cannot brick updates beyond that same bound.
