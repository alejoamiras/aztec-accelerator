# 1. Cohort boundaries and order

Accept the ratified order:

`B2 → (B3 | B5 | B7) → B6 → B4 last`

Each cohort gets one worktree, branch, plan, and PR stack. B3/B5/B7 may be developed concurrently, but merge one stack at a time: rebase onto fresh `main`, obtain one clean run of every required context, merge, verify `main`, then advance. B3 and B5 both touch `main.rs`, so expect a rebase rather than an atomic merge.

This order is correct:

- B2 closes the remotely reachable consent race first.
- B3, B5, and B7 are functionally independent.
- B6 must establish the publication boundary before B4 wires the final release gate.
- B4 must consume the actual B5 cleanup, B7 tarball, and B6 workflow—not parallel approximations.

Aztec remains frozen at 5.0.1.

# 2. Per-cohort design

## B2 — consent hardening

Changes:

- In `frontend-src/bridge.js`, make `wireButton` guarded by default, with an explicit opt-out only for future non-consent controls; export `rearmInputGuard`.
- In `authorize.js`, track the previous `active` value and rearm exactly once on `false → true`, before enabling controls. Do not rearm on every poll.
- Onboarding, renewal, authorization, and update buttons inherit the default guard. Rebuild `frontend/assets` before every Rust or Playwright run.
- In `update-prompt.js`, send the displayed version for both actions. In `commands.rs`, compare it with `VerifiedUpdate::version()` while holding the pending lock and reject a mismatch without consuming the update.
- In `authorization.rs`, add a 30-second, bounded deny cooldown keyed by `CanonicalOrigin`. Record it in both `resolve` and `resolve_active` for click, close, and timeout denials; prune expired entries during `request`. `server/auth.rs` returns the existing 429 path during cooldown.

Forks:

- **F7: transition latch.** Adopt the exported rearm plus `wasActive`; it binds the guard to actionability without poll-induced starvation.
- **F8: version echo.** Choose version comparison, not a new query/capability: the update is a singleton and the cheap binding closes the actual race.
- **F9: cooldown yes, fairness no.** Cooldown is local and testable; reordering FIFO would invalidate `AUTH_QUEUE_BACKSTOP` and needs a separate policy.

Decisive tests:

- Playwright: `all_consent_buttons_ignore_activation_until_guard_arms`, including a newly added renewal spec.
- Playwright/WebDriver: `promoted_authorization_rearms_click_guard`.
- Rust: `update_prompt_rejects_stale_displayed_version_without_consuming_pending`.
- Rust: `deny_cooldown_blocks_immediate_retry` and `deny_paths_all_record_cooldown`.

Reverting each security fix must fail its named test.

## B3 — native bb containment and diagnostics

Changes:

- In `core/src/bb.rs`, replace `wait_with_output` with a concurrent stderr drain that retains at most 64 KiB but continues draining to EOF. Add an injectable internal timeout seam.
- Read proof output through a 64 MiB bounded reader; reject empty, oversized, and non-32-byte-aligned output before adding the header.
- Add a narrow per-child containment guard: a fresh Unix process group and a Windows Job Object with `KILL_ON_JOB_CLOSE`. Add `Win32_System_JobObjects` and the minimal Unix syscall dependency; no daemon or supervisor.
- The guard registers the active group/job and exposes cancel-and-reap. `main.rs` quit and `updater.rs` install/restart call it explicitly and abort the exit/update if reaping cannot be confirmed. Windows update cancellation occurs before the no-return `install()` call. `renew_cert` retains its existing refuse-while-proving behavior rather than destroying a user proof.
- Add a synchronous panic sink beside the rolling logs: payload, source location, timestamp, flush and `sync_all` before abort. Chain the previous hook.
- Include “Show Logs” in the production menu in `tray.rs`; document Windows/macOS/Linux log paths.

Forks:

- **F4: cap-and-continue.** Stopping the drain recreates pipe deadlock; only retained bytes are capped.
- **F5: validate inside `bb::prove`.** Malformed prover output is `ProveFailed`, preserving the pinned HTTP wire contract.
- **F6: process group plus Job Object.** Direct-child kill alone does not contain descendants or Windows parent death; the small guard is the minimum complete mechanism.

Decisive tests:

- `stderr_spam_is_drained_and_capped`, `prove_timeout_kills_and_reaps_child`, `empty_misaligned_and_oversized_proofs_fail`.
- Per-OS serial test `exit_preparation_kills_bb_and_descendants`; its fake bb spawns a grandchild.
- `updater_cancels_bb_before_no_return_install`.
- Release-profile subprocess test `panic_is_synchronously_persisted`.
- Production tray menu construction test contains `show_logs`.

## B5 — real uninstall

Changes:

- Add `src-tauri/src/uninstall.rs` and early `--prepare-uninstall` handling in `main.rs`. It reports component results and exits nonzero unless cleanup is confirmed.
- Reuse `set_enabled_at`, `disable_crash_recovery`, and `remove_ca_trust`; remove update handoff/token markers only when no live update transaction exists.
- Add ownership-aware autostart removal in `autostart.rs`: remove only `Healthy { points_elsewhere:false }` or a confirmed orphan/absent state. Foreign or unreadable ownership is left untouched and reported.
- Keep `config.json`, approved origins, logs, locks, and `updater-state.json`; the last preserves the anti-rollback floor. Remove cert material and stale update markers only.
- In `nsis/hooks.nsi`, retain POSTUNINSTALL and perform native, idempotent Run/StartupApproved deletion plus verified `schtasks /Delete`; do not invoke an exe that is probably already gone. Preserve `$UpdateMode` and `$EXEDIR != $INSTDIR` guards and the CA-removal breadcrumb.
- Add `prepare-uninstall.sh`/`.ps1` wrappers and documented DMG, AppImage, and deb flows in the accelerator README and `PLATFORM_SUPPORT.md`. They run preparation before deleting the app/package.

Forks:

- **F1: hybrid, native NSIS cleanup.** `--prepare-uninstall` serves manual flows; adding PREUNINSTALL would repeat the ratified one-shot-hook mistake.
- **F2: ownership check.** One copied install must not silently remove another’s persistence.
- **F3: preserve user/security state.** Delete certs and ephemeral markers; retain config/origins and the updater floor.

Decisive tests:

- Real-OS ignored test `prepare_uninstall_removes_owned_artifacts_and_is_idempotent`, run twice on all three OSes.
- `prepare_uninstall_preserves_foreign_autostart_and_user_state`.
- NSIS harness seeds Run, StartupApproved, and a scheduled task, then proves real uninstall removes them while update mode and foreign ownership preserve them.

## B6 — publish, promotion, and recovery

Changes:

- Split `release-accelerator.yml` into immutable GitHub publication, `verify-published-release`, feed promotion, live verification, and GitHub-Latest marking.
- Publication uses `--latest=false`, never deletes/recreates an existing release, and has no AWS credentials. It publishes candidate `latest.json` plus `previous-latest.json`, captured from and cryptographically verified against the current live feed.
- `verify-published-release` downloads every GitHub asset and compares its name, size, and hash with the build artifacts before promotion.
- Add `.github/actions/promote-accelerator-feed/action.yml`: verify signature, expected version, platform set, and asset reachability; upload the exact object; invalidate CloudFront.
- Keep recovery as a mode of the existing `Release Accelerator` workflow. This preserves the IAM role’s exact workflow-name trust. Recovery republishes a selected release’s `previous-latest.json`; normal N+1 release is the fix-forward path.
- Promotion has only `contents:read` and OIDC; GitHub Latest is marked afterward in a separate `contents:write` job.
- Add declared-version-equals-input checks for `tauri.conf.json` and the three Cargo packages. Update `RELEASE_RUNBOOK.md` and `CLAUDE.md`.
- Filter `draft` and `prerelease` releases in `landing/src/main.ts`.

Forks:

- **F12: explicit promotion plus recovery, same top-level workflow.** A second top-level workflow cannot assume the current exact-name OIDC role without an avoidable IAM expansion.
- **F16: filter prereleases.** Public download buttons must not bypass the RC soak.

The recon’s “no previous latest anywhere” is only partly current: stable publication now attaches candidate `latest.json`, but 1.0.7 may not contain the prior feed. Capturing `previous-latest.json` closes the first-transition gap without S3 versioning.

Decisive tests:

- `release_workflow_contract.test.ts`: no AWS in publication, no release deletion, prereleases cannot promote, promotion depends on published verification.
- Fixture test `promotion_rejects_unsigned_wrong_version_or_missing_asset`.
- Landing unit test `latest_download_ignores_rc_and_draft`.
- A recovery drill against fixtures exercises N, freeze-to-N−1, and N+1.

## B4 — shipped-product and migration gate

Changes:

- In `core/src/config.rs`, bump schema to 2 and parse the raw version before deserialization. Migrate v1 `safari_support` to `https_enabled` only when the latter is absent; new key wins when both exist. Preserve origins, speed, update preference, localhost policy, and onboarding state. Unknown future or malformed configs are not overwritten.
- Extend the existing `_e2e-updater*.yml` and updater-smoke scripts rather than creating a second installer framework. Pin 1.0.7 on all OSes and copy Windows’ four-point historical preflight to macOS/Linux.
- Seed a genuine 1.0.7 profile, autostart, CA files/trust where automatable, HTTPS, non-default speed, auto-update, and an approved origin. Upgrade to the candidate, then assert every state item.
- Add `playground/e2e/installed-accelerator.spec.ts` plus a runner script that installs the B7-packed SDK into a fresh playground copy, forces HTTPS-only, drives one real browser proof, and asserts `transmit → proved` with no WASM fallback.
- Finish each OS leg by running the B5 uninstall flow and asserting persistence/trust residues are gone.

Forks:

- **F10: automate macOS login-keychain trust; retain an explicit Windows residual.** CurrentUser Root installation prompts and has frozen CI repeatedly. Windows still tests read/remove and preservation; the production add-dialog remains documented, not falsely “green.”
- **F11: extend updater smokes.** They already perform the real installs and updates; a parallel harness would duplicate the most fragile logic.

Decisive tests:

- `v1_config_migrates_safari_support_and_preserves_all_state`.
- One combined installed-product gate per OS: `1.0.7 → candidate → HTTPS native browser proof → uninstall`.
- Linux real trust test, macOS fresh-login-keychain test, Windows headless-safe trust suite. Any remaining Windows trust-install claim is explicitly excluded from automated coverage.

## B7 — SDK release contract

Changes:

- In `package.json`, make directly imported `@aztec/bb-prover`, `@aztec/stdlib`, and `@aztec/simulator` exact 5.0.1 peers, mirrored in devDependencies. Remove redundant top-level Aztec declarations not imported by the SDK.
- Add exported `AcceleratorHttpError` with status, server code, message, and cause. Fallback on recognized denial/timeout/cancel, 408/413/429/503, known proving/download failures, network failure, and malformed success. Surface typed errors for invalid request/version, `version_not_allowed`, and unknown HTTP contracts. Never expose raw ky errors; only actual denial emits `denied`.
- Centralize Rust `ACCELERATOR_API_VERSION` and TS `SUPPORTED_ACCELERATOR_API_VERSION`; expose `appVersion` and `apiVersion` in reachable `AcceleratorStatus` variants.
- Replace the inline publish rewrite with `scripts/prepare-sdk-package.ts`. CI builds, prepares, `npm pack`s, inspects the file list, and installs the exact tarball in fresh Node/typecheck and Vite/build consumers.
- Include `MIGRATION.md` in `files`; use it as named SDK release notes and attach it as an asset. Document revision and dist-tag policy.

Forks:

- **F13: exact peer dependencies.** This preserves the frozen Aztec axis while preventing duplicate-class/wire-format graphs.
- **F14: typed wrapper and explicit table.** Raw undocumented ky exceptions are not a release contract.
- **F15: ship and attach MIGRATION.md.** Repository-only documentation does not help npm consumers.
- **F17: use `publish-testnet.yml`, then promote.** Its native E2E and main-ref gates are mandatory; direct `_publish-sdk` dispatch is weaker. Publish after desktop GA, smoke the exact npm version, then move `latest`.

Decisive tests:

- Tarball Node and Vite consumers, including exact-peer resolution.
- Error-table tests for every server status/code and `no_raw_http_error_escapes`.
- `public-contract.test.ts` pins exports, error behavior, status fields, migration inclusion, and corrected README/SKILL claims.

# 3. Release execution

1. Merge a version-only PR setting app/core/server declarations and lockfiles to `2.0.0-rc.1`; SDK stays the publish-time `0.0.0` placeholder.
2. Dispatch RC. It must build, pass all three installed-product gates, publish a GitHub prerelease, and verify published assets. Feed signing/promotion and GitHub Latest remain skipped.
3. Fix forward through `2.0.0-rc.N`; every RC gets the full gate. On the chosen RC, run the installed E2E loop continuously for at least two hours, then obtain a clean release review.
4. Merge a version-only `2.0.0` PR and rerun main CI. Dispatch stable: rebuild and regate, sign the feed, publish immutable assets plus current/previous manifests, verify publication, promote, verify the public CDN cryptographically, then mark GitHub Latest.
5. Run `publish-testnet.yml` for `5.0.1-revision.N`. Require the tarball consumers and native E2E first. Install that exact registry version in clean Node/Vite consumers, run a live proof, then dispatch `promote-latest.yml`.
6. Run fresh-install and live-feed 1.0.7→2.0.0 smokes on macOS, Linux, and Windows. Verify release notes, MIGRATION, npm metadata, download links, and nomenclature. Notify at RC publication, GA promotion, SDK publication, or blocker.

Before promotion, failure is harmless to the fleet. After promotion, recovery republishes the archived previous manifest to stop further uptake; already-updated clients do not downgrade. Ship 2.0.1 through the full path. A crash-on-start 2.0.0 requires communicated manual reinstall. Never delete tags/releases. SDK recovery moves `latest` back for new installs and publishes a new revision; existing lockfiles require consumer action.

# 4. Top risks

1. **Descendant containment differs by OS.** Use real per-OS child/grandchild tests, not compile-only coverage.
2. **Packaged proof gates become flaky or too slow.** One proof per OS, pinned 1.0.7 fixtures, cached browser/bb, bounded retries, and failure artifacts.
3. **NSIS cleanup deletes another copy’s state.** Ownership checks plus foreign-entry mutation tests.
4. **Promotion succeeds partially or CDN serves stale bytes.** Immutable previous manifest, fail-closed verification, invalidation, public cryptographic canary.
5. **SDK tarball differs from the tested workspace.** One preparation script feeds PR CI, publication, and consumer tests; publish that exact output.

# 5. Explicit scope cuts

Refuse B1/Authenticode; protocol/Aztec bumps; TUF/replay freshness; telemetry or diagnostic bundles; bb revocation/sandboxing; queue fairness, aggregate budgeting, or download singleflight; independent SDK SemVer; npm Trusted Publisher; TS↔Rust code generation; supervisor daemons; automated root/user enumeration for DMG/AppImage/deb uninstall; deleting configs/origins/updater floors; automatic downgrade; new release channels; and unrelated landing/playground security work.