# Phase 2 — #96: crash-recovery-armed updater-smoke (Level 1)

## What shipped (updater-smoke-windows.ps1)
The positive leg now runs the update WITH crash-recovery armed, exercising the disarm-before-install
guard (updater.rs, hardened over 5 codex rounds on #275) — the one update interaction unique to
Windows (linux/mac recovery keys on exit code; no install-race).
1. ARM (positive leg only, after install N-1, before launch): write the single Run key
   `HKCU\...\Run\"Aztec Accelerator"` = quoted exe path. Verified sufficient via auto-launch-0.5.0
   source: is_enabled() = Run-key-exists && task_manager_enabled.unwrap_or(true); a missing
   StartupApproved\Run entry → None.unwrap_or(true) → true. So N-1's startup enable_crash_recovery()
   registers the task. (set_autostart command does this + enable_crash_recovery, but it's IPC — can't
   drive from PowerShell.)
2. ASSERT (positive success path): after /health==N, schtasks /Query the task must succeed (re-armed)
   — proves the guard disarmed-then-rearmed, not leaked/left-off. (In-install-window absence too brief
   to observe; the re-armed end-state is the durable signal.)
3. CLEANUP (finally): Remove the Run key + schtasks /Delete the task (an armed task must not leak).

The release pipeline's positive Windows smoke auto-inherits the arming (same ps1). Negative leg
unchanged (it rejects before install — nothing to arm).

## Validation
_e2e-updater-windows.yml is workflow_call + workflow_dispatch (on main). Validate the armed positive
leg by dispatching it on this branch: `gh workflow run _e2e-updater-windows.yml --ref feat/smoke-crash-recovery-armed`.
