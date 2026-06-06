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

## Concrete implementation plan (bodies read — ready for a focused pass)
**Structural finding:** on each platform `enable` and `disable` are NOT contiguous — a helper sits
between them (macOS `patch_plist_with_keepalive` L55-71 + `macos_plist_path` L82-88; linux inline;
windows `schtasks_exe`/`task_xml`/`xml_escape`). So the impl block can't just wrap a line range.

**~8 edits, all behavior-preserving (bodies verbatim):**
1. Trait def at top (platform-agnostic): `pub trait CrashRecovery { fn enable(&self); fn disable(&self) -> bool; }`.
2-4. Per platform: replace `pub fn enable_crash_recovery() {<body>}` with
   `pub struct PlatformRecovery;` + `impl CrashRecovery for PlatformRecovery { fn enable(&self){<enable body>} fn disable(&self)->bool{<disable body>; true /*mac,linux*/} }`,
   pulling the disable body up into the impl. Helpers stay as free fns after the impl.
5-7. Per platform: delete the old standalone `disable_crash_recovery` fn (its body moved into the impl).
8. Non-cfg dispatch free fns (signatures preserved; disable now `-> bool` on ALL platforms):
   `pub fn enable_crash_recovery() { PlatformRecovery.enable() }` /
   `pub fn disable_crash_recovery() -> bool { PlatformRecovery.disable() }`.
   (`PlatformRecovery` is the per-platform cfg'd ZST; the trait is in-module so methods resolve.)

**Callers unchanged** (commands.rs:47/50, main.rs:276/294, updater.rs:92-windows). The mac/linux
callers ignore the now-`bool` return (statement position; not `#[must_use]` → no clippy warning).
Windows `disable` already returned the bool. **updater.rs untouched → disarm ordering preserved.**

**Validation:** `cargo test --lib` (macOS: `task_xml`/`patch_plist` generation tests guard bodies) +
`cargo check` macOS local → PR → accelerator gate compiles windows+linux → **rc dry-run** (Windows
updater-smoke) before any stable cut.

**Design note (resolved):** plan's "free fns → thin dispatch [to ZST]" chosen over trait-wraps-fns —
the former makes the free fns the trait's consumer (not a dead abstraction). No codex needed.
LESSONS_FILE=implementations-plan/quality-refactor-2026-06-05/lessons/phase-6.md
