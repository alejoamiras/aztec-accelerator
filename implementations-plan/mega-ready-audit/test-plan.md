# Test-suite improvement — mega-ready-audit

**Date:** 2026-08-21 · Baseline inventory in [../recon.md](../recon.md).

## Implemented this engagement

- **`win_acl.rs` inline unit tests** (commit `7d71688`) — first direct tests for the 402-LOC
  security module: owner-only DACL readback on create-dir/create-file, child-inheritance of the
  owner-only ACE, pre-planted file/dir rejection, reparse (symlink) rejection with
  privilege-tolerant skip, harden-existing idempotence. Run by the existing `windows-build`
  CI lane (`cargo test` both crates); Windows-target compile verified locally via
  `cargo check --target x86_64-pc-windows-gnu --all-targets`.
- **CLAUDE.md test counts corrected** — were stale by ~2 months and ~3×: 12→17 WebDriver,
  35→66 Playwright, ~115→431 Rust, ~104→~280 TS.

## Prioritized roadmap (not implemented here — each is its own arc)

| Prio | Item | Why | Where |
|---|---|---|---|
| P1 | Updater tamper-negative (rejection) smoke legs for linux-x86_64 + darwin-x86_64 | Tamper-rejection currently proven only on macOS-arm64 + Windows; a feed/signing regression on the other two platforms ships silently | `_e2e-updater.yml`, `_e2e-updater-linux.yml` |
| P1 | Playwright desktop-ui matrix → add windows-latest | The consent UX (the most security-sensitive UI) is UI-tested on ubuntu only; Windows webview quirks untested | `accelerator.yml` desktop-ui job |
| P2 | Router/auth wiring direct tests (`core/server.rs` assembly, per-port host guard) | Only exercised indirectly through HTTP-level tests today | core/server/tests.rs |
| P2 | Crash-recovery CI trigger widening | Task Scheduler crux fires only when its own script changes, not on `crash_recovery.rs` edits | paths-filter in `_e2e-crash-recovery-windows.yml` |
| P3 | macOS trust-INSTALL path in CI | Interactive Keychain prompt blocks headless runners; needs a self-signed-harness approach like the Windows consent work | new ignored suite |
| P3 | tray.rs / tls.rs / trust/macos.rs inline tests | Small, mostly-glue modules; lowest marginal value | src-tauri |

## Deliberately NOT added

Broad characterization tests around already-audited code (consent flow, updater transaction) —
the existing mutation-proven suites from #434–#446 already pin those behaviors; duplicating them
adds maintenance without new failure detection.
