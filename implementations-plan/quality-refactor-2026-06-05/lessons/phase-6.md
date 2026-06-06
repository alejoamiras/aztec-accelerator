# Phase 6 — Crash-recovery trait (Q4) [SAFETY-CRITICAL] — research + design

## Key finding: the safety-critical ordering is OUTSIDE the refactor scope
The #96/#97 **disarm-before-install ordering** lives in `updater.rs::perform_update` (L86-119) and is
**entirely `#[cfg(target_os = "windows")]`-gated** — it calls the *free* `disable_crash_recovery()`
(Windows → bool), aborts+rearms if disarm can't be confirmed, then installs, then rearms. macOS/Linux
have **no disarm dance** at install (launchd KeepAlive / systemd don't have the Windows always-armed-
repeating-task-ticks-during-NSIS-mutation race).

**Therefore the Q4 trait extraction leaves `updater.rs` UNTOUCHED.** The brick-risk ordering is not
refactored at all — it keeps calling the free `disable_crash_recovery()`. This de-risks Q4 massively vs
the plan's "preserve the ordering byte-for-byte" framing (there's nothing to preserve in the refactored
file — the ordering is in a file we don't touch).

## Current platform-divergent surface (crash_recovery.rs, 452 lines)
- `enable_crash_recovery()`: macOS L23 (plist KeepAlive), Linux L93 (systemd), Windows L216 (schtasks).
- `disable_crash_recovery()`: macOS L76 → `()`, Linux L163 → `()`, **Windows L281 → `bool`** (the
  /Delete-then-/Query-confirm; returns true if never armed).
- Callers: updater.rs:92 (`if !disable()`, **windows-cfg only**), commands.rs:47/50 (set_autostart),
  main.rs:276/294. The mac/linux callers use `disable()` as a statement (ignore any return).

## Design (Extract Interface — behavior-preserving)
```rust
pub trait CrashRecovery { fn enable(&self); fn disable(&self) -> bool; }
#[cfg(target_os="macos")]  pub struct PlatformRecovery; // + impl: bodies verbatim, disable appends `true`
#[cfg(target_os="linux")]  pub struct PlatformRecovery; // + impl
#[cfg(target_os="windows")] pub struct PlatformRecovery; // + impl: disable returns the bool verbatim
// free fns become thin dispatch; disable_crash_recovery() now -> bool on ALL platforms
pub fn enable_crash_recovery()  { PlatformRecovery.enable() }
pub fn disable_crash_recovery() -> bool { PlatformRecovery.disable() }
```
- **Unify the free `disable` to `-> bool` everywhere** (mac/linux now return `true`). Caller-compatible:
  the mac/linux call sites ignore the return (statement position; `bool` isn't `#[must_use]` → no clippy
  warning). updater.rs (windows) already used the bool.
- Bodies move **verbatim** → behavior-preserving.

## Test safety net
- **Generation** pinned by existing `task_xml_uses_repeating_trigger_and_escapes_exe` (windows, asserts
  NOT `<RestartOnFailure>`) + `patch_plist_*` (macOS). These survive the move (bodies unchanged).
- **Ordering** (disarm→install→rearm) is NOT a unit test — it's the **updater-smoke CI gate** (the
  `_e2e-updater-windows.yml` from #96/#97). Since updater.rs is untouched, this gate's behavior is
  unchanged; the **1.0.x-rc dry-run** is the validation per the /goal.

## Validation gap (why this is a careful pass, not a deep-tail crank)
Only macOS compiles locally; Windows + Linux trait impls compile **only via CI** (accelerator gate runs
all three). A cfg typo in the windows/linux block is CI-caught (~15 min), not local — so implement the
3 blocks carefully, `cargo check` macOS locally, then lean on the gate. Then **rc dry-run** before any
stable cut (blocking updater-smoke green).

## Plan
1. Implement the trait + 3 ZST impls + dispatch (verbatim body moves).
2. `cargo test --lib` + `clippy` (macOS local) → PR → accelerator gate (all 3 platforms) green.
3. rc dry-run (autonomous-allowed; rc only) validates the Windows updater path.
LESSONS_FILE=implementations-plan/quality-refactor-2026-06-05/lessons/phase-6.md
