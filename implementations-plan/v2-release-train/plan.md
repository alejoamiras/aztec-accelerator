# plan.md — v2-release-train (rev 2, post contradiction-check)

Status: rev 2 — all 20 contradiction-check findings folded (codex 12, fable 8; overlaps merged;
every disposition ledgered). Awaiting double audit (codex + fresh fable), then final fresh codex.
Legs: `plan-main.md`, `plan-fable.md`, `plan-codex.md`. Binding: `brief.md` (D-R1..5),
`recon.md` (F1–F17), `decision-ledger.md`.

## Phase 0 answers (owner-ratified via brief)

Success = goal DONE block. Quality bar: production. Validation layers: cargo/bun unit +
integration, Playwright (mocked UI), NSIS harness, L3 `#[ignore]` per-OS integration, 3-OS
installed-product gates, actionlint + workflow-contract tests, full release pipeline. Untestable
⇒ only if innocuous. Security-relevant fixes mutation-proved.

## Verified constraints discovered post-rev-1 (contradiction round)

- `autostart` reconciliation NEVER resurrects an Absent entry (autostart.rs:17-18, :1563) — the
  rev-1 "self-heal" rationale was false.
- `classify_launch_https` → `UntrustedSkip`: the app does NOT serve HTTPS unless its OWN trust
  predicate passes (login keychain / CurrentUser\Root / NSS). Browser-side trust alone cannot
  light the HTTPS listener on mac/win CI.
- `gh release upload` requires `contents:write` — the promote job cannot attach assets.
- npm ≥7 auto-installs missing peers — a host-absent fixture cannot decide F13.

## Architecture & Implementation (per cohort)

Order: **B2 → B3 → B5 → B7 → B6 → B4 → release** (all legs agree; F16 rides in B6; B3+B5 both
touch main.rs → expect rebase). One worktree/branch/stack per cohort; merge one PR at a time on
fresh main, single green run per context.

### B2 — consent hardening (`b2-consent-guard`)

1. Guard default-on in `wireButton` (`opts.guard !== false`); export `rearmClickGuard()`;
   native focus/pageshow rearm kept. settings.js deferred (tray-initiated; backlog).
2. F7: `wasActive` latch in `refreshPending()`; rearm exactly on false→true before
   `setControlsEnabled`.
3. F8 version-echo, type-enforced: `PendingUpdate::take_matching(&displayed)`; blind `take()`
   deleted (compile-time binding). Mismatch: retain pending, `SECURITY:` log, close + re-show
   with current pending version.
4. F9 cooldown: **new `ProveError::AuthorizationCooldown` → 429 `authorization_cooldown`**
   (D-C4 RESOLVED codex's way — D-C7 makes recognized 429 fall back to WASM, so nothing
   surfaces to dApps, and we stop emitting a false `denied` phase for a cached decision).
   `DENY_COOLDOWN = 30s` injectable const beside MAX_PENDING_ORIGINS; per-CanonicalOrigin
   timestamps in PendingState; recorded in `resolve`/`resolve_active` on Deny (click/timeout/
   close); pruned on `request`, capped map; checked in `server/auth.rs` after `is_approved`.
   Fairness DEFERRED.

Tests: (1) Playwright guard spec (onboarding/renewal/update; `__CLICK_GUARD_MS__=200`;
immediate→absent, waited→fires) [mut: revert default-on]. (2) Playwright authorize
rearm-on-promotion [mut: remove latch]. (3) Rust take_matching match/mismatch+retained (blind
take = compile error — the proof). (4) Playwright update spec: payload carries DOM version.
(5) Rust `deny_cooldown_blocks_immediate_retry` + `cooldown_recorded_on_timeout_and_close` +
wire-shape row for 429 authorization_cooldown [mut: revert auth.rs check]. (6) unit pin
`DEFAULT_GUARD_MS == 700`. wdio real-timing spec DROPPED (flake-prone duplicate; fable c-8).

### B3 — bb containment (`b3-bb-containment`)

1. F4 cap-and-continue drain (≤64KiB retained, EOF-drained, total counted) concurrent with
   `child.wait()` under injectable timeout (`prove_inner(timeout)`).
2. F5 validate in `bb::prove`: empty / non-32-aligned / >64MiB proof → Err → ProveFailed.
3. F6: Unix `process_group(0)` + registry + `terminate_inflight()` (group-SIGKILL; also in
   timeout branch); Windows static Job Object `KILL_ON_JOB_CLOSE` (covers NSIS-handoff
   `exit(0)` + crashes); explicit calls at tray-quit (kill, log, never block quit) and
   `perform_update` pre-`install()` (unconfirmed reap → ABORT update). Linux PDEATHSIG kept
   **with documented thread-scope caveat** (fires if the spawning thread dies; benign here —
   group-kill/Job Object are primary; fable dissent ledgered D-C8). `renew_cert` refuse kept.
4. Panic hook: sync append+`sync_all` to `log_dir()/panic.log`, best-effort tracing, chains
   prior hook. Show Logs in prod menu.

Tests: stderr-spam bounded [mut: cap]; proof triple+oversize [mut: validation]; unix group-kill
incl. grandchild [mut: drop process_group]; windows Job Object cfg-test **verified to execute
in windows-build job log**; timeout kill (100ms injected); updater pre-install containment
helper unit; panic sink unit + thread-panic hook test (release-profile subprocess REFUSED);
tray prod-menu has show_logs. Fake-bb + BB_BINARY_PATH + default serial bucket.

### B5 — real uninstall (`b5-real-uninstall`)

1. F1 hybrid (unchanged): `--prepare-uninstall` early-argv flag → `uninstall::prepare_uninstall()
   -> Report`; updater.lock check first (live update → abort, exit 1); per-step status; exit 1
   unless confirmed. NSIS POSTUNINSTALL native inline additions; PREUNINSTALL refused (F-05).
2. **F2 FLIPPED → ownership-aware removal (D-C2 revised; codex was right, self-heal was
   false)**: CLI removes autostart/recovery only when the stored entry targets THIS install
   (reuse #429 probes; `Absent` ⇒ idempotent success; `points_elsewhere`/`Unreadable` ⇒ leave +
   report "foreign entry left"). NSIS: `ReadRegStr` the Run value → delete only if it
   references `$INSTDIR` (canonicalized); task delete gated on `schtasks /Query /XML` output
   containing `$INSTDIR` (FindStr); StartupApproved value deleted only alongside its Run value.
3. F3 scope unchanged: remove certs/, markers, breadcrumb; keep config.json/origins (README
   security note + manual full-purge path), updater-state.json, locks. deb/DMG/AppImage
   documented flows (+ thin `scripts/prepare-uninstall.{sh,ps1}` locate-and-invoke wrappers).

Tests: (1) `tests/prepare_uninstall.rs` per-OS L3 via `CARGO_BIN_EXE_AztecAccelerator`: arm →
run → gone → run again → still-gone [mut: skip any callee]; PLUS `foreign_entry_left_untouched`
(seed entry pointing elsewhere → preserved + reported) [mut: drop ownership check]. (2) NSIS
harness: ours-seeded → gone; foreign-seeded → preserved; update-mode → preserved [mut: drop
inline deletes / drop $INSTDIR match]. (3) CA-path regression green.

### B7 — SDK contract (`b7-sdk-contract`)

1. **F13 evidence-gated with PREDECLARED acceptance criteria (codex c-10)**: adopt exact-pinned
   PEERS iff (a) exact-match host: install+typecheck+build green AND `npm ls` shows a SINGLETON
   @aztec graph; (b) conflicting host (@aztec 5.0.0): loud `ERESOLVE` failure; (c) tarball
   consumers stay green. Host-absent behavior is explicitly NOT a criterion (npm auto-installs
   peers). Fail any → keep exact-pinned deps; decision + evidence ledgered in-cohort.
2. F14 error contract (D-C7) + cooldown row: fallback = network, 403 auth-family (`denied`),
   403 `version_not_allowed` (reason `version-mismatch`), 408, 413, **429 incl.
   `authorization_cooldown` (NO `denied` phase — cached decision, not a new one)**, 503, 500
   w/ recognized code, malformed-2xx. Typed `AcceleratorHttpError` = 400
   invalid_version/invalid_origin + unrecognized. SKILL/README corrected; public-contract pins.
3. appVersion/apiVersion surfaced; per-language constants; no codegen.
4. F15: MIGRATION.md in `files`, attached to release; consumer job asserts presence in pack.
5. `scripts/prepare-sdk-publish.ts` shared by publish + consumer job; fixtures (node-tsc, vite,
   + the conflicting-host fixture) on Node 24; joins sdk-status.
6. F17: publish-testnet chain after desktop promote; `latest` via promote-latest.yml;
   dist-tag policy documented.

Tests: error-table suite incl. authorization_cooldown row + `no_raw_ky_error_escapes` [mut:
revert wrap]; version_not_allowed fallback-reason [mut: revert reclassify]; health-parse
carries versions; public-contract additions; consumer job [mut: drop dist from files];
peer-criteria fixtures (if adopted) [mut: unpin a peer → ERESOLVE fixture fails].

### B6 — publish/promote split + recovery (`b6-promote-split`)

1. D-C5 revised for permissions truth (codex c-4 / fable c-1): **publish job** (contents:write)
   captures the current PUBLIC CDN feed pre-flip (curl, no AWS creds), verifies it with the
   production verifier, uploads it as `previous-latest.json` release asset alongside
   `latest.json` at `gh release create` time. **promote-feed job** stays `id-token:write +
   contents:read`: downloads release assets, verifies, flips S3 + CloudFront. Asset-count
   gates: **16 for RC, 18 for stable** (codex c-5).
2. **Immutability everywhere (codex c-7)**: delete-recreate REMOVED for RCs too — any failed/
   dirty publish → next `rc.N+1`; stable re-dispatch refuses if release exists. Partial-publish
   recovery = runbook additive `gh release upload` procedure + promote-only's hard pre-flight
   (every latest.json platform URL must resolve) (fable c-4; codex's resume-existing mode
   rejected as extra machinery — ledgered).
3. `promote-only` mode (freeze/fix-forward/re-point lever, same workflow per IAM argument):
   inputs `version`, **`source: candidate|previous`** (codex c-8); pre-flight asset-URL
   resolution; production-verifier check; flip; then **verify-live-feed + mark-GitHub-Latest +
   bump-source run in this mode** (fable c-3: they belong to promotion, not publication).
   `promote:false` renamed semantics: **hold** (pre-promotion), not "freeze"; runbook defines
   incident freeze = hold + no promote-only dispatch.
4. **Landing switches to the `/releases/latest` endpoint** (fable c-2; subsumes F16): Latest
   badge is marked only post-promote, so landing is promotion-aligned for RC/soak/hold/normal
   alike. Prerelease/draft list-filter dropped as dead code path.
5. Declaration-equality gate (codex c-9): validate asserts tauri.conf + all three Cargo
   versions == dispatch input.
6. Runbook rewrite (real graph, hold/promote/re-point/fix-forward procedures, Layer-B floor
   honesty, additive-upload recovery, manual trust-install pre-release checklist) + CLAUDE.md
   refresh same PR.

Tests: `release-workflow-contract.test.ts` — no AWS creds in publish; promote gated
prerelease-false; per-channel asset counts 16/18; no `gh release delete` anywhere; promote-only
requires source; verify-live-feed/mark-Latest/bump-source wired to promotion [each mutation-
provable by editing the YAML]. Landing unit `latest_endpoint_used`. Promote composite fixture
test (wrong version/unsigned/missing-asset → refuse). actionlint. Live: auth_probe graph check;
RC chain exercises publish; the post-release drill exercises promote-only both sources.

### B4 — shipped-product + migration gate (`b4-product-gate`) — LAST

1. Config migration (unchanged mechanics) **+ future-version save protection (codex c-11)**:
   `config_version > CONFIG_VERSION` at load → in-memory read-only flag; ALL saves rejected
   (surfaced as settings error + log) so a future schema is never clobbered by a v2 process.
   Tests: migration trio + `future_config_never_persisted_over` [mut: drop flag].
2. Per-OS combined gate, now TWO stages (fable c-5 restores the silently-merged deliverable):
   **(a) fresh-install stage**: install candidate → first-run (v2 config seed) → /health →
   native proof; **(b) upgrade stage**: wipe → install pinned 1.0.7 with seeded legacy profile
   → update → assert state survived (origins, https_enabled via migration, speed, auto_update,
   floor bumped, autostart entry) → packed-SDK browser proof → **full uninstall**: Windows real
   uninstaller /S; macOS/Linux `--prepare-uninstall` THEN package removal (`rm -rf` .app /
   AppImage, `sudo dpkg -r` for deb) (codex c-12) → assert binary gone + artifacts gone,
   **scoped to app-owned stores** (fable c-6).
3. **HTTPS proof matrix REVISED (codex c-3 verified — UntrustedSkip)**: the app serves HTTPS
   only if ITS OWN trust predicate passes. Linux: certutil into user NSS (the app's own store)
   → app serves HTTPS → full browser proof over `https://localhost:59834`. macOS: 1-day spike
   on non-interactive login-keychain trust (fresh keychain + unlock + add-trusted-cert); works
   → full proof; else → residual. Windows: CurrentUser\Root is interactive (empirical freeze)
   and widening the app's trust predicate to LocalMachine for testability is REFUSED
   (production security surface) → residual. Where residual: browser proof runs over HTTP,
   TLS layer stays covered by existing `tls_handshake.rs` integration tests + Linux's full
   HTTPS leg; upgrade-stage HTTPS assertions per-OS: config-file migration asserted everywhere,
   listener-up asserted only where trust is seedable. Residuals documented in the audit report
   with codex concurrence + the runbook manual checklist (B6).
4. `e2e-gate.yml` thin dispatcher for soak loops.

## Release execution (rev 2 — fable c-3 sequencing fix)

1. Version PR: **lockstep** `2.0.0-rc.1` across tauri.conf + src-tauri/server/core Cargo.toml
   (+locks) (codex c-9); README example refresh rides along.
2. Dispatch release (`2.0.0-rc.1`): build → publish (16 assets, immutable) → 3-OS gates
   (fresh-install + upgrade stages). Gate failure → fix → `rc.N+1` (never delete). Push
   notification at RC published.
3. Soak ≥2h on the final RC: repeated `e2e-gate.yml` cycles (≥4 consecutive green spanning
   ≥2h) while MIGRATION/notes/runbook finalize in parallel.
4. Codex release-readiness review (fresh session) → clean round required.
5. Version PR → `2.0.0` lockstep. **Dispatch stable with hold (`promote:false`)**: rebuild →
   full regate → publish (18 assets incl. latest.json + previous-latest.json) →
   verify-published-assets. The fleet is untouched.
6. Run the 3-OS gate suite against the PUBLISHED stable assets (the real bytes, pre-promote —
   this is the "full regate on the stable build" made mechanically true).
7. **Promote**: dispatch `promote-only source:candidate version 2.0.0` (autonomous, D-R1) →
   flip → verify-live-feed → mark GitHub Latest → bump-source PR. PushNotification.
8. **Recovery drill (real, bounded)**: `promote-only source:previous` (feed → 1.0.7's manifest;
   not-yet-updated clients unaffected, updated clients floor-protected) → verify →
   `promote-only source:candidate` restore → verify. Total exposure: minutes; documented in
   runbook as the rehearsed procedure.
9. SDK: publish-testnet chain → `-revision.N` → fresh-dir install+typecheck of the exact
   registry version → promote-latest.yml → PushNotification.
10. Post-release: fresh-install smokes (3 OS, public links); live-feed 1.0.7→2.0.0 update
    smokes; close-out per brief §8 + post-v2 backlog.

Rollback story: pre-promote failure is fleet-invisible (hold). Post-promote: promote-only
source:previous stops further uptake; updated clients are fix-forward-only (Layer-B floor,
stated); crash-on-start N ⇒ communicated manual reinstall; never delete tags/releases.

## Risk register

1. 3-OS gate flake → proven smoke bones, bounded waits, infra-attributed retries logged, full
   suite per RC, 3-strikes → codex.
2. Windows Job Object compiles-but-never-runs → verify execution in windows-build log pre-merge.
3. macOS keychain spike fails → residual path predefined (HTTP proof + tls_handshake + Linux
   HTTPS leg); no schedule slip.
4. Guard default-flip breaks a spec → full Playwright suite in cohort CI; settings.js outside
   wireButton (verified).
5. Ownership-gated NSIS deletes leave residue on odd install layouts (8.3 paths, moved
   installs) → canonicalize both sides ($EXEDIR pattern exists); foreign-left is REPORTED, and
   the CLI path remains available; harness covers ours/foreign/update-mode.
6. Peers criteria ambiguous in practice (ERESOLVE behavior varies by npm config) → fixtures pin
   npm version + `--strict-peer-deps` explicitly; falls back to keep-deps on any ambiguity.
7. Stable rebuild ≠ soaked RC artifacts → same commit, full regate on the PUBLISHED stable
   bytes pre-promote (step 6 now mechanically real).

## Post-v2 backlog

n1-version default → 2.0.0 (+drop N1BinaryName); peer-migration or dep-consolidation follow-up;
settings.js guard; macOS/Windows interactive-trust automation research; aztec-stable cron
dormancy; npm Trusted Publisher; provenance binding; Mainnet version-cache LRU; update-failure
UI; tray error states; F-09; independent SDK semver.

## Scope cuts

B1/Authenticode; TUF/F-09; telemetry; bb revocation/sandboxing/supervisors; queue fairness;
independent SDK semver; npm Trusted Publisher; api_version codegen; PREUNINSTALL hook; `--purge`;
deb postrm; S3 versioning; multi-Node matrix; release-profile panic subprocess test; full
restart-mid-proof automation; nomenclature automation; new channels; smoke-script framework
rewrite; resume-existing release mode; widening the app trust predicate for testability.
