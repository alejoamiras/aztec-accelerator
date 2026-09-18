# v2 Release Brief — binding spec for the "ship v2.0" goal

This file is part of the active `/goal`. Read it in full at the start of every session working
that goal. At Phase 0.75 (worktree homing), copy this file + `./inputs/` into the release-train
worktree as `implementations-plan/v2-release-train/{brief.md,inputs/}` and commit; the committed
copy is canonical from then on (the `~/.agents/briefs/aztec-accelerator-v2/` copy is the bootstrap
and gets deleted at close-out).

## Mission

Take freshly-fetched `origin/main` (all merged security arcs included) to production v2.0:
readiness blockers **B2–B7** fixed, test-proven, codex-reviewed to a clean round each, the desktop
app released as **2.0.0** on macOS+Linux+Windows, and the SDK published as the next
**`-revision.N`** — end to end, autonomously.

**B1 (Windows Authenticode signing) is explicitly DEFERRED by the owner. Do not touch it.**

## Sources

- Consolidated readiness verdict: claude.ai artifact `2c1ce7ba-a0be-4820-a7bd-316251f43904`
  ("strong release candidate, not yet production v2.0").
- Raw inputs: `./inputs/` — sub-agent reports 01/03/04/05/08 + `codex-consolidated.md` (the
  codex/terra pass; its 7 numbered items map to B-numbers as: 1→B3, 2→B2, 3→B1, 4→B4, 5→B5,
  6→B6, 7→B7).
- Current state: app `accelerator-v1.0.8-rc.1`; latest published stable `1.0.7` (its tag contains
  `1.0.7-rc.1` — known provenance quirk). SDK versioned `X.Y.Z-revision.N` tracking `@aztec/stdlib`.
- The canonical clone's checked-out `main` is ~20 commits stale — always branch from freshly
  fetched `origin/main`.

## Owner-ratified decisions (do not re-litigate; changing any requires pinging the owner)

- **D-R1** Release + npm publish are authorized without approval waits, gated on the full gate
  suite + a clean codex release review; PushNotification at each irreversible step.
- **D-R2** SDK keeps the aztec-tracking `X.Y.Z-revision.N` scheme; the independent-SDK-semver
  redesign is rejected for this arc.
- **D-R3** Cohort order: B2 → (B3 | B5 | B7 parallelizable) → B6 → B4 last (B4's harness is the
  gate the real RC passes through). Root blueprint may regroup with argument.
- **D-R4** RC soak is ≥2h running the E2E loop (gate suite substitutes for a multi-day soak).
- **D-R5** B1 deferred; owner may start cert/Trusted Signing paperwork separately.

## Cohort specs

### B2 — consent hardening (F-10 + cheap F-07 parts) · est 1–2d
`frontend-src/bridge.js:15-42,104-109`, `frontend-src/authorize.js:63-108`.
- 700ms click-steal guard **default-on for every consent popup**: authorize, onboarding, renewal,
  update. Today it is opt-in and only authorization uses it.
- Arm the guard from **"became actionable"**, not the initial-focus 1s poll; re-arm on re-focus.
- Bind update consent to the displayed version/token.
- F-07 fold-ins only if genuinely small: deny cooldown, consent-queue fairness
  (`authorization.rs:206-218`).

### B3 — contain the native bb worker · est 3–5d
`core/src/bb.rs` (stderr buffered unbounded to the 5-min PROVE_TIMEOUT at :326-338; unbounded
proof read :347-352), `src-tauri/src/main.rs:414-429`, `src-tauri/src/updater.rs:558-568`.
- Size-capped stderr drain **while streaming** (reuse the `CappedReader` pattern from
  `downloader.rs:339-360`), e.g. 64KB.
- Reject empty/malformed proof output before returning 200 (empty/non-32B-aligned).
- Ensure the bb child dies on quit/restart/update: process group (Unix) / Job Object (Windows) or
  the smallest mechanism codex agrees closes the leak — **no supervisor framework**.
- Tests: stderr spam, timeout, restart-mid-proof.
- Fold in (observability report #1/#2): a panic hook that writes panic payload+location to the
  tracing file before abort; enable tray "Show Logs" in production builds (`tray.rs:91-113`).

### B5 — real uninstall · est 3–5d
`src-tauri/nsis/hooks.nsi:101-160` (removes CA files only), `autostart.rs:1968-1978`,
`crash_recovery.rs:461-491`.
- Idempotent `--prepare-uninstall` entrypoint: autostart state, crash-recovery scheduled task,
  Run/StartupApproved keys, browser/OS trust, LaunchAgent/.desktop/systemd units.
- Invoke it from the NSIS uninstall hook; documented/scripted cleanup flows for DMG/AppImage/deb.
- Residual CA is keyless + loopback-constrained — lifecycle hygiene, not an escalated security
  claim.

### B7 — SDK release contract (inside the existing scheme, per D-R2) · est 2–4d
`packages/sdk/package.json`, `_publish-sdk.yml:79-123`, `promote-latest.yml`,
`accelerator-prover.ts:399-535`, `types.ts:60-96`.
- CI job: `npm pack` the tarball, install+typecheck+build it in fresh Node and Vite consumers.
- Decide `@aztec/*` peerDependencies vs exact-pin **with codex** (README claims peers; weigh
  consumer breakage vs instanceof/wire-format dupes).
- Typed + documented error contract: which errors fall back to WASM vs surface; no bare ky
  `HTTPError` escaping untyped.
- Expose app version + `api_version` in `AcceleratorStatus`.
- `MIGRATION.md` attached to a named release; document the `-revision.N` dist-tag policy in
  promote-latest (prereleases never become `latest` silently).

### B6 — split publish from promotion + rollback · est 2–4d
`release-accelerator.yml:876-998` (stable job overwrites production `latest.json` immediately),
`docs/RELEASE_RUNBOOK.md:87-110` (currently advises delete-and-restore).
- Publish + verify artifacts first; **explicit promote job** flips `latest.json` after gates.
- Least-privilege freeze/fix-forward path; rehearse an N+1 recovery once.
- Rewrite the runbook accordingly (no delete-tags-and-restore advice).
- Scope release secrets to the legs that need them where cheap (full provenance work is otherwise
  post-v2).

### B4 — prove the shipped product + 1.x→2.0 migration (LAST) · est 4–7d + soak
`release-accelerator.yml:392-445` (smoke stops at /health), `tests/trust_macos.rs`,
`tests/trust_windows.rs` (omit real trust install), `core/src/config.rs:38-53`.
- **First** fix config migration: honor `config_version`; don't silently drop legacy fields
  (e.g. `safari_support`).
- Packaged-app E2E on all 3 OSes: packed SDK → real browser → installed desktop → native bb proof
  over HTTPS.
- Stateful upgrade test: latest-stable → 2.0.0 preserving origins, config, autostart, HTTPS,
  CA trust, on all 3 OSes.
- Uninstall test (B5's flows) in the same matrix.
- Close macOS/Windows trust-install test gaps where CI runners permit; remaining gaps documented
  as residuals with codex concurrence.

## v1→v2 nomenclature checklist (root plan owns this; all items verified before RC)

- Updater + promote semver comparisons: real semver, no lexical / `"1."`-prefix assumptions.
- `tauri.conf.json` / `Cargo.toml` / `package.json` version fields; NSIS upgrade-over-1.x +
  uninstaller registry keys; macOS `CFBundleVersion` rules; deb/AppImage version fields.
- Identity-guard / marker files / scheduled-task names keyed by version.
- Docs, landing download links, runbook, README version strings.
- SDK `x-aztec-version` negotiation is orthogonal — confirm nothing couples it to app major.
- The 1.0.7→2.0.0 updater jump is exercised on real OS runners (B4 harness), not assumed.

## Scope — OUT (do not touch)

B1/Authenticode. F-09 replay freshness / TUF-like designs. bb revocation/OS sandboxing.
Telemetry/crash reporting beyond the B3 panic hook. Playground/landing security audit (flagged for
GA decision, not this arc). Independent SDK semver. npm Trusted Publisher migration (needs npmjs
UI — keep token flow, note post-v2). **No Aztec protocol bump mid-arc** — Phase 0: if the
aztec-stable automation already has a newer stable ready and green, land it via the existing
workflow BEFORE freeze; otherwise freeze on current. All other should-fix / nice-to-have items →
post-v2 backlog entry in `implementations-plan/index.md`.

## Process

1. Pre-flight: `codex login status` (logged out ⇒ surface immediately, continue only non-codex
   work); `EnterWorktree` from fresh `origin/main` (if the session's git guard still blocks git
   afterward, say so and stop); `agent-worktree register`; keep SSH commit signing ON (homelab
   signing is non-interactive — never skip it here).
2. Root `/blueprint deep` "v2-release-train": cohort boundaries (default per D-R3), release
   mechanics, the nomenclature checklist, rollback story. Full deep protocol (codex + fable legs,
   decision ledger).
3. Per cohort: own worktree/plan/branch (1:1:1), tier by rubric (default mid; light only with
   codex concurrence), smallest decisive test set, every security-relevant fix **mutation-proved**
   (revert → named test fails → restore). Untestable ⇒ ship only if innocuous (owner constraint).
4. Codex loop per cohort until a round comes back clean; codex arbitrates over-engineering trims;
   log every consult in `lessons/`.
5. Ship per cohort as `gh stack` PRs. **Merge protocol (hard lesson)**: one PR at a time, each
   rebased onto fresh main, exactly ONE green run per required context — never `rerun --failed`
   then atomic-merge (the ruleset reads lingering failed runs). Main fully green before the next
   cohort starts.
6. Release execution: bump to 2.0.0 → `2.0.0-rc.1` through the NEW publish→verify (NOT promote)
   flow → gate suite on the RC: 3-OS packaged E2E, stateful upgrade, uninstall, nomenclature
   sweep, all green → ≥2h RC soak running the E2E loop (do docs meanwhile) → codex
   release-readiness review returns a clean round.
7. Promote `latest.json` → 2.0.0 via the new promote job. Then SDK: `npm publish` next
   `-revision.N` (tarball consumer test green first), verify a clean install from npm.
   Post-release: fresh-install smoke + live-feed update-from-1.0.7 smoke on all 3 OSes; release
   notes + MIGRATION.md attached.
8. Close-out: `implementations-plan/index.md`, runbook, readiness artifact (flip blocker
   statuses), memory (mark v2 shipped), `agent-worktree done` for merged cohorts, delete the
   bootstrap copy at `~/.agents/briefs/aztec-accelerator-v2/`. PushNotification at: RC published /
   stable promoted / SDK published / any blocker.

## Authorization (standing, this goal only)

**Granted**: pushing + merging this arc's stacks; dispatching release workflows; creating
`2.0.0-rc.N` + `2.0.0` tags/releases; promoting `latest.json` after ALL gates pass; `npm publish`
of the SDK `-revision.N` via the existing pipeline.

**Never (stop + surface instead)**: history rewrite; deleting published tags/releases (recovery =
fix-forward via the new B6 path); anything B1/signing; creating/rotating secrets; scope beyond
this brief. After 3 failed attempts at the same step: stop, log the lesson, consult codex before
attempt 4.

## Operational notes

- Run-isolation rules apply: ports from `~/.agents/ports.md`, own-pgid teardown only, real-disk
  datadirs, never pkill by name.
- CI: keep the `npm_config_min_release_age=7` export for the aztec installer and the NSIS
  resolver+retry in `accelerator.yml` — don't regress either.
- **D-ITEM7** governs any touched bow-out code: the Windows bow-out exits unless the socket owner
  is positively `Foreign` (`Ours` and `Unknown` both exit); the owner check may only ever ADD a
  reason to stay resident.
- Playwright locally: reuse the cached browser revision (memory: playwright-local-run-workaround).
- Estimated scale: ~15–27 engineering days; expect many sessions/compactions — this brief is the
  cross-session anchor.

## Done =

B2–B7 merged (mutation-proofs + a clean codex round each) · main fully green · 2.0.0 live in the
stable feed with the 1.0.7→2.0.0 update verified on macOS+Linux+Windows · SDK `-revision.N` live
on npm and passing the tarball consumer test · runbook/MIGRATION/docs updated · post-v2 backlog
written · readiness artifact statuses flipped.
