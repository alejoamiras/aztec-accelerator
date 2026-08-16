# plan.md — v2-release-train (consolidated from 3 independent legs)

Status: consolidated draft (rev 1) — awaiting contradiction-check + double audit.
Legs: `plan-main.md` (main), fable leg (agent report, positions folded here), codex leg
(`~/.cache/tmp/codex-3BWeA8Xi/response.md`, session `01a00cc4-0918-7603-94ed-12851a926021`).
Binding constraints: `brief.md` (D-R1..D-R5), `recon.md` (F1–F17), `decision-ledger.md`.

## Phase 0 answers (owner-ratified via brief)

Success = DONE block of the goal. Quality bar: production. Validation layers: cargo/bun unit +
integration, Playwright (mocked UI), WebDriverIO (real webview), NSIS harness, L3 `#[ignore]`
per-OS integration, 3-OS installed-product gates, actionlint, full release pipeline. Untestable ⇒
only if innocuous. Security-relevant fixes mutation-proved.

## Architecture & Implementation (per cohort)

Cohort order (all legs agree): **B2 → B3 → B5 → B7 → B6 → B4 → release**. Solo dev = sequential;
merge one PR at a time on fresh main. B3+B5 both touch `main.rs` → expect rebase. F16 rides in B6.
Each cohort: worktree `.claude/worktrees/<slug>`, branch `worktree-<slug>`, own mini-plan header in
the PR body, gh-stack of 1–3 PRs.

### B2 — consent hardening (`b2-consent-guard`)

Files: `frontend-src/{bridge.js,authorize.js,update-prompt.js}`, `src-tauri/src/{commands.rs,
main.rs,updater.rs}`, `core/src/{authorization.rs,server/auth.rs}`, `e2e/*`, `e2e-webdriver/*`.

1. **Guard default-on** (all legs): `wireButton` guards unless `opts.guard === false`; export
   `rearmClickGuard()`; native focus/pageshow rearm kept. onboarding/renewal/update inherit.
   settings.js: DEFERRED (tray-initiated window; remote page cannot pop it under the cursor) —
   backlog.
2. **F7**: `wasActive` latch in `refreshPending()`; rearm exactly on false→true, before
   `setControlsEnabled`.
3. **F8 version-echo, type-enforced** (fable's shape): update-prompt.js sends displayed version
   with both actions; `PendingUpdate` gains `take_matching(&displayed) -> Option<VerifiedUpdate>`
   and the unconditional `take()` path is DELETED (compile-time binding). Mismatch: pending
   retained, `SECURITY:` log, close popup, immediately re-show with the current pending version
   (user not stranded until next 12h poll).
4. **F9 cooldown IN / fairness DEFER** (all legs): `DENY_COOLDOWN = 30s` const beside
   `MAX_PENDING_ORIGINS`, `Duration` injectable for tests; per-CanonicalOrigin timestamps in
   `PendingState`; recorded in `resolve`/`resolve_active` on Deny (covers click/timeout/close);
   pruned on `request`; capped map. Check in `server/auth.rs` after the `is_approved`
   early-return → **immediate `OriginDenied`** (main+fable; codex preferred 429 — rejected:
   denial semantics must keep dApps on the WASM-fallback path, and 429 would surface a typed
   error instead; ledger D-C4).

Tests (decisive set): (1) Playwright guard spec parameterized over onboarding/renewal/update:
`__CLICK_GUARD_MS__=200`, immediate click → no IPC recorded, post-wait click → fires [mutation:
revert default-on]. (2) Playwright authorize rearm-on-promotion [mutation: remove latch/rearm].
(3) Rust `take_matching` match/mismatch (+pending retained) unit [mutation: restore blind take —
won't compile, which is the proof]. (4) Playwright update spec asserts respond payload carries
the DOM version. (5) Rust `deny_cooldown_blocks_immediate_retry` +
`cooldown_recorded_on_timeout_and_close` [mutation: revert auth.rs check]. (6) wdio auth-flow
addition: real-webview click-within-700ms ignored. Frontend rebuild before every test loop.

### B3 — bb containment (`b3-bb-containment`)

Files: `core/src/bb.rs`, `core/Cargo.toml` (+`Win32_System_JobObjects`), `src-tauri/src/{main.rs,
updater.rs,tray.rs}`, `core/src/server/tests.rs`.

1. **F4 cap-and-continue** (all legs): manual drain replacing `wait_with_output` — async chunk
   loop (downloader.rs:118-152 idiom) retaining ≤64KiB, draining to EOF, total counted; runs
   concurrent with `child.wait()` under an **injectable timeout** (`prove_inner(timeout)`,
   bind_with_retry_inner pattern).
2. **F5 validate inside `bb::prove`** (all legs): reject empty, non-32-aligned, and **>64MiB**
   (codex's bounded read) proof files → `Err` → existing `ProveFailed`. Wire contract untouched.
3. **F6 — process group + Job Object + explicit terminate** (fable+codex majority over main's
   kill-registry-only): Unix `cmd.process_group(0)`; registry (`Mutex<Option<pgid>>`) via RAII
   guard (StatusGuard idiom), `bb::terminate_inflight()` SIGKILLs the group — also called in the
   timeout branch (kills grandchildren `kill_on_drop` misses). Windows: one static Job Object
   with `KILL_ON_JOB_CLOSE`; every bb child assigned → OS kills bb on ANY app exit incl. the
   NSIS-handoff internal `exit(0)` and crashes; zero per-exit-site code. Explicit call sites:
   tray quit (before `app.exit(0)`) and `perform_update` **before** `install()`/`restart()`
   (explicit-before-no-return; on unconfirmed reap: ABORT the update — it can retry; quit never
   blocks: kill, log `SECURITY:`, proceed). Linux extra: `pre_exec` PDEATHSIG(SIGKILL) one-liner
   (crash-orphan closed where CI can prove it). macOS crash-orphan = residual (bb self-
   terminates; F-08a reaps workspace). `renew_cert` refuse-pattern KEPT; coexistence documented
   on `terminate_inflight`.
4. **Panic hook**: installed after tracing init; synchronous append (open/write/`sync_all`) of
   payload+location+timestamp to `log_dir()/panic.log`, best-effort `tracing::error!`, chains
   previous hook. **Show Logs** in production menu (handler already ungated).

Tests: (1) `stderr_spam_is_drained_and_capped` — 5MiB spam, prove OK, retained ≤64KiB [mutation:
revert cap]. (2) proof validation triple (empty/33B/>cap → Err; 64B → Ok) [mutation: revert
validation]. (3) unix `terminate_inflight_kills_group_including_grandchild` — fake bb spawns
`sleep 300 &`; assert child+grandchild dead [mutation: drop process_group → grandchild survives].
(4) windows cfg-gated `job_object_kills_on_handle_close` — VERIFY it executes in the
windows-build job log before merge (repo lesson). (5) `prove_timeout_kills_and_reaps` (100ms
injected). (6) updater helper unit: `pre_install_containment_confirms_or_aborts`. (7) panic sink
unit (format+append+sync); hook-fires test via thread panic; release-profile subprocess test
REFUSED (over-engineering; sink is innocuous beyond unit coverage). (8) tray prod-menu contains
show_logs. All bb tests: fake-bb + `BB_BINARY_PATH` + default `#[serial]` bucket.

### B5 — real uninstall (`b5-real-uninstall`)

Files: `src-tauri/src/{main.rs,uninstall.rs(new)}`, `src-tauri/nsis/hooks.nsi` (+harness),
`tests/prepare_uninstall.rs` (new), README, `docs/PLATFORM_SUPPORT.md`, `scripts/
prepare-uninstall.{sh,ps1}` (thin locate-and-invoke wrappers).

1. **F1 hybrid, no new NSIS hook** (all legs): (a) `--prepare-uninstall` early-argv flag →
   `uninstall::prepare_uninstall() -> Report`: check `updater.lock` (non-blocking; live update →
   abort with message, exit 1); `autostart::set_enabled_at(None,false)` (removes entry +
   disables crash recovery via OFF branch); `trust::remove_ca_trust()`; delete `certs/`, update
   markers (only when no live txn), CA-TRUST-NOT-REMOVED.txt; per-step status lines; exit 1
   unless all confirmed (–remove-ca-trust exit-code contract). (b) POSTUNINSTALL additions,
   NSIS-native inline: verified `schtasks /Delete /F` (+`/Query` re-check), `DeleteRegValue`
   Run + StartupApproved value, marker deletes — guards (`$UpdateMode`, `$EXEDIR != $INSTDIR`)
   and CA breadcrumb preserved. PREUNINSTALL refused (ratified F-05).
2. **F2 accept+document** (main+fable majority; codex dissent ledgered D-C2): unconditional
   removal; a surviving copied install self-heals via startup_reconcile/#429 on next launch;
   doc comment + README note. (Ownership-aware removal rejected as corner-case code with a
   self-healing failure mode.)
3. **F3 precise scope** (all legs): REMOVE certs/, markers, breadcrumb; KEEP config.json +
   approved origins (user data; security note in README: full removal = delete
   `~/.aztec-accelerator` — old approvals otherwise reapply on reinstall), updater-state.json
   (anti-rollback floor), locks. deb/DMG/AppImage: documented flows invoking the flag BEFORE
   deleting the app (ratified S6: no root postrm).

Tests: (1) `tests/prepare_uninstall.rs` per-OS `#[ignore]` L3 (autostart_heal shape, throwaway
HOME, `--test-threads=1`), invoking the REAL binary via `CARGO_BIN_EXE_AztecAccelerator`
(establishes the missing argv-branch pattern): arm everything → run → all gone → run again →
still gone [mutation: skip any callee → named assert fails]; wired into the 3-OS cert-trust
legs. (2) NSIS harness extension: seed real Run value + StartupApproved + scheduled task →
uninstall.exe → gone; update-mode run → preserved [mutation: drop inline deletes]. (3) CA-path
regression lines stay green.

### B7 — SDK contract (`b7-sdk-contract`)

Files: `packages/sdk/{package.json,src/lib/{accelerator-prover.ts,accelerator-transport.ts,
types.ts,errors.ts(new)},src/index.ts,src/lib/public-contract.test.ts,README.md,.claude skill,
MIGRATION.md}`, `scripts/prepare-sdk-publish.ts` (new, replaces inline `node -e`),
`.github/workflows/{sdk.yml,_publish-sdk.yml,promote-latest.yml}`, `packages/sdk/
consumer-fixtures/{node-tsc,vite}` (committed).

1. **F13 — DISPUTED → evidence-gated position: exact-pinned PEERS** (codex) **iff the fixtures
   prove it; else keep exact-pinned deps** (main+fable). Rationale for the lean: the
   `X.Y.Z-revision.N` scheme MEANS "for Aztec X.Y.Z"; README already documents peer semantics;
   loud ERESOLVE beats silent nested-dup wire mismatch; exact peers keep the frozen-axis
   supply-chain story. Cohort task 1 = import audit (which @aztec/* are directly imported) +
   both fixture variants green (host-provided exact match; host-absent → clear failure).
   Decision committed in-cohort, ledgered. (D-C3)
2. **F14 — one error class + refined fallback table** (consolidated): `AcceleratorHttpError
   {status, serverCode?, cause}` exported from barrel. Contract: **fallback to WASM** = network,
   403 auth-family (`denied` phase), 403 `version_not_allowed` (phase reason
   `version-mismatch` — availability preserved, visibility via phase), 408, 413, 429, 503, 500
   with RECOGNIZED code (`download_failed`/`prove_failed`), malformed-2xx. **Typed throw** =
   400 `invalid_version`/`invalid_origin` (caller bugs must surface) + unrecognized
   status/codes. (Refines the deliberate 503-only rule via the server's own code field — the
   original "don't mask misconfig" rationale is preserved for UNRECOGNIZED responses.)
   SKILL.md:145-153 corrected; README status→behavior table incl. all four 403 variants.
3. `appVersion`/`apiVersion` optional fields on the available arm; `#classifyHealth` stops
   discarding them; per-language constants centralized (TS `SUPPORTED_ACCELERATOR_API_VERSION`,
   Rust already has its own); NO cross-language codegen.
4. **F15**: `MIGRATION.md` added to `files`; publish attaches it as release asset + notes-file;
   consumer job asserts it's in `npm pack` output.
5. Tarball consumer CI job (sdk.yml → joins sdk-status): build → `prepare-sdk-publish.ts` (same
   script publish uses — the job must test the SHIPPED shape) → `npm pack` → install tgz into
   both fixtures → tsc/vite build. Node 24 (matches publish env; first and only Node pin in
   test CI, deliberate).
6. **F17**: publish via publish-testnet chain (e2e-gated) AFTER desktop promote; `latest` moved
   only by explicit promote-latest.yml dispatch; `-revision.N` dist-tag policy documented in
   runbook + promote-latest header ("`^` consumers never auto-receive revisions; `latest`
   promotion is the distribution lever").

Tests: error-table unit suite (each row one case; mocked ky) + `no_raw_ky_error_escapes`
[mutation: revert wrap]; `version_not_allowed_falls_back_with_version_mismatch_reason`
[mutation: revert reclassify]; health-parse carries app/api version; public-contract additions
(exports, corrected claims, no-peer-lie); the consumer job itself [mutation: drop dist from
files → job fails].

### B6 — publish/promote split + recovery (`b6-promote-split`)

Files: `.github/workflows/release-accelerator.yml`, `.github/actions/promote-feed/action.yml`
(new composite), `docs/RELEASE_RUNBOOK.md`, root `CLAUDE.md`, `packages/landing/src/main.ts`,
`scripts/release-workflow-contract.test.ts` (new).

1. **F12 — split + recovery as a MODE of the same workflow** (codex's IAM argument adopted: the
   OIDC role trust may be bound to this workflow's identity; a second top-level workflow risks
   an avoidable IAM change we cannot verify from the repo): dispatch inputs gain
   `mode: release|promote-only` + `promote: bool (default true)`.
   - `release` mode: publish (:792-979, minus AWS) → `verify-published-assets` step (download,
     count=17 incl. latest.json, sha256 vs build outputs) → **promote-feed job**
     (`id-token:write + contents:read` ONLY; AWS secrets REMOVED from the release job):
     captures current live feed → uploads it as `previous-latest.json` release asset
     (first-transition gap closed; belt over fable's asset-source correction) → uploads new
     latest.json + CloudFront invalidation. `verify-live-feed` needs promote-feed. **GitHub
     "Latest" badge marked AFTER promote in a separate `contents:write` job** (publish always
     creates `--latest=false`).
   - `promote-only` mode (THE freeze/fix-forward/re-point lever): input version → asserts
     stable semver + release exists → `gh release download --pattern latest.json` (or
     `previous-latest.json` for re-point) → production-verifier check → same promote-feed
     composite. Idempotent.
   - Delete-recreate: RC dispatches keep clean-slate; **stable refuses if the release already
     exists** (fail loud).
   - Freeze = dispatch with `promote:false` (publish-without-promote). No repo-variable second
     mechanism.
2. **F16**: landing filters `draft`/`prerelease`.
3. Runbook rewrite: real job graph; publish/promote/recovery procedures (fix-forward canonical;
   re-point protects only not-yet-updated clients — Layer-B floor stated honestly);
   delete-and-restore advice REMOVED; **manual trust-install pre-release checklist ADDED**
   (closes trust_{macos,windows}.rs dangling reference); CLAUDE.md job-graph summary refreshed
   same PR.

Tests (codex's YAML-contract idea adopted — mutation-provable guards where I had "innocuous
YAML"): `release-workflow-contract.test.ts` (tauri-identity.test.ts precedent) asserting: no
AWS creds in the publish job; promote gated on is_prerelease==false && promote==true;
verify-live-feed needs promote-feed; stable path has no release-delete; landing unit test
`latest_download_ignores_rc_and_draft`; promote composite fixture test (wrong version/unsigned
feed/missing asset → refuse); actionlint. Live validation: `auth_probe` dispatch (graph shape),
the RC chain (publish path), post-release idempotent re-promote (the drill).

### B4 — shipped-product + migration gate (`b4-product-gate`) — LAST

Files: `core/src/config.rs` (+tests), `.github/workflows/{_e2e-updater*.yml}` (extended into
combined per-OS gates), updater-smoke scripts (seed/assert/uninstall stages),
`packages/playground/e2e/installed-accelerator.spec.ts` (+runner), `e2e-gate.yml` (thin
dispatcher for soak re-runs).

1. **Config migration FIRST** (fable+codex mechanics over main's serde-alias): raw
   `serde_json::Value` pre-pass — `safari_support` → `https_enabled` only when the new key is
   absent (new key wins); strip legacy key; bump `CONFIG_VERSION = 2` and **read** it (v1 →
   migrate; v2 → as-is; **greater/malformed → best-effort read-only, never persisted over**);
   preserve origins/speed/auto_update/auto_approve_localhost/onboarding_version. The two tests
   pinning the old drop are REWRITTEN to pin the migration (deliberate owner-ratified
   reversal). Smoke-seed `safari_support`/`https_enabled` drift reconciled.
2. **F11 — extend the updater smokes into combined per-OS installed-product gates** (codex+
   fable over main's separate legs): one job per OS = the real user journey: install pinned
   **1.0.7** (port Windows' 4-point preflight to mac/linux) with a SEEDED legacy profile
   (`safari_support:true`, ≥1 approved origin, speed≠default, auto_update, autostart enabled)
   → update to candidate → assert state survived (version, https_enabled true via migration in
   the REAL packaged binary, origins, floor bumped, /health, HTTPS port serving) → **packed-SDK
   real-browser proof over HTTPS** (playground fixture consuming B7's tarball; asserts native
   path, no WASM fallback) → quit → uninstall (Windows: real uninstaller /S; mac/linux:
   `--prepare-uninstall`) → assert artifacts gone. `e2e-gate.yml` dispatches these for soak
   loops.
3. **F10 browser trust per-OS** (fable's matrix + codex's macOS ambition, timeboxed): Linux =
   NSS certutil + Chromium (proven). Windows = import app CA into **LocalMachine\Root**
   (proven in-repo pattern; Chromium trusts it; avoids the CurrentUser freeze). macOS =
   **Firefox + profile-NSS certutil** (same mechanism the app's Linux trust walks; no
   Keychain); spike codex's fresh-login-keychain route 1 day IF Firefox route fails; fallback
   = `curl --cacert` HTTPS native-proof + documented residual. Residual on all OSes either
   way: the app's OWN interactive trust dialogs (manual runbook checklist, codex-concurred).

Tests: migration unit trio [mutation: revert Value-pass] + `v2_and_future_configs_not_clobbered`;
the three combined per-OS gates ARE the decisive E2E set (one assertion battery per OS, not a
pyramid). Nomenclature sweep = checklist execution logged in release notes (recon: machinery
already clean; no automation built).

## Release execution

1. Version PR: source → `2.0.0-rc.1` (tauri.conf + src-tauri/server Cargo.toml+locks; core
   Cargo.toml cosmetic 2.0.0; README example refresh rides along).
2. Dispatch `2.0.0-rc.1` (release mode): build → all gates (3-OS combined installed-product
   incl. 1.0.7 upgrade + HTTPS proof + uninstall) → GitHub prerelease published; feed untouched
   (landing now filters). PushNotification.
3. Fix-forward through rc.N; EVERY RC gets the full gate suite.
4. **Soak ≥2h** on the chosen RC: repeated `e2e-gate.yml` cycles (≥4 consecutive green spanning
   ≥2h) while MIGRATION/release-notes/runbook finalization happens in parallel.
5. Codex release-readiness review (fresh session) → clean round required.
6. Version PR → `2.0.0`; dispatch stable (promote:true default): rebuild → full regate →
   publish (+previous-latest capture, asset verify) → promote-feed → verify-live-feed →
   GitHub-Latest marked. PushNotification.
7. Recovery drill: `promote-only` re-promote of 2.0.0 (idempotent, zero feed change) — proves
   the lever live; fixture tests already proved refusal paths.
8. SDK: publish-testnet chain → `-revision.N` on npm → fresh-dir install+typecheck of the EXACT
   registry version → promote-latest.yml → PushNotification.
9. Post-release: fresh-install smokes (3 OS, public links); live-feed 1.0.7→2.0.0 update smokes
   (dynamic-latest now resolves 2.0.0); bump-source auto-PR lands 2.0.1-rc.1.
10. Close-out per brief §8 + post-v2 backlog (below).

Rollback story (runbook-canonical): pre-promote failure is fleet-harmless (freeze =
promote:false). Post-promote: `promote-only` re-points the feed to the prior stable's
`previous-latest.json`/asset feed for not-yet-updated clients; already-updated clients are
fix-forward-only (Layer-B monotonic floor — stated, no machinery pretends otherwise);
crash-on-start N requires communicated manual reinstall. Never delete tags/releases.

## Risk register

1. 3-OS gate flake (installs, CDN, GH API) → built on proven smoke bones; bounded waits; retry
   only infra-attributed failures (logged); full suite per new RC; 3-strikes → codex consult.
2. Windows Job Object compiles-but-never-runs → cfg-gated test + verify execution in
   windows-build job log before B3 merges.
3. macOS browser-trust route unproven → Firefox-NSS first, keychain spike second, curl-cacert
   fallback third; residual documented.
4. Guard default-flip breaks an existing spec → full Playwright+wdio suites in cohort CI;
   settings.js confirmed outside wireButton.
5. Stable rebuild ≠ soaked RC artifacts (existing model) → same commit, full regate on the
   stable build; provenance binding stays out of scope (backlog).
6. Peers decision destabilizes consumers → evidence-gated in-cohort; fixtures prove both
   configurations before commitment; revert path = keep-deps (one manifest diff).

## Post-v2 backlog (write to implementations-plan/index.md at close-out)

n1-version default bump to 2.0.0 (+drop N1BinaryName override); peer-migration design round (if
B7 lands keep-deps) or dep-consolidation follow-ups (if peers); settings.js guard; macOS/Windows
interactive-trust automation research; aztec-stable cron dormancy; npm Trusted Publisher; SF
provenance binding (tag↔shipped-version); Mainnet version-cache LRU; update-failure UI; tray
error states; F-09 replay freshness; independent SDK semver debate.

## Scope cuts (consolidated refusals)

B1/Authenticode; TUF/F-09; telemetry/diagnostic bundles; bb revocation/sandboxing/supervisor
frameworks; queue fairness + aggregate budgeting + download singleflight; independent SDK
semver; npm Trusted Publisher; TS↔Rust api_version codegen; PREUNINSTALL hook; ownership-checked
uninstall (ledgered dissent); `--purge` flag / deleting user config; deb postrm; S3
versioning/infra; multi-Node matrix; release-profile panic subprocess test; full
app-restart-mid-proof automation; nomenclature sweep automation; new release channels;
rewriting smoke scripts into a framework.
