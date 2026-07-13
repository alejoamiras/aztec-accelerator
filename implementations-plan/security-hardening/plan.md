# Security-Hardening Campaign — Master Plan

**Integration branch:** `security-hardening` (cut from `main` @ `9e0d742`).
**Source audit:** `audit/security/2026-07-09-5c788c0/` (report.md / findings/verified.md).
**Co-architect:** Codex `gpt-5.6-sol` @ `model_reasoning_effort=xhigh` (invoked on every non-trivial decision).
**Executor:** Claude (GUI-less Linux VPS — macOS/Windows/Tauri-GUI paths validated via CI only).

## Scope

**Fix:** F-002, F-003, F-004, F-005, F-006, F-007, F-008, F-009, F-010, F-011, F-012, F-014, F-015, F-016.
**Exclude:** F-001 (SDK→server auth — owned by another team), F-013 (headless localhost auto-approve — accepted; headless is CI-only).

## Rules (invariants for every cluster)

1. **Blueprint before code** at the tier in the ledger; home each cluster in its own worktree+branch cut from the *latest* `security-hardening`.
2. **Codex consult** (`codex exec -m gpt-5.6-sol -c model_reasoning_effort=xhigh`) on every non-trivial design decision; discuss trade-offs before committing.
3. **No merge into `security-hardening` without BOTH** local tests AND CI green on the cluster PR. CI never faked.
4. **GUI-less VPS**: cannot locally run macOS (cert trust/codesign), Windows (schtasks/crash-recovery), Tauri GUI/WebDriver → rely on CI, documented per cluster.
5. **Human-gated (never done autonomously):** `tofu apply`, GitHub ruleset apply via API/UI, secret creation/rotation/cutover, force-push/history-rewrite on shared branches, merge to `main`. These are flagged in the PR + a runbook.
6. **Sequential, not stacked**: cut each branch from `security-hardening` AFTER the prior cluster merges. No parallel stacked branches where workflows / updater release code / frontend files overlap.
7. Update `index.md` (ledger) after every cluster.

## Finalized cluster order (Codex verdict)

| # | Branch | Findings / tier | Local gate (VPS) | CI gate | Primary risk |
|---|---|---|---|---|---|
| C0 | `sechard/ci-integration-gates` | bootstrap / **light** | actionlint | manually dispatch all 4 gate workflows on the C0 ref | bootstrap PR may not auto-trigger |
| C1 | `sechard/workflow-input-hardening` | F-006 / **light** | actionlint + tag-validator tests | `actionlint.yml` | validation must run before any token-bearing step |
| C2 | `sechard/core-request-safety` | F-003, F-009, F-011 / **mid** | core fmt/clippy/tests; concurrency/backpressure tests | `accelerator.yml` | slow bodies, perms-at-creation, persisted dotted origins |
| C3 | `sechard/action-pinning` | F-015 / **mid** | actionlint + "all remote uses are SHAs" checker | all 4 gates + `actionlint.yml` | huge mechanical diff; unpinned downloaded tools |
| C4 | `sechard/updater-rollback` | F-004 / **deep** | Rust/feed unit + fixture-key rollback tests | `accelerator.yml`, `actionlint.yml`, new PR-safe Linux rollback smoke | manifest canonicalization, floor corruption, signing-key scope |
| C5 | `sechard/infra-deploy-authz` | F-005 / **deep** | `tofu fmt/init -backend=false/validate`; IAM/ruleset policy tests | `actionlint.yml` extended to validate ruleset JSON + IAM invariants | staged role migration; landing `--delete`; ruleset not live from JSON |
| C6 | `sechard/bb-cache-integrity` | F-007 / **mid** | Bun fixture + Rust cache + safe-extraction tests | `accelerator.yml` | digest marker must form a real verification chain |
| C7 | `sechard/bb-windows-provenance` | F-008 / **mid** | manifest/provenance validator tests | Windows prebuild/build in `accelerator.yml` | no independent provenance → Windows release must fail closed |
| C8 | `sechard/desktop-platform-secrets` | F-010, F-016 / **mid** | systemd serialization + cert-gen tests | `accelerator.yml` + targeted macOS cert test | systemd byte escaping; rcgen zeroization incomplete |
| C9 | `sechard/authorize-popup-safety` | F-014 / **light** | helper tests + headless mocked Playwright | accelerator desktop UI + WebDriver | IDN/punycode display; hiding security-relevant subdomains |
| C10 | `sechard/tauri-trust-boundary` | F-012 / **deep** | frontend build, CSP lint, Rust command-policy tests, mocked UI | WebDriver on Linux/macOS/Windows + negative IPC tests | highest regression risk: IPC, popup, updater prompt |
| C11 | `sechard/incumbent-identity` | F-002 / **deep** | fake-identity, forged/replay challenge tests | Windows dual-instance/port-squatter integration | **must consume F-001 identity contract; no public-health fallback** |

> Note: `app.yml` covers playground/SDK, NOT the desktop app. F-012/F-014 desktop CI is under `accelerator.yml` + WebDriver, not `app.yml`.

## Non-negotiable implementation details (Codex)

- **F-003:** create the Unix dir `0700` + witness file `0600` **at creation** (Builder), never write-then-chmod. `cfg(unix)`; Windows uses ACLs. macOS validates the Unix path in CI.
- **F-009:** authorize → acquire proof permit → **then** read body under the 50MB limit **and a body-read timeout** (defeats slowloris monopolizing the single permit). Test oversized body, cancellation, permit release, no early second-body poll.
- **F-011:** reject trailing-dot origins in the `CanonicalOrigin` constructor (don't migrate to undotted). Drop invalid persisted entries with a warning. Keep Host-header normalization separate.
- **F-004:** sign a **canonical manifest envelope embedded in `latest.json`** (version, publish time, per-platform url+sig+size); verify the envelope with the updater pubkey and require `Update.raw_json` to match exactly. Sign in a dedicated job with **no** AWS/GitHub write perms. Persist highest-running installed version as an atomic `0600` **monotonic floor**; require candidate `> max(current, floor)`; bump floor only after the new build starts OK; corrupt floor ⇒ updater fail-closed (not reset). Signed size fixes the feed-only lie vs the preflight cap (residual: host can serve huge bytes before sig rejection — document). Chrome-142 LNA note (9e0d742) is unrelated to rollback.
- **F-005:** current landing `aws s3 sync … landing/ --delete` **can delete `landing/releases/latest.json`** — add `--exclude 'releases/*'` + IAM explicit deny (`deploy-landing.yml` landmine). **Four** roles: landing (`landing/*`, deny `landing/releases/*`), release-feed (`landing/releases/latest.json` only), testnet-playground (`playground/*`), nightly-playground (`nightlies` → `playground-nightly/*`). Prefix-conditioned `ListBucket` + object perms. CloudFront invalidation can't be prefix-scoped in one distribution (document residual). **Remove `chore/aztec-*` from OIDC trust entirely**; use exact subjects. Protect `main` + `nightlies`, require 1 review + all 4 status aggregators. Two-stage rollout: additive roles → new secrets → workflow cutover → smoke → remove legacy broad role. Editing `main-branch-protection.json` is desired-state only; a human applies rulesets via API/UI + reads back live config.
- **F-006:** validate `dist_tag` (`^[a-z0-9._-]+$`) and pass via `env:` quoted, **before** any token-bearing step.
- **F-007:** both download paths verify the GitHub release-API digest, extract into a **private staging dir**, reject unsafe archive members, atomically publish binary + a **marker** (verified archive digest + extracted-binary digest). Runtime **rehashes cached `bb` against the marker on every use**; missing/malformed legacy marker ⇒ fail closed + redownload.
- **F-008:** remove auto-pinning from `update-aztec-version.ts`. Accept a Windows pin only from signed upstream checksum/provenance, a verifiable attestation, or a reproducible build + human review (a twice-downloaded asset is not independent evidence). Revalidate the current live pin; block the version if no independent evidence. **Also fix `_aztec-update.yml`: `auto_merge: false` currently performs an immediate merge — it must leave the PR open.**
- **F-010:** serialize `ExecStart` with systemd unit-escaping over Unix path bytes; reject controls/newlines, escape quotes/backslashes, double `%`. Validate generated units with `systemd-analyze verify` (don't install).
- **F-012:** externalize inline scripts/styles; `withGlobalTauri:false`; bundle imports from `@tauri-apps/api`; strict CSP with only documented Tauri IPC connect sources. Declare custom commands in `build.rs`, split capabilities by window label, keep Rust-side caller-label checks as defense-in-depth.
- **F-014:** preserve scheme + security-relevant subdomain context + registrable domain (bundled PSL), keep punycode canonical, expose full origin via accessible text/title, make content scrollable, keep actions reachable, default Remember **unchecked**.
- **F-015:** SHA-pin every remote `uses:` (incl. GitHub-owned) with version comments; pin the actionlint download by commit + checksum; stop mutable `bun-version: latest` / Rust `stable` in touched setup paths.
- **F-016:** `Zeroizing<KeyPair>` around the CA key + explicit early drop after leaf signing. rcgen 0.13 provably scrubs serialized DER but not every backend allocation — document residual. Leaf key stays persistent by design.

## CI-gate bootstrap (C0)

Add `security-hardening` to each gate's `pull_request.branches: [main, security-hardening]`. `pull_request.branches` matches the PR **base**; jobs test the merge ref. Keep the internal `changes` jobs (aggregators pass legitimately-skipped jobs); **do not** add top-level `paths` filters to required workflows. `actionlint.yml` lints its own edit. No `pull_request_target`. Procedure: open C0 → local actionlint → manually dispatch `accelerator.yml`/`sdk.yml`/`app.yml`/`actionlint.yml` on the C0 ref → merge when all 4 green → (human) apply a temporary `security-hardening` ruleset requiring the 4 aggregators. Remove the temporary trigger/ruleset during the final integration PR into `main`.

## Human-gated closeout (cannot be done by Claude)

- **F-005:** trusted human runs `tofu plan` → review → `apply`; applies the GitHub ruleset via API/UI; cuts over deploy secrets; live read-back. "Source/CI complete" ≠ "operationally remediated".
- **F-002:** blocked on F-001's team exposing the `InstallationIdentity` contract (see below). Lands last; if F-001 isn't ready, C0–C10 land but **F-002 cannot be claimed closed**.
- Temporary `security-hardening` CI trigger + ruleset are reverted in the final `main` integration PR.

### F-002 identity contract (must come from F-001's team)

```
InstallationIdentity
  expected_identity() -> trusted local identity/key id
  answer_challenge(nonce, context) -> authenticated response
  verify_challenge(nonce, context, response) -> verified/rejected
```
Protocol: fresh 32-byte nonce; domain-separated context `aztec-accelerator/incumbent/v1`; bind response to nonce+api_version+port; verify against identity from the trusted local provider (never a key supplied only by the response); legacy/missing/malformed/replayed ⇒ "foreign process" (stay resident, surface port-in-use); only a verified incumbent permits Windows `exit(0)`. F-002 must not read F-001's files or assume their crypto directly.

## Seed strings

### /goal
```
GOAL: Land fixes for the 2026-07-09 security audit on `security-hardening` (cut from latest origin/main), one blueprinted cluster-branch at a time, each merged into security-hardening ONLY after local tests AND CI are both green.
SCOPE: fix F-002,F-003,F-004,F-005,F-006,F-007,F-008,F-009,F-010,F-011,F-012,F-014,F-015,F-016. NEVER touch F-001 (owned elsewhere) or F-013 (accepted).
RULES: (1) /blueprint each cluster at the ledger tier BEFORE coding, in its own worktree+branch off latest security-hardening. (2) Consult Codex `-m gpt-5.6-sol -c model_reasoning_effort=xhigh` on every non-trivial decision; discuss trade-offs before committing. (3) NEVER merge into security-hardening without local AND CI green on the cluster PR. (4) GUI-less VPS: rely on CI for macOS/Windows/Tauri-GUI; never fake-pass. (5) Infra/ruleset: commit + validate only; NEVER tofu apply / gh api ruleset apply — human steps. (6) No secret creation/rotation, no force-push/history-rewrite on security-hardening or main. (7) Update implementations-plan/security-hardening/index.md after each cluster. (8) Cut each branch AFTER the prior merges (sequential).
DONE: C0–C10 merged into security-hardening via local+CI-green PRs; C11/F-002 landed or explicitly BLOCKED on F-001; final status + human-gated runbook posted.
```

### /loop
```
/loop Work the security-hardening campaign one cluster at a time. Each iteration: (1) read implementations-plan/security-hardening/index.md; pick the next PENDING cluster whose deps are met — if none, STOP + report. (2) `agent-worktree new <slug>` off latest security-hardening; /blueprint <tier> its findings. (3) Consult Codex gpt-5.6-sol xhigh on design; implement per the plan's non-negotiable details, tests inline. (4) Validate locally (cluster's applicable commands). (5) Push branch; open a draft PR INTO security-hardening; `gh pr checks --watch`; fix until green (dispatch workflows if a base-branch trigger is missing). (6) Only when local AND CI green: mark ready, merge the PR into security-hardening, set the ledger row DONE with PR link + lessons note. (7) Consult Codex on any ambiguity. Never merge on red. Continue.
```
