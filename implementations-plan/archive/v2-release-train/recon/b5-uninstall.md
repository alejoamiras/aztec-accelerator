# Recon: B5 — uninstall/cleanup lifecycle

Agent: sonnet Explore, 2026-08-16, tree @ 0c351bc.

## Headline structural findings

1. **Hook-ordering problem**: NSIS_HOOK_POSTINSTALL fires as the LAST act of Section Install
   (harness.test.nsi:34-36, hooks.nsi:61-64); by symmetry POSTUNINSTALL almost certainly fires
   AFTER `RMDir /r $INSTDIR` — the exe is already gone, which is likely WHY the existing hook
   shells certutil directly instead of calling the app's own `--remove-ca-trust`. Not 100% proven
   in-repo (bundler template not vendored) — load-bearing open question.
2. **Ratified precedent AGAINST a new uninstall hook**: audit report.md:265-268 — "F-05: no new
   NSIS_HOOK_PREUNINSTALL. A release cannot fix its own uninstaller, so a NEW uninstall hook is a
   one-shot bet. The existing POSTUNINSTALL was hardened instead." PREINSTALL macro confirmed to
   exist in template (smoke-updater-windows.yml:99-104); PREUNINSTALL almost certainly exists too
   and would fire with exe alive. **THE B5 design fork**: NSIS-native inline cleanup (mirror the
   certutil pattern: schtasks /Delete + reg delete Run value) vs new PREUNINSTALL invoking
   `"$INSTDIR\AztecAccelerator.exe" --prepare-uninstall`.

## hooks.nsi inventory (168 ln, both macros !ifmacrodef-guarded — misspelling silently no-ops)

- POSTINSTALL :75-86: update-txn → update-txn-done rename handoff (idempotent by Rename semantics).
- POSTUNINSTALL :101-167: guard = 8.3-canonicalized $EXEDIR != $INSTDIR AND $UpdateMode<>1
  (:108-124; EXEDIR check is the load-bearing one); certutil -user -delstore via absolute
  $SYSDIR path :128; RE-QUERIES store :134 rather than trusting exit code; failure → writes
  CA-TRUST-NOT-REMOVED.txt with certmgr.msc instructions :141-157 (stale-file cleared first :126);
  `RMDir /r $PROFILE\.aztec-accelerator\certs` :159 — the ONLY fs cleanup. NOTHING touches
  Run/StartupApproved, scheduled task, locks, updater-state, markers.
- House invocation style: `ExecWait '"<abs-path>" <args>' $out` (:128,134).
- harness.test.nsi: includes hooks.nsi verbatim (:23), real makensis build, drives uninstall.exe
  under 3 command lines on windows-latest (accelerator.yml:199-324); its Section Uninstall does
  ONLY the macro — can't prove ordering either.

## CLI flag surface (main.rs)

- Only `--remove-ca-trust` exists (:514-539): FIRST statement of main(), argv .any(), no Tauri
  runtime, exit(1) if report.removal_incomplete() (scripted-caller contract), else return before
  tray/server. `--prepare-uninstall` slots identically.
- NO single-instance plugin/mutex anywhere; dedup = bind-fail → probe_and_identify → may_bow_out
  (D-ITEM7 polarity), lives in .setup(), unreachable from early-argv branch. A --prepare-uninstall
  process and a running tray are unsynchronized except per-function file locks.
- AppHandle-free reusable fns: autostart::set_enabled_at(None,false) (:1891-1981, takes
  autostart.lock, bounded 10s), autostart::remove_entry() (:1870-1873),
  crash_recovery::disable_crash_recovery() (:34-36, NO lock), trust::remove_ca_trust(&Path)
  (trust/mod.rs:176-178, NO cross-process lock).

## Per-platform cleanup that exists

- autostart OFF branch (:1968-1980): backend::remove() (idempotent NotFound⇒Ok) then
  disable_crash_recovery() (error if unconfirmed).
  macOS :844-851 deletes LaunchAgents plist (SAME plist carries KeepAlive — crash-recovery disable
  on macOS is an unconditional no-op returning true, crash_recovery.rs:187-193, relies on plist
  deletion). Linux :947-954 deletes XDG autostart .desktop. Windows :1090-1105 deletes HKCU Run
  value; deliberately does NOT touch StartupApproved on removal (:1094-1096 "never CREATE the key
  just to delete from it" — harmless residue).
- crash_recovery disable: Linux :331-360 systemctl --user disable + rm unit + daemon-reload;
  Windows :464-492 schtasks /Delete via absolute path (:399-410) with 3× retry + /Query re-verify
  + 200ms backoff — correctness-critical (:466-469: task relaunches app 1 min after quit).
- trust removal (post-F-05, all idempotent + error-honest, return TrustReport with
  removal_incomplete()): windows delete_by_cn + re-query tri-state Probe (fail-closed Unknown);
  macOS ≤64× delete_by_sha1 loop + trust-settings clear; Linux delete_all_ours across nssdb +
  Firefox profiles (unreadable DB ⇒ still-installed).
- commands::remove_https_trust (Settings button) needs running app + in-process
  claim_https_lifecycle tokio::Mutex — NOT reusable from CLI, and that mutex is invisible
  cross-process (pre-existing race, inherited not introduced).

## Packaging / docs

- tauri.conf.json: targets "all"; deb block = desktopTemplate + depends libnss3-tools ONLY — no
  postrm/prerm surface; no [package.metadata.deb] in Cargo.toml. macOS: no uninstall-hook concept
  (DMG = draggable .app). NSIS is the ONLY OS with hooks (installerHooks :71-74).
- RATIFIED prior decision (https-by-default plan.md:130 S6): "a root postrm can't safely walk user
  NSS DBs — out" → deb/DMG/AppImage cleanup is DOCUMENTED/SCRIPTED, not hook-automatic. Matches
  brief wording.
- Binary: [[bin]] name AztecAccelerator (Cargo.toml:22-28); MAIN_BINARY_EXE const
  update_marker.rs:536.
- Uninstall docs today: README:290-292 CA-trust-only; PLATFORM_SUPPORT.md:25-29,48 same scope;
  RELEASE_RUNBOOK zero mentions; no scripts/ helpers.

## Test shapes

- L3 pattern to extend: tests/autostart_heal.rs — per-OS #[ignore], real OS artifacts, throwaway
  $HOME, --test-threads=1 (HKCU is per-user global). A prepare-uninstall test: enable everything →
  invoke → assert gone → invoke again → assert still gone.
- NSIS side: harness.test.nsi + accelerator.yml:199-324 (only place macros EXECUTE).
- GAP: --remove-ca-trust argv branch NEVER tested end-to-end (no assert_cmd/CARGO_BIN_EXE
  precedent in crate; trust tests call the library fn). New flag needs to establish the pattern
  or accept same gap.
- CI cfg matrix: ubuntu `test` job = bare cargo test (linux-gated only); cert-trust legs run
  --ignored integration on 3 OSes; macOS leg ALSO bare cargo test (:188-189, only place macOS unit
  tests run); windows-build job (:626-647) = where Windows-gated unit tests actually execute.
  **No macOS/Linux uninstall CI surface exists at all** (inputs/03 P0: install covered 3 OSes,
  removal 1).

## Collision risks

1. Running tray vs CLI process: only autostart.lock serializes (bounded, surfaced error);
   crash-recovery + trust ops have NO cross-process lock (trust race pre-existing). Consider
   checking updater.lock before mutating (mirror heal's Skipped("updater active") pattern) — gap,
   not implemented.
2. **Crash-recovery relaunch mid-uninstall (Windows, real)**: PT1M task relaunches anything not
   running; disarmed ONLY by in-app Quit menu (main.rs:414-429 quit_disarm). Uninstall via
   Add/Remove while app open (or after force-terminate) → task survives today; hooks.nsi never
   touches it. THE bug B5 fixes. Ordering: PREUNINSTALL-time call has exe alive; POSTUNINSTALL-time
   only NSIS-native schtasks /Delete is reachable.
3. **Copied-instance (#429)**: implicit_arm gates (autostart.rs:1699-1776) protect ARMING; but
   backend::remove() deletes UNCONDITIONALLY — one artifact slot per OS (Run value/plist/.desktop/
   unit keyed by APP_NAME) — prepare-uninstall run for copy A strips a surviving copy B's
   autostart. Unguarded risk SPECIFIC to this entrypoint; plan must decide (ownership check before
   delete vs accept+document).
4. Update-marker interplay: set_enabled_at consults live_marker under lock (inherited free); a
   DIRECT disable_crash_recovery() call bypasses the marker check — uninstall-mid-update-window
   unmapped. NSIS $INSTDIR file-lock makes update+uninstall mutually exclusive per-install, but
   ~/.aztec-accelerator markers are shared user state.
5. **State-dir scope precision**: ~/.aztec-accelerator holds certs/ (remove), locks (leave),
   updater-state.json (monotonic floor — affects future reinstall; decide+document), update markers
   (clear), config.json + approved origins (USER data — brief scope does NOT include deleting; a
   careless RMDir /r of the whole dir would destroy it). Enumerate explicitly.
