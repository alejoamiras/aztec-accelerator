# plan.md — v2-release-train (rev 3, post double-audit)

Status: rev 3 — double-audit folded (codex round-2: 12 findings incl. 1 BLOCKER; fresh hostile
fable: 19 findings incl. 4 HIGH; overlaps merged). ONE open dispute for the final fresh codex
pass (B4 HTTPS residuals, below). Legs: `plan-main.md`, `plan-fable.md`, `plan-codex.md`.
Binding: `brief.md` (D-R1..5), `recon.md` (F1–F17), `decision-ledger.md` (through D-C24).

## Phase 0 answers (owner-ratified via brief)

Success = goal DONE block. Production bar. Validation layers: cargo/bun unit + integration,
Playwright (mocked UI), NSIS harness, L3 `#[ignore]` per-OS, 3-OS installed-product gates,
actionlint + workflow-contract tests, full release pipeline. Untestable ⇒ only if innocuous.
Security-relevant fixes mutation-proved.

## Load-bearing verified facts (accumulated)

- Absent autostart entries are NEVER resurrected (autostart.rs:17-18,:1563).
- `classify_launch_https` → UntrustedSkip: app serves HTTPS only if ITS OWN trust predicate
  passes (login keychain / CurrentUser\Root / user NSS).
- `gh release upload` needs contents:write; npm ≥7 auto-installs peers; PDEATHSIG is
  thread-scoped; deployed old SDKs throw raw on every non-403/503 HTTPError
  (accelerator-prover.ts:535) — wire changes must stay 403/503-shaped for fallback semantics.
- CONFIG_VERSION policy: additive fields never bump it; a bumped (v3) config is by-policy
  unparseable by v2 and load_from is fail-open — version gating must probe-parse.
- `accelerator-v1.0.7` carries `latest.json` as a release asset (verified 2026-08-16) — every
  stable release ships its own signed feed; rollback needs NO snapshot capture.
- PendingUpdate today is a bare `Arc<Mutex<Option<VerifiedUpdate>>>` (commands.rs:34) — a
  compile-time consent-binding claim requires a NEWTYPE.

## Architecture & Implementation

Order: **B2 → B3 → B5 → B7 → B6 → B4 → release**. One worktree/branch/stack per cohort; merge
one PR at a time on fresh main, single green run per context; B3+B5 rebase expected (main.rs).

### B2 — consent hardening (`b2-consent-guard`)

1. Guard default-on in `wireButton` (`opts.guard !== false`); export `rearmClickGuard()`;
   focus/pageshow rearm kept; settings.js deferred (backlog).
2. F7: `wasActive` latch; rearm exactly on false→true before `setControlsEnabled`.
3. F8 **newtype**: `PendingUpdate` wraps a private `Mutex<Option<VerifiedUpdate>>`; the ONLY
   extractor is `take_matching(&displayed_version)` (mismatch ⇒ retained + None) — the
   compile-time claim is now true (H4). Mismatch recovery: **navigate the existing
   update-prompt window to the refreshed versioned URL** (no close+re-show race, M1);
   `SECURITY:` log.
4. F9 cooldown — **403 + code `authorization_cooldown`** (H1 supersedes the 429: deployed old
   SDKs hard-throw on non-403/503; 403 keeps them on the denied→WASM path — a cached denial IS
   a denial; the NEW SDK recognizes the code and falls back without emitting a fresh `denied`
   phase). New `ProveError::AuthorizationCooldown`. `DENY_COOLDOWN = 30s` injectable const;
   per-CanonicalOrigin timestamps in PendingState; recorded in `resolve`/`resolve_active` on
   Deny (click/timeout/close); pruned on `request`; **capped map, drop-new at cap** (anti-nag,
   not a boundary — L2); checked in `server/auth.rs` after `is_approved`. Fairness DEFERRED.

Tests: (1) Playwright guard spec (3 popups, 200ms override) [mut: revert default-on].
(2) authorize rearm-on-promotion [mut: remove latch]. (3) newtype unit: mismatch ⇒ retained;
match ⇒ taken (blind take = compile error) + **command-layer test: mismatched echo ⇒ pending
retained AND update not spawned** [mut: bypass take_matching → this test fails]. (4) Playwright
payload-carries-DOM-version. (5) `deny_cooldown_blocks_immediate_retry` (through
`authorize_origin`) + `cooldown_recorded_on_timeout_and_close` + wire row: 403
`authorization_cooldown` stays text/plain [mut: revert check]. (6) unit pin
`DEFAULT_GUARD_MS == 700`.

### B3 — bb containment (`b3-bb-containment`)

1. F4 cap-and-continue drain (≤64KiB retained, EOF-drained, total counted), concurrent with
   `child.wait()` under injectable timeout.
2. F5 validate in `bb::prove`: empty / non-32-aligned / >64MiB → Err → ProveFailed.
3. F6 revised for the drop path + races (M2/M3 + codex r2 #3):
   - Unix: `process_group(0)`; **RAII registry guard whose Drop group-SIGKILLs** — so
     client-disconnect future-drop kills grandchildren too (kill_on_drop stays as belt for the
     direct child); explicit `terminate_inflight()` for tray-quit and pre-install.
   - **Reap/kill discipline**: registry entries removed under the killer's lock BEFORE reap
     acknowledgment; terminate only un-reaped entries (zombie pins the pgid) — no pgid-reuse
     kill.
   - Windows: static Job Object `KILL_ON_JOB_CLOSE`; assign immediately post-spawn +
     `IsProcessInJob` verify + **fail the prove if unassigned**; suspended-spawn REFUSED
     (heavy; bb is our marker-verified sidecar; microsecond window documented-accepted).
   - Call sites extracted into testable fns with injected registry (M5); pre-install:
     unconfirmed reap ⇒ ABORT update; quit: kill + log, never block.
   - Linux PDEATHSIG kept, thread-scope caveat documented. **macOS crash-orphan = explicit
     ledgered residual** (bb exits naturally; F-08a reaps; PROVE_TIMEOUT bounds app-alive).
4. Panic hook (sync append + `sync_all` to `panic.log`, chains prior hook); Show Logs in prod.

Tests (all through the REAL `prove_inner` spawn path — codex #10): stderr-spam with test-only
retained-bytes observation [mut: revert to wait_with_output → fails]; proof validation triple +
oversize; unix drop-path group-kill incl. grandchild (drop the future, assert tree dead) [mut:
remove Drop-kill]; unix explicit terminate test; **windows wiring test: fake-bb spawned via
prove path asserts `IsProcessInJob` + tree death** (M4; verified to EXECUTE in windows-build
log); timeout kill (100ms); call-site fns assert terminate-called (M5); **bounded
restart-mid-proof L3: fake-bb proving, invoke pre-restart containment, assert reap-confirmed**
(M8 — replaces the dropped full-app automation; ledgered); panic sink unit + thread-panic hook
test; tray prod-menu has show_logs.

### B5 — real uninstall (`b5-real-uninstall`)

1. F1 hybrid unchanged (`--prepare-uninstall` + NSIS-native inline; PREUNINSTALL refused).
2. Ownership matching hardened (codex r2 #5 + M12): CLI reuses #429 canonicalized probes
   (Absent ⇒ success; foreign/unreadable ⇒ leave + report). NSIS: parse the Run value's QUOTED
   EXE TOKEN, canonicalize, EXACT-equal against `$INSTDIR\AztecAccelerator.exe`; task XML:
   extract `<Command>`, XML-unescape, exact-equal; `FindStr /L /C:` literal-mode only.
   **One-time cohort task: resolve POSTUNINSTALL-vs-RMDir ordering against the actual
   tauri-bundler 2.8.1 template** (recon's open question) — canonicalization must not silently
   fail on a deleted `$INSTDIR` (GetFullPathName /SHORT errors on nonexistent paths).
3. **Foreign-install rule extended to shared state (codex r2 #6)**: if ANY foreign ownership
   is detected, ALSO skip certs/ + trust removal (shared across installs); report everything
   left and why; user guidance printed.
4. F3 scope unchanged (keep config/origins/floor/locks; docs + wrappers).

Tests: L3 per-OS via CARGO_BIN_EXE: arm → run → gone → rerun → still-gone [mut: skip callee];
`foreign_entry_leaves_everything_incl_certs` [mut: drop ownership check OR drop shared-state
skip]; NSIS harness fixtures: ours / deceptive-prefix / args-only / XML-escaped /
**spaces-in-path** / update-mode / **$INSTDIR-deleted-before-macro** [mut: drop exact-token
match]; CA regression green.

### B7 — SDK contract (`b7-sdk-contract`)

1. F13 evidence-gated exact-PEERS with predeclared criteria (D-C13) **+ criterion (d)
   patch-ahead host (M11)**: @aztec 5.0.2-style host fixture; PREDECLARED verdict = hard
   ERESOLVE refusal is ACCEPTED as the contract (aligned with the revision-scheme's "for Aztec
   X.Y.Z" semantics) — but the fixture result + verdict are ledgered before adoption; any
   surprise ⇒ keep-deps. **Exact npm version pinned** (`npm i -g npm@<exact>`) in evidence +
   publish jobs, recorded (codex r2 #11).
2. F14 table (rev 3): fallback = network, 403 auth-family (`denied`), 403 `version_not_allowed`
   (`version-mismatch`), **403 `authorization_cooldown` (fallback, NO fresh `denied` phase)**,
   408, 413, 429, 503, 500 w/ recognized code, malformed-2xx. Typed `AcceleratorHttpError` =
   400 invalid_version/invalid_origin + unrecognized.
3. appVersion/apiVersion surfaced; per-language constants.
4. F15 MIGRATION.md in `files` + release asset + pack assert.
5. `prepare-sdk-publish.ts` shared by publish + consumer job; fixtures node-tsc / vite /
   conflicting-host / patch-ahead; Node 24 + pinned npm.
6. F17 unchanged (publish-testnet chain after desktop GA; promote-latest moves `latest`).

Tests: error-table suite incl. cooldown row + `no_raw_ky_error_escapes` [mut: revert wrap];
version_not_allowed reason [mut: revert reclassify]; health-parse; public-contract additions;
consumer job [mut: files]; peer fixtures incl. (d).

### B6 — publish/promote split + recovery (`b6-promote-split`)

**Simplified by verified fact: every stable release ships its own signed latest.json asset —
the previous-feed CAPTURE IS DROPPED** (fable-fresh H2 alternative; kills codex r2 #1 BLOCKER
and #4 staleness class at the root).

1. Publish job (contents:write): create release `--latest=false` ALWAYS; assets = 16 (RC) /
   **17** (stable: 16 + latest.json); verify-published-assets step (names+sizes+sha256 vs
   build outputs). NO delete-recreate anywhere; failed publish ⇒ rc.N+1 (RC) or burned version
   (stable, below).
2. `promote-only` mode — **single `version` input, no source param**: pre-flight = release
   exists ∧ latest.json asset present ∧ production-verifier signature ∧ **feed_version ==
   input version** ∧ full asset completeness (count + every platform URL resolves). Flip S3 +
   invalidate. Then, ALWAYS keyed to the promoted version: verify-live-feed (asserts
   feed_version live) + mark-GitHub-Latest (re-badges THAT release — landing `/releases/
   latest` stays consistent even on rollback). `bump_source: bool` input (only the organic GA
   promote passes true) — no next-RC PR after a rollback (M6/codex r2 #1 resolved by
   construction). **Rollback = `promote-only version:1.0.7`** (uses 1.0.7's own signed feed
   asset — verified present).
3. **Burned-stable rule (M7)**: a held stable that fails its regate is BURNED — never promoted,
   never re-dispatched; fix-forward X.Y.Z+1. Stated in plan + runbook.
4. Append-only policy (codex r2 #7 + L1): workflow-contract rows ban `gh release delete` AND
   `--clobber` AND pin stable-create `--latest=false`; additive-repair runbook procedure
   forbids `--clobber` and ends with a promote-only pre-flight rerun (the full asset verifier).
5. Rehearsal (codex r2 #8): promote composite parameterized on S3 key → **pre-GA staging
   rehearsal** (promote candidate feed to `releases/rehearsal/latest.json`, verify, remove) +
   **post-GA live drill** (promote-only 1.0.7 → verify → promote-only 2.0.0 → verify; minutes,
   floor-protected).
6. Landing → `/releases/latest` endpoint. Declaration-equality gate (conf + 3 Cargo == input).
   Runbook rewrite + CLAUDE.md; manual trust-install pre-release checklist added.

Tests: workflow-contract rows (publish has no AWS creds; promote prerelease-false-gated;
16/17 per-channel asset gates; no delete; no --clobber; --latest=false pinned; bump_source
wiring; verify-live-feed keyed to promoted version) — each row mutation-provable by YAML edit;
landing endpoint unit; promote composite fixture tests (wrong version / unsigned / missing
asset / feed-version mismatch → refuse); actionlint; auth_probe graph dry-run.

### B4 — shipped-product + migration gate (`b4-product-gate`) — LAST

1. Config migration + **probe-parse version gate (H3)**: stage 1 = lenient
   `{config_version: u32}` probe (unknown fields ignored); probe > CONFIG_VERSION ⇒ read-only
   mode (ALL saves rejected + surfaced) regardless of full-parse outcome; stage 2 = full parse
   with `safari_support`→`https_enabled` Value-pass migration (new key wins), CONFIG_VERSION=2
   written. Tests: migration trio + `future_config_never_persisted_over` with a fixture that
   is **NOT v2-parseable** [mut: drop probe] + v2-untouched.
2. Per-OS combined gate, two stages (fresh-install; upgrade→proof→FULL uninstall incl. package
   removal). **Upgrade stage adds: CA trust still present after 1.0.7→2.0.0 where seedable**
   (M9 — brief-mandated; Linux now, macOS if spike; mac/win residual ledgered). Uninstall
   assertions scoped to app-owned stores.
3. HTTPS proof matrix (UntrustedSkip-derived) — **OPEN DISPUTE, package for final codex pass**:
   Linux = full packed-SDK browser proof over HTTPS via the app's own NSS store. macOS =
   MANDATORY keychain spike (fresh login keychain + unlock + add-trusted-cert), full proof if
   it lands; else residual. Windows = residual (CurrentUser\Root is interactively gated —
   empirical CI freeze; widening the app's trust predicate for testability REFUSED).
   Compensating evidence where residual: `tls_handshake.rs` on that OS runner (real rustls
   handshake of the app's TLS stack), upgrade-stage assertion that migration set
   `https_enabled` AND the app logged UntrustedSkip (gate logic proven), HTTP-transport native
   proof (bb path proven), and the runbook manual pre-GA checklist item. This invokes the
   brief's own residual clause ("remaining gaps documented as residuals with codex
   concurrence") — **final codex pass is asked to concur or the dispute goes to the owner
   before RC dispatch**.
4. **Nomenclature checklist re-run against the RC tree** (M10/codex r2 #9):
   `nomenclature-signoff.md` artifact, every brief checklist item + evidence + result,
   attached to the codex release-readiness round.
5. `e2e-gate.yml` thin dispatcher for soak loops.

## Release execution (rev 3)

1. Version PR: lockstep `2.0.0-rc.1` (4 manifests + locks; README refresh).
2. Dispatch RC: build → publish (16 assets, immutable) → 3-OS gates. Failure ⇒ rc.N+1.
   PushNotification at RC published.
3. Soak ≥2h on the final RC (≥4 consecutive green `e2e-gate` cycles) while docs finalize.
   Nomenclature signoff re-run against the RC tree.
4. Codex release-readiness review (fresh session; receives nomenclature-signoff + gate
   evidence + residual ledger) → clean round required.
5. Version PR → `2.0.0` lockstep ("same tree modulo the four version manifests, verified by
   diff" — L3). Dispatch stable **hold** (`promote:false`): rebuild → pipeline's built-in
   build/unit/wire smokes → publish (17 assets) → verify-published-assets. Fleet untouched.
6. **The single full 3-OS installed journey runs ONCE, against the held published stable
   bytes** (codex r2 #12). Failure ⇒ burned version ⇒ fix-forward 2.0.1 (M7).
7. Pre-GA staging rehearsal (promote composite → rehearsal key → verify → remove).
8. Promote: `promote-only version:2.0.0 bump_source:true` → verify-live-feed → mark-Latest →
   bump-source PR. PushNotification.
9. Post-GA live drill: promote-only 1.0.7 → verify → promote-only 2.0.0 → verify (minutes).
10. SDK: publish-testnet chain → `-revision.N` → fresh-dir install+typecheck → promote-latest.
    PushNotification.
11. Post-release: fresh-install smokes (3 OS, public links); live-feed 1.0.7→2.0.0 update
    smokes; close-out per brief §8 + backlog.

Rollback: pre-promote invisible (hold). Post-promote: promote-only 1.0.7 stops uptake (updated
clients fix-forward-only, Layer-B floor; landing re-badged consistently); crash-on-start ⇒
communicated manual reinstall; never delete; burned versions stay burned.

## Risk register

1. 3-OS gate flake → proven smoke bones; bounded waits; infra-attributed retries logged; full
   suite per RC; 3-strikes → codex.
2. Windows Job wiring silently dropped → IsProcessInJob wiring test through prove path in
   windows-build (M4), not just mechanism test.
3. macOS keychain spike fails → predefined residual package + dispute already packaged for
   codex concurrence; no schedule slip.
4. NSIS exact-token match vs real-world paths (spaces, 8.3, deleted $INSTDIR) → literal-mode
   FindStr, canonicalize-before-delete, harness fixtures for each, template-ordering resolved
   in-cohort.
5. Old-SDK fleet compat on wire changes → everything new stays 403/503-shaped
   (authorization_cooldown is 403); contract test pins text/plain; no new statuses reach old
   SDKs outside their handled set.
6. Peers criteria ambiguity → exact npm pinned; criterion (d) predeclared; any ambiguity ⇒
   keep-deps.
7. pgid reuse / job assignment races → lock discipline stated; fail-prove-if-unassigned;
   tests through real spawn path.

## Post-v2 backlog

(unchanged from rev 2) + macOS crash-orphan containment research; suspended-spawn Job
assignment if codex ever demands it; patch-ahead peer policy revisit at the first real Aztec
bump.

## Scope cuts

(rev 2 list) + previous-latest.json capture (superseded by release-asset sourcing);
suspended-spawn Windows containment; per-source promote param (single-version design).
