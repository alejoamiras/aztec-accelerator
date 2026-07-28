# Implementation Plan

## 1. Architecture and implementation

I would remove `tauri-plugin-autostart`, not work around it. Once we need three readers and safe writers, the plugin contributes only unsafe serialization, dishonest status, and a Linux panic. SMAppService and a launcher helper are larger product/distribution changes and do not solve the cross-platform problem consistently.

Add `packages/accelerator/src-tauri/src/autostart.rs` with this public surface:

```rust
pub struct AutostartStatus {
    pub enabled: bool,
    pub condition: AutostartCondition,
}

pub enum AutostartCondition {
    Disabled,
    Healthy,
    DifferentInstall,
    MissingTarget,
    Malformed,
}

pub fn inspect(app: &tauri::AppHandle) -> Result<AutostartStatus, AutostartError>;
pub fn reconcile_on_launch(app: &tauri::AppHandle) -> Result<ReconcileOutcome, AutostartError>;
pub fn set_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<(), AutostartError>;
pub fn intent_enabled() -> Result<bool, AutostartError>;
```

Internally, separate pure codecs from OS I/O and inject a `PlatformStore` in tests. Preserve raw snapshots so an explicit enable transaction can restore the exact previous artifact if crash-recovery arming fails.

Desired path:

- macOS: `current_exe().canonicalize()`, matching the existing plugin.
- Linux: canonicalized `app.env().appimage` when present, otherwise canonicalized `current_exe()`.
- Windows: canonicalized `current_exe()`.
- Require absolute UTF-8, no controls/DEL, canonicalization success, and a regular executable file before writing.

An artifact is:

- healthy-same: stored target resolves and canonicalizes to desired;
- healthy-different: stored target resolves to another executable;
- stale: parsing succeeds but target is missing/non-executable;
- malformed/unknown: parsing or non-NotFound I/O failed.

Only stale is auto-healed. A healthy-different entry is not silently stolen by whichever copy launched last.

Platform implementations:

- macOS: use direct `plist = "1.8"` dependency. Read `ProgramArguments[0]`; update that value through an atomic same-directory temp-file rename. Preserve `KeepAlive`, `ThrottleInterval`, and unknown keys. Fresh enable creates a structured plist with XML escaping.
- Linux: use `${XDG_CONFIG_HOME:-$HOME/.config}/autostart/Aztec Accelerator.desktop`; never unwrap HOME/config resolution. Strictly parse the desktop entry’s single `Exec` token. Write a quoted/escaped Desktop Entry argument, including `%%` for literal `%`.
- Windows: use direct `winreg` access. Read the Run `REG_SZ`, require the owned format `"absolute path"` with no trailing arguments, and retain the plugin’s `StartupApproved` interpretation. Write a fully quoted command. Startup repair never changes `StartupApproved`; an explicit ON does.

Add `coordination.rs` containing a nonblocking cross-process `desktop-state.lock`. Replace the updater-private lock with this shared lock for updater state, autostart mutations, and crash-recovery mutations.

Startup reconciliation replaces `packages/accelerator/src-tauri/src/main.rs:607` and is disabled under `feature="webdriver"`:

1. Acquire the shared lock; if busy, log and skip.
2. Apply the Windows updater-disarm gate below.
3. Inspect the artifact.
4. Healthy-same: arm crash recovery.
5. Stale: re-read under the lock, patch to desired, then arm.
6. Healthy-different, disabled, malformed, or unknown: do not write or arm.

## 2. The six hard parts

1. **macOS plist recreation:** never call plugin `enable()`. Targeted structured mutation preserves `KeepAlive`. Keep the current `enable_crash_recovery()` early-return: after reconciliation it is safe and genuinely idempotent. Ungate/refactor its plist transform for Linux-hosted tests.

2. **No stored-path API:** own strict plist, desktop-entry, and registry readers. No legacy-format migration or permissive guessing.

3. **Redundant instances:** the shared lock serializes reads/writes, and a second compare occurs under it. More importantly, repair happens only when the stored target is unresolved. Once one valid process repairs it, another path sees healthy-different and cannot flip it back. An unlinked/moved old process cannot pass desired-path canonicalization.

4. **Updater disarm window:** a lock alone is insufficient because Windows `install()` exits while external NSIS continues. After confirmed disarm and before `install()`, atomically write `update-disarmed.json` containing candidate version and expected installed executable path. Old/path-mismatched processes must not heal or rearm while it exists. The matching new build reconciles, successfully rearms, then removes it. Returned install failures rearm first and remove the marker through the existing guard. Corrupt markers fail closed. Explicit OFF remains available after acquiring the lock; ON is rejected while a foreign marker is active.

5. **Serialization/canonicalization:** no raw interpolation. Plist is structured, Linux uses Desktop Entry escaping, Windows always quotes argv0. Canonicalize both paths for identity; lexical differences and symlinks do not trigger repair. A resolving alternate installation remains untouched.

6. **Linux directories:** eliminate the plugin’s `home_dir().unwrap()`. Honor `XDG_CONFIG_HOME`, fall back to `$HOME/.config`, and return an error when neither resolves. Tests set both HOME and an XDG decoy/override deliberately.

## 3. Status honesty and Settings

`get_autostart_enabled` should return the structured `AutostartStatus`, not a bare plugin boolean.

- Missing target or malformed entry: `enabled=false`.
- Valid target: `enabled=true`.
- I/O/coordination uncertainty: command error; the existing disabled-switch “state unavailable” treatment remains.
- `DifferentInstall`: checked, with “Start on Login points to another copy. Turn it off and on to use this one.”
- `MissingTarget`/`Malformed`: unchecked, with “Start on Login needs repair. Turn it on to repair.”

Opening Settings must never write OS state. Startup may already have healed successfully, in which case Settings honestly reports true. If healing failed, false plus an actionable repair is better than hiding the failure by retrying during a read.

## 4. Test plan

Pure, containerized unit coverage:

- Plist round trips with spaces, Unicode, `& < > "`, nested dictionaries, malformed arrays, and exact preservation of recovery/unknown keys.
- Linux Exec encode/decode for spaces, quotes, backslashes, `$`, backticks, `%`, Unicode, controls, duplicate Exec fields, and field-code injection.
- Windows command encoding/parsing, trailing-argument rejection, and StartupApproved enabled/disabled fixtures.
- State table covering absent, valid symlink, dangling link, directory, non-executable file, permission error, malformed artifact, and healthy-different.
- Closure-injected reconciliation races: lock busy, CAS changed underneath, two desired paths, invalid current path, failed atomic write.
- Snapshot rollback and updater-marker state machine, including every early return.

Add `test:autostart:container`, running pure codecs and Linux real-filesystem tests in Docker. It also uses Wine `reg.exe` to prove quoted Run-value and StartupApproved mechanics. This is not misrepresented as Rust-backend coverage.

Native ignored integration tests:

- `autostart_linux`: isolated HOME/XDG, real `.desktop` enable→stale→heal→disable.
- `autostart_macos`: isolated HOME, real plist repair preserving KeepAlive.
- `autostart_windows`: unique HKCU value with RAII cleanup; real stale repair, disabled override preservation, explicit-enable reset, quoting, and marker suppression.

Extend Windows Build Smoke: seed the production Run value with a missing spaced path before launching the installed app, then assert it becomes the exactly quoted installed executable and resolves. Extend the release-only updater smoke to assert the disarm marker survives old-process attempts and is removed only after the new version rearms.

The unresolved Wine Rust exit-53 issue is not a merge gate; native `windows-latest` tests production code. A separate WineHQ spike may later move that native test into Docker.

## 5. Phases and gates

1. **Owned codecs/store:** remove plugin; add readers, writers, classifiers.

   ```bash
   cd packages/accelerator/src-tauri
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo test autostart -- --nocapture
   ```

   Pass: all serializers round-trip exact targets; malformed/injection fixtures write nothing.

2. **Coordination, updater marker, startup and Settings integration.**

   ```bash
   cargo test
   bun run --cwd packages/accelerator frontend:build
   bun run --cwd packages/accelerator test:e2e:ui
   bun run --cwd packages/accelerator test:autostart:container
   ```

   Pass: race/rollback/marker tables green; Settings distinguishes disabled, repairable, alternate-copy, and unknown.

3. **Native OS proof and CI wiring.** Add bare `cargo test` to the macOS matrix leg, plus each ignored integration test on its matching runner.

   ```bash
   gh workflow run accelerator.yml --ref <branch>
   ```

   Pass: Clippy, Rust Tests, Cert Trust matrix with autostart integrations, Desktop UI Tests, Windows Build Smoke, and final Accelerator Status all succeed. `_e2e-updater-windows.yml` is release-only `workflow_call`; it is not claimed as a PR/manual gate.

## 6. Assumptions

**Facts:** the plugin cannot read stored paths; its serializers are unsafe; macOS recreation strips KeepAlive; startup precedes redundancy detection; updater disarms recovery; Windows native PR CI exists.

**Inferences:** “path resolves” means a canonicalizable regular executable; valid alternate copies are functional and should not be auto-repointed; matching candidate version plus expected installed path is sufficient evidence that NSIS finished.

**Asks:** none blocking. Later decisions are whether to adopt SMAppService and whether maintaining a WineHQ Rust image is worth its cost.

## 7. Security and adversarial considerations

Reject unsafe encodings before mutation; never invoke a shell; bound artifact reads; reject symlink/non-regular artifact files; use owner-private coordination state and atomic writes; preserve Task Manager OFF; treat malformed state and markers as unknown, not disabled; avoid logging full user paths. Same-user malware can already alter HKCU and user startup files, so this does not claim a privilege boundary—but it must not turn malformed attacker input into command-line or plist injection.