# recon.md — consolidated Phase 0.4 recon, v2-release-train

Six read-only explorers over worktree @ `0c351bc` (fresh origin/main), 2026-08-16. Per-surface
detail lives in `recon/*.md` (b2-consent, b3-bb-containment, b5-uninstall, b6b4-release, b7-sdk,
nomenclature). This file is the synthesis planners and auditors argue against.

## Global verdicts

- **The 1→2 major bump itself is mechanically safe.** Real SemVer everywhere (updater Layers A+B,
  feed, tags, RC branching); zero major-digit special-casing in the tree; identity/task/marker/
  cache keys are product-name-keyed, never version-keyed. `2.0.0-rc.N` flows exactly like
  `1.0.8-rc.1`. No code change needed for the bump mechanics. (recon/nomenclature.md)
- **RCs never reach the fleet**: everything feed-touching is `is_prerelease == false`-gated; the
  B6 "publish→verify→promote" split formalizes a boundary that already half-exists — split point
  is release job :792-979 (publish) vs :981-998 (promote), and the promote job needs FEWER
  privileges than today's combined job. (recon/b6b4-release.md)
- **The brief's anchors are current** except two SDK line-drifts (types.ts union now :76-112;
  health handler now server.rs:405-453).
- Input 04's P2 (crash-orphan witness reaper) is CONFIRMED CLOSED by F-08a.

## Reuse map (strongest levers)

- B2: bridge.js guard engine reusable as-is; only arming + call sites change. Playwright can test
  guard timing via `__CLICK_GUARD_MS__` override; WebDriver auth-flow spec has the real-timing
  plumbing. Rust arbiter tests (authorization.rs:431-919) are the cooldown-test template.
- B3: `renew_cert`'s `prove_semaphore.try_acquire()` refuse-pattern (commands.rs:624-667) is a
  pre-written design note; RAII-guard idiom established (StatusGuard/BindOwnedGuard/
  CrashRecoveryGuard with defuse()/rearm_now() for no-return-call sites); fake-bb-via-
  BB_BINARY_PATH + #[serial] harness is the test seam; downloader.rs has both bounded-read shapes
  (sync CappedReader + async chunk loop).
- B5: all needed cleanup fns already exist AppHandle-free (set_enabled_at(None,false),
  disable_crash_recovery, remove_ca_trust); `--remove-ca-trust` is the early-argv flag precedent;
  autostart_heal.rs is the L3 idempotency-test shape; NSIS harness executes hooks for real in CI.
- B4: updater-smoke scripts already do REAL installs on all 3 OSes (DMG ditto / AppImage /
  silent NSIS) + config-seeding seam; the Windows pinned-N1 + 4-point preflight
  (_e2e-updater-windows.yml:21-158, default n1=1.0.7) is THE pattern for a stateful 1.0.7→2.0.0
  upgrade test; GitHub Releases is the fixture store.
- B6: release-auth-preflight (:73-89) is the minimal-job shape for promote; verify-live-feed
  already exists as post-promote canary; promote-latest.yml (SDK) is the dispatch-rollback-lever
  shape (name is TAKEN — accelerator promote must be distinctly named).
- B7: sdk-status aggregate job is the gate to extend; get-sdk-publish-version.ts already tested;
  public-contract.test.ts is the docs-drift guard to extend for error-contract claims.

## Design-fork register (each cohort plan must take a position; auditors attack these)

- **F1 (B5, structural)**: NSIS-native inline cleanup in POSTUNINSTALL (schtasks /Delete + reg
  delete, mirroring the proven certutil pattern; exe already gone) VS a new PREUNINSTALL hook
  invoking `AztecAccelerator.exe --prepare-uninstall` (exe alive, but contradicts the ratified
  F-05 "no new one-shot-bet uninstall hook" precedent). Hybrid possible: --prepare-uninstall
  exists for scripted/manual use on all OSes; NSIS does inline native deletes.
- **F2 (B5)**: unconditional backend::remove() can strip a SURVIVING other install's autostart
  (one artifact slot per OS) — ownership-check before delete vs accept+document.
- **F3 (B5)**: ~/.aztec-accelerator scope enumeration — certs/ + markers removed; config.json +
  approved origins are USER data (keep); updater-state.json floor: decide + document.
- **F4 (B3)**: stderr drain semantics — cap-and-continue (keep draining so bb never blocks on full
  pipe; stop accumulating) vs CappedReader-style fail-closed. Recon leans cap-and-continue.
- **F5 (B3)**: proof-output validation placement — inside bb::prove (Box<dyn Error> → ProveFailed)
  vs new ProveError variant (wire contract is test-pinned text/plain; new variant must join it).
- **F6 (B3)**: child-death mechanism — Unix process_group (tokio 1.52.3 has it) + Windows Job
  Object (needs Win32_System_JobObjects feature in core's windows-sys) vs simpler explicit
  kill-via-RAII-guard on the 3 exit sites (must subsume/coexist with renew_cert's refuse pattern;
  updater.rs site must be explicit-before-no-return-call, not Drop).
- **F7 (B2)**: authorize.js arming — export rearm from bridge.js, call on queued→active TRANSITION
  with a was-active latch (else every 1s tick re-pushes arm time).
- **F8 (B2)**: update consent binding — (a) version-echo: JS sends displayed version,
  respond_update_prompt compares to VerifiedUpdate::version() before take() (cheap, recommended by
  recon) vs (b) get_pending_update query command + capability change (singleton/no-id — auth's
  SEC-06 pattern does NOT port verbatim).
- **F9 (B2)**: deny-cooldown — check near server/auth.rs:53-55, RECORD in resolve/resolve_active
  (covers click/timeout/window-close uniformly). Queue FAIRNESS: recommend DEFER — no policy
  exists, and AUTH_QUEUE_BACKSTOP's bound assumes strict FIFO (not "genuinely small").
- **F10 (B4)**: trust-install on CI — macOS: plausible via fresh login keychain + unlock +
  set-key-partition-list (no repo precedent); Windows CurrentUser\Root: empirically FREEZES
  runners (documented in-script) → honest residual with codex concurrence. Linux already covered.
- **F11 (B4)**: packaged E2E home — extend updater-smoke scripts/workflows vs new
  _e2e-installed.yml reusable. Note only Windows updater leg drags the heavy setup-accelerator
  composite (reconcile, don't propagate).
- **F12 (B6)**: promote shape — job inside release-accelerator.yml (inherits concurrency; D-R1
  autonomous gates) PLUS a separate dispatchable freeze/fix-forward workflow reusing the same
  step (rollback lever ≙ promote-latest.yml precedent). Store previous latest.json (S3 versioning
  or artifact) so fix-forward has a source.
- **F13 (B7)**: peerDependencies vs exact-pin — recon surfaces BOTH sides: exact-pin is
  load-bearing for the vetted-once-frozen-forever supply-chain story; peer-move fixes nested-dup
  instanceof risk for foreign hosts. Only the new tarball job can even exercise the foreign-host
  case. Codex arbitrates.
- **F14 (B7)**: error contract — SDK error class wrapping the escape path vs document-the-raw
  behavior; 403 conflation (4 server variants → one "denied") needs at least doc truth;
  SKILL.md:145-153 is FALSE today and public-contract.test.ts should pin the corrected claim.
- **F15 (B7)**: MIGRATION.md is likely NOT in the tarball (files array) — verify npm pack
  --dry-run; attach to GH release via --notes-file or asset.
- **F16 (landing)**: fetchLatestAcceleratorTag has no prerelease filter → RC exposed to real
  visitors during the ≥2h soak. Recommend: filter prereleases (small, in-scope as release
  mechanics).
- **F17 (B7/release ordering)**: SDK -revision.N publish trigger — existing publish-testnet chain
  (e2e-gated) vs direct _publish-sdk dispatch; and WHEN relative to desktop promote.

## Cross-cutting constraints (from recon, binding on all plans)

- Frontend edits require `bun run --cwd packages/accelerator frontend:build` before Rust/Playwright
  see them (build.rs guards Rust; Playwright does NOT guard staleness).
- New Rust tests touching BB_BINARY_PATH join the DEFAULT serial bucket.
- Windows-gated unit tests execute ONLY in the windows-build job; macOS unit tests ONLY in the
  cert-trust macOS leg's bare cargo test. cfg-gated code compiles ≠ runs (standing repo lesson).
- actionlint.yaml must learn any new runner label; per-job explicit permissions; secrets explicit
  per-call, never inherit; user-facing workflows plain-named, workflow_call `_`-prefixed.
- ProveError wire contract: text/plain JSON-string, pinned by test.
- panic=abort only in src-tauri release profile; tracing file layer is non_blocking → panic hook
  needs sync write/flush path (WorkerGuard race).
- Runbook :28-36 and CLAUDE.md summaries are stale vs the real job graph — fix together in B6 or
  they re-diverge.
- Smoke config seeds: mac/linux scripts still write legacy `safari_support`, Windows writes
  `https_enabled` — reconcile when touched (B4).
