# AFK run — take the updater gate to green on a dry-run

**Started**: 2026-05-29, user AFK.
**Authorization**: user explicitly instructed "run the dry-run … take this into its final stage." That supersedes the default AFK hard-limit on release tags **for prereleases only**. A `1.0.3-rc.N` dispatch is safe: prereleases skip the S3 `latest.json` upload (`is_prerelease == 'false'` gates), are `--prerelease --latest=false`, don't bump-source, don't npm-publish, and the updater gate's feed is local-only. **A stable `1.0.3` will NOT be cut AFK** — that touches the prod updater feed; stop + report if that's ever the apparent next step.

## Goal (definition of done for AFK)

`update-smoke` (advisory macOS gate, both arches) **green on a real `1.0.3-rc` dry-run** — proving install-N-1 → auto-update-to-N → relaunch → `/health.version == N` works end-to-end on hosted macOS runners. Then report.

## Plan

1. Merge #239 when its (lint-only) PR-gate is green.
2. Trigger `release-accelerator.yml` with `version=1.0.3-rc.1` (N=1.0.3-rc.1, N-1=1.0.2 stable).
3. Watch the run. `update-smoke` is advisory (not in `tag.needs`), so the release completes regardless; I only care whether the two `update-smoke` legs go green.
4. On failure: pull the job log, diagnose (codex consult if non-trivial), fix `updater-smoke.sh` / `updater-feed-server.ts` / `_e2e-updater.yml` on a new branch → PR → merge on green lint → re-trigger with the next rc (`1.0.3-rc.2`, …). Log each attempt below.
5. **Bound: after 3 failed dry-run attempts on the same root cause, stop and reassess/report** (per CLAUDE.md).
6. When macOS green: report. PR2 (Linux AppImage + negative test + flip macOS to `tag.needs` blocking) is a release-gate change — prepare/note it but leave the "make it block" decision for the user's return unless clearly safe.

## Anticipated failure modes (from the plan's open questions)

- Gatekeeper on the `gh`-downloaded notarized N-1 launched headlessly (mitigation in script: `xattr -dr com.apple.quarantine`).
- `app.restart()` not relaunching cleanly in CI (tray-only, no Dock).
- Timing: download+verify+swap+relaunch within the 300s poll.
- `:443` bind / `security add-trusted-cert` / hosts perms on hosted runners.
- The local CA not honored by the updater's TLS (mitigated: rustls-platform-verifier uses OS trust store — but unproven in practice).

## Attempt log

(filled in as the dry-runs run)

### Attempt 1 — 1.0.3-rc.1 (run 26648432364)
- Triggered after #239 merged (cc4a8cb). N=1.0.3-rc.1, N-1=1.0.2.
- update-smoke is advisory; watching the two macOS legs (darwin-aarch64, darwin-x86_64).

**Attempt 1 result (run 26648432364): FAIL — but proved the approach.**
- ✓ TLS impersonation works: N-1's updater hit the local feed via hosts+trusted-CA and logged "Update available current=1.0.2 new=1.0.3-rc.1". Option A validated.
- ✓ auto_update:true drove it headlessly → "performing update" → "Downloading update".
- ✗ "Download request failed with status: 404 Not Found".
- Root cause: build artifact basename is `Aztec Accelerator.app.tar.gz` (SPACE). latest.json url has the space → updater requests it %20-encoded → feed server's `path.split('/').pop()` didn't URL-decode → 404.
- Fix: `decodeURIComponent` the basename in updater-feed-server.ts.
- Note: the swap/restart/amfid path (the actual 1.0.1 crux) is still unexercised — rc.2 will reach it once the download succeeds.
