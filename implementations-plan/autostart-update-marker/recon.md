# Recon — autostart update marker (piece 2)

Phase 0.4, three read-only sweeps against `main` @ `921d5ab`. The design itself is SETTLED
(`implementations-plan/autostart-self-heal/plan.md` D18/D21/D22/D23, §3.2 Fork B, r4/r5 revisions);
this recon maps the code the settled design must land in, and surfaces the gaps between the design
and its own file-change map.

## A. The critical assumption holds — with one trap

`NSIS_HOOK_POSTINSTALL` **exists** in the exact bundler this repo builds with: `@tauri-apps/cli`
2.10.1 (`bun.lock:795`) pins `tauri-bundler 2.8.1`, whose template invokes the macro at
`installer.nsi:709-711`, `!ifmacrodef`-guarded — defining it in `hooks.nsi` is purely additive.

By the time it fires, `Section Install` has completed: main exe copied (`:624`), resources
(`:627-637`), `WriteUninstaller` (`:655`), registry (`:658-689`), shortcuts (`:698-707`). The OLD
version's uninstaller runs during the wizard pages (`PageLeaveReinstall`, `:323-359`) — **before any
Section** — so POSTINSTALL is strictly after it. In scope: `$INSTDIR` (resolves to
`$LOCALAPPDATA\{productName}` under `installMode: currentUser`), `$UpdateMode`, `$PROFILE`.

**Trap:** `!ifmacrodef` silently skips a misspelled macro. Nothing errors. The harness must carry a
POSITIVE case (token must appear) or a typo ships as a permanently-stranded marker on every update.

(Provenance: no tauri-bundler source is vendored under `~/.cargo/registry` — the app uses the npm
CLI, not a Cargo dep — so the template was read from the upstream `tauri-cli-v2.10.1` tag. Line
numbers verified against the raw file, not a summary.)

## B. Where the marker write lands in `perform_update` (updater.rs)

Sequence today: lock (`:275`) → own-version parse → layer-B re-check → size caps → download →
**Windows intent snapshot** `was_recovery_enabled = autostart::intent_enabled(app)` (`:365-376`,
Err ⇒ assume true) → **disarm** `disable_crash_recovery()` (`:383-393`; failure ⇒ rearm + return)
→ `CrashRecoveryGuard` (`:399-401`) → **`record_pending`** (`:412-428`) → `install()` (`:430`;
Windows exits inside; Err branch `:441-446` logs only, guard Drop rearms).

The marker write slots exactly where `record_pending` sits: after confirmed disarm, before
`install()`. The Err branch is where marker cleanup must be added (r5: the writer is N−1 and can
never satisfy its own removal rule).

## C. The marker must NOT own version semantics

`record_pending` (`core/updater_state.rs:153-176`) already stores the candidate version in
`updater-state.json` at the same instant, under the same lock — with a **different lifecycle**: it
is the cross-platform anti-rollback floor, cleared only by `commit_successful_launch` when the
pending version healthily launches (3 consecutive `/health` probes, `main.rs:339-366` →
`updater.rs:79-101`). The marker's `candidate` field exists ONLY for removal rule #1
(version-match); it must never be used for floor/gate decisions — `layer_b_gate`
(`updater.rs:132-149`) owns those.

Reuse-as-is: the `write_state` atomic pattern (`core/updater_state.rs:205-245` — same-dir tempfile,
0600, fsync file + parent, persist), the `LoadedState::{Missing,Corrupt,Valid}` +
`deny_unknown_fields` + canonical-SemVer-round-trip schema shape (`:40-100`), and
`win_acl::secure_create_file` (`core/win_acl.rs:306-374`, CREATE_NEW + reparse-safe + readback) for
Windows-side owner-private creation.

## D. Locks — order exists, one visibility bump needed

Established nesting: `updater.lock` (outer) → `autostart.lock` (inner) — `heal_if_broken_at` probes
updater non-blocking (`autostart.rs:1327`) then takes autostart bounded-10s (`:1332`).
`perform_update` holds `updater.lock` for the whole transaction (`:275`) and **never touches
`autostart.lock` today**; the disarm→marker-create span must acquire it (same nesting, no
inversion). `acquire_autostart_lock` is private (`autostart.rs:699`) → needs `pub(crate)`.
The lock is NOT reentrant (piece-1 lesson) — the removal transaction and the heal must be strictly
sequential, never nested.

`acquire_autostart_lock` is bounded (10s poll, `:716-731`); the removal transaction inherits that:
on timeout, log + skip (suppression persists this launch, retried next launch) — never block startup.

## E. Three gaps between the settled design and its own file map (this plan closes them)

1. **"Explicit ON is rejected while a marker is live" has no wiring.** `set_enabled_at`'s ON branch
   (`autostart.rs:1395-1459`) is a third marker-aware call site (reachable via the ungated
   `set_autostart` command); the check must live INSIDE its locked section or it is a TOCTOU. Not in
   the settled file map.
2. **The startup rearm is a second suppression target.** `main.rs:625-640` (`intent_enabled` →
   `enable_crash_recovery`) runs on every platform, unconditionally, and is NOT reached through
   `heal_if_broken_at` — a marker check inside the heal alone leaves "no process rearms" false.
   Startup ordering must be: marker reconciliation FIRST, then (only if no live marker) heal → rearm.
3. **`acquire_autostart_lock` visibility** (D above).

Also confirmed: there is NO read-only "is the task armed?" query — reconcile-to-intent is built from
the two existing primitives only: idempotent `enable_crash_recovery()` for ON
(`crash_recovery.rs:381-426`), `/Query`-confirmed `disable_crash_recovery()` for OFF (`:432-459`).
No new query primitive is needed or wanted.

## F. Call-site census (complete, grep-verified)

- `heal_if_broken`: `main.rs:613` (gated) + `commands.rs:69` (`repair_autostart`, ungated, all
  platforms). A marker check inside `heal_if_broken_at` (new step 0, Windows-only) covers both.
- `enable_crash_recovery`: `main.rs:632`, `autostart.rs:1452` (enable transaction), `updater.rs:459`.
- `disable_crash_recovery`: `main.rs:393` (Quit, Windows), `autostart.rs:1412,1458,1464`,
  `updater.rs:384`.
- `acquire_updater_lock`: `autostart.rs:1327`, `updater.rs:91,275`.
- The heal's updater-lock bow-out comment (`autostart.rs:1325-1326`) already says "Piece 2 adds the
  Windows post-install marker on top of this."

## G. Test/CI surfaces

- **Harness** (`nsis/harness.test.nsi` + `scripts/nsis-hook-test.sh`): includes the REAL hooks.nsi,
  never reimplements; driver seeds/observes files under a wine `$PROFILE`. A POSTINSTALL case slots
  in as `!insertmacro NSIS_HOOK_POSTINSTALL` in the harness's install Section. `test:nsis` is
  local-only; the CI enforcement point is the cert-trust Windows step (`accelerator.yml:179-226`) —
  new hook cases must be added THERE too or they gate nothing.
- **L6 flip point**: `accelerator.yml:616` (`if ($after -cne $staleValue)`) — comments at `:591-595`
  already name piece 2 as the flip. Installed exe found under `$env:LOCALAPPDATA` (`:585`). F-B1
  cleanup order (`:621-628`) and the explicit `exit 0` (`:638`) must survive the flip.
- **Windows integration test** (`tests/autostart_heal.rs:628+`): numbered steps 0–6 with an RAII
  registry guard; marker-suppression assertions extend it (natural slot: after step 4's positive
  heal). Updater unit-test house style: closure-injected `Cell` counters (`updater.rs:509-568`);
  marker state-machine style: real tempdir + truth tables (`core/updater_state.rs:247-396`).
- **Webdriver spec is unaffected**: `e2e-webdriver/autostart.spec.ts` skips win32 entirely and
  depends only on the `webdriver` half of the heal gate, which piece 2 does not touch.

## H. Startup order (main.rs, for the reconciliation insertion)

`.setup()`: dock-hide → status item → **heal+rearm block (`:603-641`)** → tray → animation →
desktop state → HTTPS → server → **update poller + floor tracker last (`:751-756`, async, ~9s-2min
later)**. Marker reconciliation inserts at the head of the heal+rearm block. `AZTEC_ACCEL_NO_UPDATE`
kills polling (`main.rs:216-219`) but must NOT kill marker reconciliation (a marker left by a
previous run still needs resolving — and L6 sets that env var).
