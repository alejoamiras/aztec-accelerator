# Phase 5 lessons — integration sweep + full E2E

- `scripts/brand-sweep.test.ts` (root, runs in `test:scripts`): 4 tests — retired visual values
  gone from 18 product surfaces (word-boundary/quoted patterns; bare `Inter` deliberately not
  matched); frozen identity literals byte-present at 11 sites; no "Presto" in identity-bearing
  files (incl. every trust backend); no "Presto" in any workflow. All green on first run — the
  per-phase discipline held.
- `packages/accelerator/scripts/run-webdriver-local.sh` (committed, shellcheck-clean): display
  preflight (owned Xvfb/DBus/stalonetray when available), port preflight 4445/59833 with owning-pid
  report, webdriver-featured build, owned-pgid launch with log capture, readiness wait, WDIO run,
  trap teardown of owned pgids only.
- **WebDriver run: RESOLVED 2026-08-27 — no longer deferred.** Owner approved installing the
  display stack (`xvfb stalonetray dbus-x11`, 3 packages; every X lib was already present from the
  Tauri build deps). `run-webdriver-local.sh` then went green first try: **22 passing across all 6
  spec files, exit 0** — 3 smoke, 3 settings, 5 trust-boundary, 6 theme (the Mac agent's new spec,
  first execution anywhere), 2 auth-flow, 3 autostart. Confirms the renamed `"Presto Settings"`
  title constants against the real binary. Teardown left no Xvfb/stalonetray/app processes.
  The Mac's WDIO `UND_ERR_INVALID_ARG` remains a Mac-local runner defect, not a suite problem.
- Original deferral record (superseded, kept for the trail): **DEFERRED-WITH-CONSENT.** The launcher exited 2 as designed: this host has no
  DISPLAY/WAYLAND_DISPLAY and no Xvfb/stalonetray installed (verified via preflight AND directly).
  System packages are not installed autonomously per plan. Options for the owner (wrap-up report):
  (a) `sudo apt install xvfb stalonetray dbus-x11` then `packages/accelerator/scripts/
  run-webdriver-local.sh`; or (b) the 17-test WebDriver suite becomes the first CI check at
  PR-flip (`_e2e-webdriver.yml` runs it on macOS + Linux). The three renamed title constants are
  covered either way; they were updated in the same commit as `windows.rs`.
- Gate: `frontend:build` ✓ · root `bun run test` ✓ (80 tests final leg — sweep included;
  lint+typecheck+unit across all packages; tauri-identity UNMODIFIED) · `test:e2e:ui` 74 pass ✓ ·
  WebDriver: deferred-with-consent record above.
