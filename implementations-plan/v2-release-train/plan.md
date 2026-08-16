# plan.md — v2-release-train (rev 4, post final-pass)

Status: rev 4 — final fresh-context codex pass folded (7 findings, 2 BLOCKERs, + 2 trims;
codex REFUSED the HTTPS residual — accepted, B4 restructured). No open disputes remain in the
plan; two items are EXECUTION-TIME verifications with owner-escalation paths (B4 trust spikes;
B5 PREUNINSTALL existence). Legs: `plan-main.md`, `plan-fable.md`, `plan-codex.md`. Binding:
`brief.md` (D-R1..5), `recon.md` (F1–F17), `decision-ledger.md` (through D-C31 + E-1..E-12).

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
   Deny (click/timeout/close); pruned on `request`; **capped map, EVICT-OLDEST at cap**
   (final-pass #7: drop-new lets an attacker fill the map to disable cooldown for everyone;
   evict-oldest is self-defeating for the flooder — cooldown is anti-nag, not a boundary);
   checked in `server/auth.rs` after `is_approved`. Fairness DEFERRED.

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
   - **Linux PDEATHSIG CUT** (final-pass trim: thread-scoped so unreliable even for its one
     purpose; redundant with RAII Drop-kill + explicit terminate for the mandated exit paths;
     two auditors converged on drop). **Crash-orphan (Linux + macOS) = explicit ledgered
     residual** (bb exits naturally at proof end; F-08a reaps the workspace on next start;
     PROVE_TIMEOUT bounds the app-alive case; the Windows Job Object is the only OS-level
     crash containment and that's acceptable).
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

1. **F1 REVISED (final-pass #3): brief-literal — invoke `--prepare-uninstall` from NSIS.** The
   brief says "invoke it from the NSIS uninstall hook"; a newer binding brief overrides the
   older F-05 "no new uninstall hook" precedent, AND F-05's sole objection (a new hook is an
   untestable one-shot bet) is DISSOLVED by B4's new installed-product gate + the NSIS harness,
   which now execute the uninstall path for real. Design: **PREUNINSTALL** runs
   `ExecWait '"$INSTDIR\AztecAccelerator.exe" --prepare-uninstall'` (exe still present — recon
   confirms PREUNINSTALL fires before RMDir) as the primary; **POSTUNINSTALL native-inline**
   (schtasks/reg deletes) stays as the belt for when the exe invocation was skipped/failed
   (exe already gone). **Cohort task 0 (blocking): confirm PREUNINSTALL exists in the actual
   tauri-bundler 2.8.1 template** (recon's open question). If it does NOT exist → this is an
   owner-ratified-deviation point: STOP, surface, propose POSTUNINSTALL-native-only as the
   fallback. `--prepare-uninstall` early-argv flag + the manual/scripted wrappers stay for all
   OSes regardless.
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

1. F13 evidence-gated exact-PEERS, criteria REVISED (final-pass #5 — `--strict-peer-deps` is
   opt-in, so proving ERESOLVE under it proves nothing about real consumers): every fixture is
   installed BOTH ways — **default `npm install` AND `--strict-peer-deps`** — with exact npm
   pinned, then `npm ls` inspected. Fixtures: exact-host (5.0.1), conflicting-host (5.0.0),
   patch-ahead host (5.0.2, M11). PREDECLARED adoption bar for peers: under DEFAULT npm the
   exact-host yields a SINGLETON @aztec graph AND the conflicting/patch-ahead hosts do NOT
   silently produce a working nested duplicate (i.e. default npm's own behavior must match the
   "one @aztec graph" contract). If default npm tolerates the dup (the likely outcome given
   npm ≥7 auto-installs peers), **peers buy nothing over exact-pinned deps → KEEP DEPS**. The
   strict runs are recorded as informational, not decisive. Result + verdict ledgered before
   any manifest change.
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

Two verified facts shape this: every stable release ships its own signed latest.json asset
(previous-feed capture DROPPED), and a GitHub **draft** release has no git tag and its assets
are non-public until published — the key to BLOCKER-2 recovery staying inside authorization.

1. **RC publish**: create a PUBLIC prerelease `--latest=false`, 16 assets, immutable; failed
   ⇒ rc.N+1 (a new version — no burned-version problem).
2. **Stable publish → DRAFT-gate → finalize (final-pass BLOCKER 2)**: the stable build uploads
   to a **DRAFT release** (byte-identical assets, no tag, non-public). The 3-OS installed
   gate + verify-published-assets run against the DRAFT's assets (the exact bytes that will
   ship). On PASS → **publish the draft** (atomic: creates the `accelerator-v2.0.0` tag +
   makes assets public, `--latest=false`), 17 assets (16 + latest.json). On FAIL → **delete
   the DRAFT** (no tag, no published release ever existed → violates neither immutability nor
   the no-delete rule) → fix → re-dispatch the SAME `2.0.0`. This keeps failed bytes fully
   private (fixing codex's "`--latest=false` is still downloadable" point) and never needs
   `2.0.1` — so it stays inside the goal's `2.0.0`-only authorization. A 2.0.1 fix-forward
   would exceed authorization and is a STOP-and-surface event, not an autonomous step.
3. `promote-only` mode — single `version` input: pre-flight = published release exists ∧
   latest.json asset present ∧ production-verifier signature ∧ **feed_version == input version**
   ∧ full asset completeness (count + every platform URL resolves). Flip S3 + invalidate,
   then **immediately PushNotification** (final-pass #6: bound the cross-system window). Then
   verify-live-feed (asserts feed_version live) + mark-GitHub-Latest (idempotent, best-effort)
   + `bump_source: bool` (true only for organic GA). Rollback = `promote-only version:1.0.7`.
4. **Landing derives the download version from the SIGNED S3 FEED** (`aztec-accelerator.dev/
   releases/latest.json`), NOT the GitHub Latest badge (final-pass #6): the feed is the single
   source of truth that `promote` flips, so a mark-Latest partial failure can never make
   landing and the updater disagree. GitHub Latest becomes cosmetic.
5. **Burned-stable rule (M7, revised)**: since stable gates as a DRAFT, a failed gate deletes
   the draft and re-dispatches the same 2.0.0 — nothing is burned in the common case. A burn
   only occurs if a stable release was already PUBLISHED (tag exists) and later found bad →
   that is fix-forward-to-2.0.1 territory = STOP-and-surface (owner authorization required).
6. Append-only policy (codex r2 #7 + L1): workflow-contract rows ban `gh release delete` on
   PUBLISHED releases (draft deletion allowed), ban `--clobber`, pin every create
   `--latest=false`; additive-repair runbook forbids `--clobber` and ends with the full asset
   verifier.
7. Rehearsal (final-pass trim #1 — **live prod drill CUT**): the promote composite is
   parameterized on S3 key; the **pre-GA staging rehearsal** exercises BOTH sourcing paths
   (promote candidate 2.0.0 AND previous 1.0.7 to `releases/rehearsal/latest.json`, verify
   each, remove) — proving the rollback machinery WITHOUT ever serving 1.0.7 to real users.
   The organic GA promotion proves the live candidate path. No deliberate prod downgrade.
8. Declaration-equality gate (conf + 3 Cargo == input). Runbook rewrite + CLAUDE.md; manual
   trust-install pre-release checklist added.

Tests: workflow-contract rows (publish has no AWS creds; stable gates as draft; promote
prerelease-false-gated; 16/17 asset gates; no delete on published; no --clobber;
--latest=false pinned; bump_source wiring; verify-live-feed keyed to promoted version) — each
mutation-provable by YAML edit; landing-reads-signed-feed unit; promote composite fixture
tests (wrong version / unsigned / missing asset / feed-version mismatch → refuse); actionlint;
auth_probe graph dry-run.

### B4 — shipped-product + migration gate (`b4-product-gate`) — LAST

1. Config migration + **probe-parse version gate (H3), TYPE-BOUND (final-pass #4 — "ALL saves
   rejected" repeats the E-7/E-8 enforcement-by-convention mistake)**: stage 1 = lenient
   `{config_version: u32}` probe (unknown fields ignored); probe > CONFIG_VERSION ⇒ **load
   returns NO `PersistCapability`** (a token minted only by a successful current-or-migratable
   load); stage 2 = full parse with `safari_support`→`https_enabled` Value-pass migration (new
   key wins), CONFIG_VERSION=2 written. **Every save path (`save`, command-layer, authorization
   persist, startup-reset, direct helpers) takes `&PersistCapability` as a required arg** — a
   future-schema load structurally cannot produce one, so saves cannot compile a bypass (same
   newtype discipline as B2's PendingUpdate). Tests: migration trio + `future_config_never_
   persisted_over` with a fixture that is **NOT v2-parseable** [mut: drop probe → compile still
   blocks save; the decisive mut is removing the capability arg → won't compile] + **each save
   path mutation-tested to require the capability** + v2-untouched.
2. Per-OS combined gate, two stages (fresh-install; upgrade→proof→FULL uninstall incl. package
   removal). **Upgrade stage adds: CA trust still present after 1.0.7→2.0.0 where seedable**
   (M9 — brief-mandated; Linux now, macOS if spike; mac/win residual ledgered). Uninstall
   assertions scoped to app-owned stores.
3. HTTPS proof matrix — **RESIDUAL REFUSED by the final pass; restructured to non-interactive
   MANDATORY trust spikes that satisfy the full composed path, with owner-escalation on
   genuine failure** (codex ruling: UntrustedSkip is affirmative proof the app did NOT serve
   HTTPS; component evidence ≠ the composed security path). The bar codex will accept, per OS:
   app CA present in ITS OWN production trust store (not distrusted) → app logs `Ready` → port
   59834 serves TLS → a real browser with the packed SDK completes a native-bb proof with HTTP
   downgrade AND WASM fallback DISABLED. Non-interactive routes to the app's own store:
   - **Linux**: certutil into the user NSS store (already proven) → full composed proof.
   - **macOS**: fresh login keychain + `security unlock-keychain` +
     `security set-key-partition-list -S apple-tool:,apple: -k <pw>` +
     `add-trusted-cert` into that login keychain (the store `trust/macos.rs:16-19` reads) → no
     GUI prompt → full composed proof.
   - **Windows**: raw registry write of the cert blob to
     `HKCU\Software\Microsoft\SystemCertificates\Root\Certificates\<thumbprint>` — this
     populates the SAME logical CurrentUser\Root the app queries (`trust/windows.rs:23`)
     WITHOUT the CryptoAPI root-install prompt that froze CI (the freeze was on
     `CertAddCertificateToStore`, not a registry write) → full composed proof.
   These are TEST-HARNESS trust seeding (the app's own interactive install flow is a separate,
   documented manual pre-GA checklist item — not on the autonomous path). **If a spike
   genuinely fails after real effort (3-strikes rule), that OS's composed HTTPS proof is a
   STOP-and-surface to the owner BEFORE RC dispatch** — not a silent residual and not a manual
   ceremony baked into the autonomous run (resolves final-pass BLOCKER 1). The refused rev-3
   compensating package (tls_handshake.rs + UntrustedSkip assertion) is retained only as
   SUPPLEMENTARY evidence, never as a substitute for the composed proof.
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
   diff" — L3). Dispatch stable: rebuild → build/unit/wire smokes → upload to a **DRAFT
   release** (17 assets staged, no tag, non-public).
6. **The single full 3-OS installed journey runs ONCE against the DRAFT's byte-identical
   assets** (codex r2 #12). PASS ⇒ **publish the draft** (creates tag, `--latest=false`).
   FAIL ⇒ delete the draft → fix → re-dispatch `2.0.0` (nothing public was created; stays in
   authorization). A publish-then-fail would be a burn = STOP-and-surface (M7).
7. Pre-GA staging rehearsal: promote composite → rehearsal key for BOTH 2.0.0 and 1.0.7 sources
   → verify each → remove (proves rollback machinery without serving a downgrade to real users).
8. Promote: `promote-only version:2.0.0 bump_source:true` → S3 flip → PushNotification →
   verify-live-feed → mark-Latest → bump-source PR.
9. SDK: publish-testnet chain → `-revision.N` → fresh-dir install+typecheck of the exact
   registry version → promote-latest. PushNotification.
10. Post-release: fresh-install smokes (3 OS, public links); live-feed 1.0.7→2.0.0 update
    smokes; close-out per brief §8 + backlog.

Rollback: pre-publish failure is fully private (draft). Post-promote: `promote-only 1.0.7`
stops uptake (updated clients fix-forward-only, Layer-B floor; landing reads the signed feed so
it follows the flip); a bad PUBLISHED 2.0.0 needs 2.0.1 = STOP-and-surface (exceeds
authorization); crash-on-start ⇒ communicated manual reinstall; never delete published
releases/tags.

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
