# plan-main.md — main-agent planning leg, v2-release-train

One of three independent legs (main / codex / fable). Positions on every fork F1–F17 from
`recon.md`. Consolidation produces the final `plan.md`.

## Cohort boundaries + order — ACCEPT default, with one refinement

`B2 → B3 → B5 → B7 → B6 → B4 → release`. Solo execution means cohorts are sequential anyway;
the "parallelizable" middle three are ordered cheapest-risk-first (B3 Rust-only, B5 Rust+NSIS,
B7 TS+CI). B6 before B4 because B4's installed-E2E consumes the artifacts B6's publish job
uploads. Each cohort: own worktree `.claude/worktrees/<cohort>`, branch `worktree-<cohort>`,
one gh-stack (1–3 PRs), merged one-PR-at-a-time on fresh main with single green runs.

F16 (landing prerelease filter) rides in the B6 cohort (release mechanics). The versions bump is
its own tiny first PR of the release phase, not a cohort.

## B2 — consent hardening

Changes:
- `bridge.js`: export `rearmClickGuard()` (rename of private `rearmInputGuard`); keep native
  focus/pageshow rearm. Make `wireButton` guard **default-on** (`opts.guard !== false`) —
  flipping the default is what makes the property hold product-wide; settings.js stays as-is
  (out of scope, backlog note).
- `authorize.js`: `wasActive` latch in `refreshPending()`; on false→true transition call
  `rearmClickGuard()`. **F7: adopt.**
- `onboarding.js/renewal.js/update-prompt.js`: nothing to do beyond the default flip (they
  become guarded automatically). Verify via Playwright.
- **F8: (a) version-echo.** `respond_update_prompt(window, accepted, displayed_version: String)`;
  Rust compares to `pending.version()` before `take()`; mismatch → drop pending, close popup,
  log `SECURITY:` line, do not install. update-prompt.js passes its `#version` text.
- **F9: deny-cooldown IN, fairness DEFER.** `DENY_COOLDOWN: Duration = 30s` const beside
  `MAX_PENDING_ORIGINS`; recorded in `resolve`/`resolve_active` on `Deny` outcome (covers click,
  60s timeout, window-close uniformly); checked in `server/auth.rs` right after the
  `is_approved` early-return → immediate `OriginDenied` without popup while cooling down.
  Fairness deferred: no policy exists and AUTH_QUEUE_BACKSTOP's bound assumes strict FIFO.

Tests (smallest decisive set):
1. Playwright `guard.spec.ts`: `__CLICK_GUARD_MS__=200` override; for each of the 4 popups:
   immediate click → IPC call absent; wait past → present. (Mutation proof for the default flip:
   revert bridge.js default → test fails.)
2. Playwright authorize rearm: mock flips `active` after load; click within 200ms of flip →
   absent; after → present. (Mutation: remove transition-rearm → fails.)
3. Playwright update version-echo: mock pending v=Y while DOM shows X → respond carries X →
   assert no install IPC. (Mutation: revert Rust compare → fails at the wdio tier; at Playwright
   tier assert the payload carries the displayed version.)
4. Rust `deny_cooldown_blocks_immediate_retry` + `cooldown_recorded_on_timeout_and_close`
   (authorization.rs test mod, co() helper). (Mutation: revert check → fails.)
5. wdio auth-flow addition: real-webview click-during-guard ignored (the one real-timing proof).
Frontend rebuild (`frontend:build`) in every loop iteration.

## B3 — bb containment

Changes (bb.rs + 3 exit sites):
- **F4: cap-and-continue.** Replace `wait_with_output()` with manual drain: spawn task reading
  stderr via `AsyncReadExt` loop (downloader.rs:118-152 idiom), accumulate ≤64KiB, keep draining
  to EOF counting total; log `stderr_total_bytes` + truncated flag. stdout stays inherited.
- **F5: validate inside `bb::prove`**: after read, `raw_proof.is_empty() || len % 32 != 0` →
  `Err("bb produced invalid proof output (N bytes))` → existing `ProveFailed` path. No new
  ProveError variant, no wire change.
- **F6: explicit-kill registry + Linux PDEATHSIG; Job Object OUT.** `bb::ActiveProveChild`
  (static Mutex<Option<pid>>) set/cleared by RAII guard inside `prove()` (cleared before guard
  drop on completion). `bb::kill_active_prove_child()` called at: tray quit (before
  `app.exit(0)`), `RunEvent::ExitRequested` with `Some(code)`, `perform_update` before
  `update.install()` (explicit-before-no-return, per updater.rs convention). Unix
  `kill(pid, SIGKILL)`; Windows `TerminateProcess` (OpenProcess via existing windows-sys).
  Linux extra: `pre_exec` PR_SET_PDEATHSIG(SIGKILL) — one unsafe line, crash-orphan closed on
  the OS where CI can prove it. macOS/Windows crash-orphan = accepted residual: bb self-
  terminates at completion, workspace reaped by F-08a, PROVE_TIMEOUT bounds the app-alive case.
  `renew_cert`'s refuse-pattern KEPT (restart-while-proving stays refused; quit/update kill).
- Panic hook (fold-in): `panic::set_hook` after tracing init writing payload+location via
  `tracing::error!` THEN a **synchronous** fallback `writeln!` to a `panic.log` in log_dir()
  (non_blocking WorkerGuard flush race — sync file is the guarantee, tracing line best-effort).
- Show Logs (fold-in): move `show_logs` item construction out of the `dev_mode` branch
  (handler already ungated). README Windows log path documented.

Tests:
1. `stderr_spam_is_bounded`: fake bb emits 8MiB to stderr then succeeds → prove OK, captured
   ≤64KiB+ε, logged total = 8MiB. (Mutation: revert drain cap → test fails on captured size.)
2. `empty_proof_rejected` + `unaligned_proof_rejected`: fake bb writes 0/33 bytes → 500
   prove_failed, wire shape test still green. (Mutation: revert validation → 200 with garbage.)
3. `kill_active_prove_child_terminates_bb` (unix, #[serial]): fake bb = `sleep 300`; start
   prove, call kill fn, assert child gone + prove errors promptly. (Mutation: no-op the kill →
   child survives → fails.)
4. `pdeathsig_reaps_on_parent_death` (linux, L3-ish, spawn helper proc) — if flaky in 1 day,
   drop to innocuous-untested (one line, OS-guaranteed semantics) with ledger note.
5. Panic hook: unit test installs hook, panics in a thread, asserts panic.log contains location
   (sync path is testable; abort path is innocuous-by-construction).
PROVE_TIMEOUT expiry test: make the timeout injectable (bind_with_retry_inner pattern) and add
one test — closes the "nothing tests timeout" gap cheaply while we're in the file.

## B5 — real uninstall

- **F1: hybrid, no new NSIS hook.** (1) `--prepare-uninstall` early-argv flag (mirrors
  `--remove-ca-trust` shape incl. exit-code contract): autostart `set_enabled_at(None,false)`
  → `disable_crash_recovery()` → `remove_ca_trust()` → clear update markers +
  CA-TRUST-NOT-REMOVED.txt; prints per-step status; exit 1 if any step unconfirmed. Checks
  `updater.lock` first → refuses during live update (mirrors heal's Skipped). (2) Windows
  POSTUNINSTALL additions, NSIS-native inline (proven pattern, exe already deleted):
  `schtasks /Delete /F /TN "Aztec Accelerator Crash Recovery"` + `reg delete ...\Run /v` +
  StartupApproved value delete + marker-file Deletes — each idempotent, each followed by
  re-query where cheap. Ratified F-05 precedent respected; the PREUNINSTALL one-shot bet is
  refused. Known residual: task may relaunch app between NSIS app-kill and POSTUNINSTALL task
  delete (pre-existing race, now bounded — task deleted at the end; document).
- **F2: accept + document.** Unconditional remove; a surviving second install self-heals via
  startup_reconcile on next launch (#429 machinery) — note in code comment.
- **F3: precise scope.** Removes: certs/, markers, warning txt, locks, OS entries (above).
  Keeps: config.json (user data), updater-state.json (anti-rollback floor survives reinstall —
  deliberate, documented). Full-purge = documented `rm -rf ~/.aztec-accelerator` in README
  uninstall section (new); DMG/AppImage/deb cleanup flows documented there + PLATFORM_SUPPORT.

Tests:
1. Extend `autostart_heal.rs` shape → new `tests/prepare_uninstall.rs` (per-OS, #[ignore],
   throwaway HOME, --test-threads=1): enable autostart+recovery+trust → run **the compiled
   binary** with `--prepare-uninstall` (establish CARGO_BIN_EXE pattern — closes the untested-
   argv-branch gap for BOTH flags) → assert all gone → run again → still-gone (idempotent).
   Wire into cert-trust legs (3 OSes). (Mutation: skip crash-recovery step → Windows leg fails.)
2. NSIS harness: extend harness.test.nsi assertions — after POSTUNINSTALL, schtasks /Query
   exit≠0, Run value absent. (Mutation: drop the inline delete → harness fails.)

## B7 — SDK contract

- **F13: KEEP exact pins for v2.0.** The 7-day-gate + vetted-once-frozen-forever story is
  load-bearing owner policy; a peer-migration mid-release-train is consumer-breaking churn
  without a driving incident. Fix the docs lie instead (README/SKILL: "bundled exact-pinned
  @aztec/*; match your host to X.Y.Z") and PROVE the nested-install works via the tarball job
  in a foreign-ish host. Peer-migration → post-v2 backlog with its own design round.
- **F14: one exported error class + 403 reclassification.** `AcceleratorRemoteError extends
  Error {status, code?, detail?}` wrapping the escape path (`throw err` :535 →
  `throw AcceleratorRemoteError.from(err)`). Reclassify `403 + code=version_not_allowed` →
  WASM fallback with reason "version-mismatch" (server refusing a version ≠ user denial);
  auth-family 403 keeps "denied". SKILL.md:145-153 corrected; public-contract.test.ts pins the
  corrected claims + barrel export.
- api_version/appVersion: additive optional fields on the available branch of
  AcceleratorStatus; #classifyHealth stops discarding them. `WIRE_API_VERSION = 1` const in
  transport (single TS source; Rust keeps its own — cross-language dedup is over-engineering).
- **F15**: add `MIGRATION.md` to `files`; tarball job asserts its presence; `_publish-sdk.yml`
  release step gains `--notes-file` (generated notes + MIGRATION link) or asset upload.
- Exports rewrite (`node -e`) → committed `scripts/prepare-sdk-publish.ts` (diff-reviewable),
  called from the workflow; unit-tested beside get-sdk-publish-version.test.ts.
- Tarball consumer CI job in sdk.yml: build → `npm pack` → two fixtures under
  `packages/sdk/consumer-fixtures/` (node-tsc: installs tgz, tsc a file importing
  AcceleratorProver + narrowing AcceleratorStatus; vite: `vite build` same import) on Node 24;
  joins sdk-status needs+results. **F17**: publish via existing publish-testnet chain (e2e-
  gated) AFTER desktop 2.0.0 promote; then promote-latest.yml dispatch moves `latest`.

Tests: the tarball job IS the test (mutation: remove `files: dist` → job fails);
`error_escape_path_is_typed` bun test (mock 500 → instanceof AcceleratorRemoteError, code
prove_failed) (mutation: revert wrap → fails); `version_not_allowed_falls_back` (mutation:
revert reclassify → fails); public-contract additions.

## B6 — publish/promote split + rollback

- Split `release` at :979/:981. `publish` keeps gh-release create (+ RC path unchanged).
  New `promote` job (stable-only): needs publish + verify-release-assets; BEFORE overwrite,
  `aws s3 cp` current live latest.json → `landing/releases/history/latest-<prevver>-<ts>.json`
  (fix-forward source, zero infra change); then upload + invalidate. `verify-live-feed`
  unchanged, post-promote.
- **F12**: plus dispatchable `promote-accelerator.yml` (distinct name; SDK's promote-latest.yml
  untouched): input version, asserts the gh release + signed feed asset exist, re-runs the
  same promote steps — THE freeze/fix-forward lever. Freeze = it and the in-train promote job
  both refuse when repo variable `RELEASE_FREEZE=1` (documented).
- Shared steps live in `.github/actions/promote-feed/action.yml` composite (used by both).
- **F16**: landing `fetchLatestAcceleratorTag` filters `!r.prerelease`.
- N+1 recovery rehearsal: once, on the 2.0.0-rc chain — promote rc to a STAGING key
  (`history/rehearsal-latest.json`), verify, restore — scripted into the runbook as the drill.
- Runbook rewrite: real job graph, publish-vs-promote checklists, fix-forward procedure
  (dispatch promote-accelerator with prior version), delete-and-restore advice REMOVED,
  trust-verification manual section ADDED (closing the dangling trust_macos/windows.rs
  reference); CLAUDE.md summary refreshed same PR.

Tests: actionlint; a workflow-level dry-run is impossible cheaply — instead the rc chain (B4
gates) exercises publish, and the rehearsal exercises promote against staging key. Innocuous-
untested surface: YAML wiring (accepted, exercised at RC time).

## B4 — prove the shipped product (LAST)

- Config migration first (core/config.rs): read `config_version`; add
  `#[serde(alias = "safari_support")]` to `https_enabled` (v1 field revived — flip the two
  pinning tests to assert the ALIAS now works); unknown future version → preserve-unknown-keys?
  NO — keep serde default fail-open but log; version bump of CONFIG_VERSION stays 1 (no new
  schema). Smoke-seed drift fixed (mac/linux scripts write https_enabled).
  Tests: `legacy_safari_support_maps_to_https_enabled` (mutation: drop alias → fails);
  round-trip preserves origins/speed.
- **F11**: new `_e2e-installed.yml` (3-OS matrix) consuming the RC's published artifacts:
  install via the updater-smoke install idioms; seed config (origins, https_enabled, speed);
  launch installed app; run packed-SDK proof **in Node over HTTPS** with NODE_EXTRA_CA_CERTS
  pointing at the app's CA (real TLS, all 3 OSes, no OS-store dialog) + **real-browser leg on
  Linux only** (NSS certutil trust, playwright chromium). **F10**: macOS login-keychain spike
  timeboxed 1 day → else residual; Windows CurrentUser\Root = residual (empirical freeze);
  both residuals recorded in audit report with codex concurrence.
- Upgrade test: extend all three `_e2e-updater-*` with stateful assertions (seed BEFORE
  installing N-1: origins+https+speed+auto_update; after update to RC assert all survived +
  https listener up + CA still trusted at client level). mac/linux keep dynamic N-1 (== 1.0.7
  now); Windows keeps pinned 1.0.7 default. Uninstall leg: after upgrade assertions, Windows
  runs the real uninstaller and asserts B5's cleanup; mac/linux run `--prepare-uninstall` and
  assert.
- These jobs join release-accelerator.yml between publish and promote (gates), and run on RC
  dispatches too (is_prerelease-agnostic — gates must gate the RC).

## Release execution

1. Versions PR: tauri.conf 2.0.0-rc.1? NO — dispatch takes version input; source stays on next-
   rc convention. Bump source to `2.0.0-rc.1` via the normal bump PR path (one PR: tauri.conf,
   both Cargo.toml+locks, core/Cargo.toml cosmetic 2.0.0, README examples touch-up).
2. Dispatch release-accelerator `2.0.0-rc.1` → full pipeline incl. new gates → RC published
   (GH prerelease; feed untouched; landing now filters prereleases).
3. Soak ≥2h: loop the installed-E2E suite locally/CI against the RC while writing MIGRATION
   notes + release notes; watch update-feed-health.
4. Codex release-readiness review (fresh session): diffstat, gate results, runbook — must
   return clean.
5. Dispatch `2.0.0` (same commit): pipeline reruns all gates on the stable build → publish →
   **promote** (auto, D-R1) → verify-live-feed → PushNotification.
6. Post-release: fresh-install smoke (3 OS, from public links), live-feed 1.0.7→2.0.0 update
   smoke (the updater workflows against the LIVE feed), README/landing checks.
7. SDK: dispatch publish-testnet (e2e-gated publish of -revision.N) → tarball job green →
   promote-latest.yml → verify `npm i @alejoamiras/aztec-accelerator` clean in a scratch dir →
   PushNotification.
8. Close-out per brief §8 (+ post-v2 backlog: n1-version bump, peer-migration design, settings
   guard, Job Object, aztec-stable cron dormancy, npm Trusted Publisher).

## Risk register (top 5)

1. **3-OS CI flake on new installed-E2E** (notarization, runner images) → build on the proven
   updater-smoke bones, retry-with-backoff at install steps, and keep legs independent so one
   OS's flake doesn't hide another's signal.
2. **NSIS ordering assumption wrong** (POSTUNINSTALL timing) → harness.test.nsi extension
   proves the inline deletes actually execute; if PREUNINSTALL turns out to fire usefully we
   still don't need it (inline is self-sufficient).
3. **Guard default-flip breaks an existing spec** (settings tests use wireButton? recon says
   settings doesn't import it — verify) → Playwright full suite in cohort CI before merge.
4. **Stable rebuild ≠ soaked RC artifacts** (existing model) → same commit + full gate rerun on
   the stable build; provenance work stays out of scope (SF, backlog).
5. **bb kill races proof completion** (pid cleared vs kill) → registry mutex + clear-before-
   guard-drop ordering; test 3 covers the live path; kill of already-dead pid is ignored-errno.

## Scope cuts (refuse even if asked)

Job Objects/full supervisor; queue fairness redesign; TS↔Rust api_version codegen; peer-deps
migration; TUF/replay freshness; telemetry; deleting user config on uninstall; S3 bucket
versioning/infra changes; new PREUNINSTALL hook; multi-Node test matrix (Node 24 only);
rewriting updater-smoke scripts into a framework (extract only what B4 needs).
