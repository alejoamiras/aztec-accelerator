# Release & Maintenance — 2026-05-27 (revised after dual audit)

```
[✓] 0. Clarifying questions
[✓] 1. Draft plan (v1)
[✓] 2. Dual audit (codex xhigh + opus subagent)
[▶] 3. Revise plan (v2 — this doc)
[ ] 4. Final codex pass
[ ] 5. Approval gate ← YOU
[ ] 6. Track 1 — automation deprecation + workflow fixes
[ ] 7. Track 2 — branch cleanup
[ ] 8. Track 3 — @aztec 4.2.0 forward-roll + SDK release + accelerator 1.0.1
[ ] 9. Post-impl codex review
[ ] 10. Fix loop
```

## Decisions locked in

| Question | Choice |
|---|---|
| Automation deprecation | Disable (workflow_dispatch-only) |
| "Rollback main to 4.2.0" | Forward-roll `@aztec/*` `4.2.0-rc.1` → `4.2.0` stable on both SDK + playground |
| Accelerator next version | `1.0.1` patch |
| Branch cleanup | Aggressive — but tightened predicate per audit |
| SDK publish (post-audit) | **Fix `get-sdk-publish-version.ts`** so stable bases produce `4.2.0-revision.1` (valid semver) and publish that |
| Workflow fixes for `release-accelerator.yml` | **Fold into PR A** — tag-after-build + prerelease detection |

## Three tracks → PR / shell mapping

- **PR A** — Automation deprecation + 4 small workflow fixes (script + release workflow + paths filter + auto-merge).
- **Shell session (no PR)** — Branch cleanup, tightened to delete MERGED only with SHA verification.
- **PR B** — Forward-roll `@aztec/*` `4.2.0-rc.1` → `4.2.0` on **both** SDK and playground.
- **Manual dispatch** — `publish-testnet.yml` to publish SDK at `4.2.0-revision.1` + `npm dist-tag` set to `latest` & `testnet` (workflow does this automatically with `latest: true`).
- **Manual dispatch** — `release-accelerator.yml` `-rc.2` dry-run (now safe), then `1.0.1` stable.

---

## PR A — Track 1 (deprecation) + audit-driven workflow fixes

### A.1 — Deprecate `@aztec/*` automation

Edits:

- `.github/workflows/aztec-stable.yml`: remove `on.schedule`. **ALSO** set `auto_merge: false` (codex finding — currently `true`, manual dispatch would still auto-merge).
- `.github/workflows/aztec-nightlies.yml`: remove `on.schedule`. ALSO set `auto_merge: false` (the workflow doesn't pass `auto_merge` today so it inherits the default `true` in `_aztec-update.yml`; explicitly disable).
- `.github/workflows/backport-nightlies.yml`: **delete the file**.
- `.github/workflows/publish-testnet.yml`: remove `on.push`. Keep `workflow_dispatch`. **Document explicitly** that this also disables the auto playground deploy on push to main (codex finding).
- `.github/workflows/publish-nightlies.yml`: remove `on.push`. Keep `workflow_dispatch`.

No changes to `_aztec-update.yml` or scripts. They're still invoked by the dispatch path.

### A.2 — Fix `scripts/get-sdk-publish-version.ts` for stable bases

**Current bug**: when `baseVersion` is stable (no `-`) and already published, the script returns `${base}.${N}` (e.g. `4.2.0.1`) — **not valid semver**, npm publish would reject.

**Fix**:

```ts
export function resolvePublishVersion(
  baseVersion: string,
  publishedVersions: string[],
): string {
  if (!publishedVersions.includes(baseVersion)) {
    return baseVersion;
  }
  const isPrerelease = baseVersion.includes("-");
  const prefix = isPrerelease ? `${baseVersion}.` : `${baseVersion}-revision.`;
  const revisions = publishedVersions
    .filter((v) => v.startsWith(prefix))
    .map((v) => Number(v.slice(prefix.length)))
    .filter((n) => Number.isInteger(n) && n > 0);
  const maxRevision = revisions.length > 0 ? Math.max(...revisions) : 0;
  return isPrerelease
    ? `${baseVersion}.${maxRevision + 1}`
    : `${baseVersion}-revision.${maxRevision + 1}`;
}
```

Update `scripts/update-aztec-version.test.ts` (or `get-sdk-publish-version.test.ts`) with cases:
- stable not-yet-published: `4.3.0` + `[]` → `4.3.0`
- stable already published: `4.2.0` + `["4.2.0"]` → `4.2.0-revision.1`
- stable + multiple revisions: `4.2.0` + `["4.2.0","4.2.0-revision.1"]` → `4.2.0-revision.2`
- prerelease unchanged: `4.2.0-nightly.20260413` + `["4.2.0-nightly.20260413"]` → `4.2.0-nightly.20260413.1`

Update the script's leading comment block to document the new behavior.

### A.3 — Fix `release-accelerator.yml`: tag-after-build + prerelease handling

**Current bugs**:
1. `tag` job (lines 49-68) runs BEFORE `build` + `build-headless`. A failed build leaves the tag pushed.
2. `latest.json` generation (line 373) and S3 upload (line 501) always run, regardless of whether the version is a prerelease. `1.0.1-rc.2` would overwrite production `latest.json` and auto-update users to an rc.
3. Bump-source PR (line 511) always opens, even for prerelease.

**Fixes**:
- Restructure job graph: `validate → e2e-webdriver → build + build-headless + smoke → tag → release → bump-source`.
  - The tag should only land after the build proves out.
  - Idempotent re-runs: if `accelerator-v$VERSION` already exists (e.g. re-run after partial failure), skip recreation (current behavior at line 62 — keep).
- **Surface prerelease as a `validate` job output**, not a `release` step output. The current `validate` job already outputs `version` and `tag`; add `is_prerelease` computed as `[[ "$INPUT_VERSION" =~ - ]] && echo true || echo false`. Reason: `bump-source` is a separate job (line 411) and cannot reference step outputs in another job — it can only see job-level outputs. Codex final-pass finding.
- Wrap prerelease-sensitive paths via `if: needs.validate.outputs.is_prerelease == 'false'`:
  - `release` job's "Generate latest.json" step
  - `release` job's "Upload latest.json to S3" + CloudFront invalidation step
  - The entire `bump-source` job (gate on `needs.validate.outputs.is_prerelease == 'false'`)
- `gh release create` in the `release` job: pass `--latest=false --prerelease` when `needs.validate.outputs.is_prerelease == 'true'`, else `--latest`.

The `-rc.2` dry-run then:
- Creates a GitHub release marked `--prerelease` (visible publicly, but not advertised as latest)
- Does NOT touch S3 `latest.json` → updater clients stay on `1.0.0`
- Does NOT open a `bump-source` PR
- DOES exercise the build matrix end-to-end including ARM64

### A.4 — Bump `build-headless` job timeout

`.github/workflows/release-accelerator.yml`: `build-headless` matrix `timeout-minutes: 30` → `45`. ARM64 with `lto = true, codegen-units = 1` (Cargo.toml:54-56) can plausibly exceed 30 min on first cold cache.

### A.5 — Fix `accelerator.yml` paths filter

Today `accelerator.yml:22-31` includes `packages/sdk/src/**` but NOT `packages/sdk/package.json`. PR B (which changes `packages/sdk/package.json`) won't fire the accelerator E2E. **Add `packages/sdk/package.json` to the filter.** This ensures the @aztec forward-roll exercises the accelerator E2E.

### A.6 — `CLAUDE.md` update

Edit the "Current State" CI section:
- Remove `backport-nightlies.yml` from the list
- Add note: "automation: aztec-nightlies.yml, aztec-stable.yml (workflow_dispatch only)"
- Note: "publish: publish-testnet.yml, publish-nightlies.yml (workflow_dispatch only)"

### PR A validation gates

- `bun run lint:actions` (modified workflows + new logic)
- `bun run test` (catches the publish-version script test addition + any TS drift)
- `bun test scripts/` (specifically the publish-version test)
- Manually eyeball the `release-accelerator.yml` diff for the job-graph reorganization

### PR A risks

- **Job reorder regresses tag/release pairing.** Mitigation: incremental + manual review + visual diff of the workflow graph; the `e2e-webdriver` job already gates the current `tag` job, so we're inserting `build + build-headless + smoke` into the same dependency chain.
- **Prerelease detection logic mistakes a stable version for prerelease.** Mitigation: explicit `[[ "$VERSION" =~ - ]]` plus a test that runs the workflow's detection step locally with sample inputs.
- **Bump-source skip for prerelease might leave us with an inconsistent source version.** Acceptable — the source version was bumped after the LAST stable (`1.0.1-rc.1` after `1.0.0`). An rc release doesn't need to advance source.

---

## Track 2 — Branch cleanup (shell session, no PR)

### Reality check (corrected per audit)

- **Remote branches: 11** (not 129 — that figure was `git for-each-ref refs/remotes/origin/` after `git fetch --all`, which included stale tracking refs).
- The 11 remote branches:
  ```
  backport/nightlies-pr-166        ← auto, no PR (PR #172 closed earlier this session)
  backport/nightlies-pr-167        ← auto, no PR (PR #171 closed earlier this session)
  chore/aztec-nightlies-4.3.0-nightly.20260419
  chore/aztec-nightlies-4.3.0-nightly.20260420
  chore/aztec-nightlies-4.3.0-nightly.20260421
  chore/aztec-nightlies-4.4.0-nightly.20260527   ← PR #222 (open)
  chore/aztec-stable-4.3.0-rc.1                  ← PR #212 (open)
  feat/landing-redesign                          ← orphan
  fix/update-prompt-ux-and-download              ← orphan
  main                                           ← preserve
  nightlies                                      ← preserve
  ```

### Steps

1. **Close** PRs #222, #212 with comment "Superseded by the manual-trigger model (PR A)". (#172 and #171 already closed.)

2. **Delete the auto-generated branches.** For each of:
   - `backport/nightlies-pr-166`
   - `backport/nightlies-pr-167`
   - `chore/aztec-nightlies-4.3.0-nightly.20260419`
   - `chore/aztec-nightlies-4.3.0-nightly.20260420`
   - `chore/aztec-nightlies-4.3.0-nightly.20260421`
   - `chore/aztec-nightlies-4.4.0-nightly.20260527` (post-PR-close)
   - `chore/aztec-stable-4.3.0-rc.1` (post-PR-close)

   Verify before delete: compare branch tip SHA against the **PR's head SHA at merge time** (not the merge commit — squash and rebase merges produce a merge commit SHA that doesn't match the branch tip even when deletion is safe). Codex final-pass finding.
   ```bash
   # Get the PR's head SHA when it was merged
   PR_HEAD_SHA=$(gh pr list --state merged --head <branch> --json headRefOid --jq '.[0].headRefOid')
   # Get the current branch tip on origin
   CURRENT_TIP=$(git ls-remote --heads origin <branch> | cut -f1)
   if [ "$PR_HEAD_SHA" = "$CURRENT_TIP" ]; then
     echo "SAFE: tip matches merged PR head → delete"
   else
     echo "SKIP: branch was force-pushed or reused after merge"
   fi
   ```
   This catches the codex-flagged edge case: a branch with a merged PR record but new commits pushed afterward gets correctly skipped.

3. **Orphans (`feat/landing-redesign`, `fix/update-prompt-ux-and-download`)**: manual review. List the latest commit message and date for each, decide individually.

4. **Local pruning**:
   ```bash
   git fetch --prune
   git branch -vv | awk '/: gone]/ {print $1}' | xargs -r -n1 git branch -D
   git branch --merged main | grep -vE '^[* ]+(main|nightlies)$' | xargs -r -n1 git branch -d
   ```
   `git branch -d` (lowercase) refuses to delete unmerged → safe. `-D` (capital) only used for gone-on-remote which means the remote agreed it was disposable.

### Risks

- **Closed PR record but force-pushed commits later** → my predicate's SHA comparison catches this (branch tip ≠ merged PR's merge commit → skip).
- **Tag dangling**: tags don't depend on branches. Safe.
- **Branch protection**: `main` and `nightlies` are protected; explicit `grep -vE '^(main|nightlies)$'` excludes them.

### Adversarial check

- **Restoration after accidental delete**: branch tips that were merged are reachable from main's history. We don't lose work. For non-merged orphans, the branch ref is gone but the commits remain in reflog for 90 days.
- **Concurrent push race**: if I delete a branch while someone (a workflow) pushes to it, the push fails harmlessly. There are no scheduled workflows post-PR-A that would push to these branches.

---

## PR B — Track 3a (forward-roll)

### Scope

`packages/sdk/package.json` AND `packages/playground/package.json` — every `@aztec/*` dep from `4.2.0-rc.1` → `4.2.0`.

### Steps

1. Branch `chore/aztec-4.2.0-forward-roll`.
2. Run `bun scripts/update-aztec-version.ts 4.2.0` (the script updates both SDK and playground per its own logic at `scripts/update-aztec-version.ts:11`).
3. Refresh `bun.lock` via `bun install`.
4. Validate:
   - `bun run lint`
   - `bun run test` (lint + typecheck + 73 unit tests + the new `get-sdk-publish-version.test.ts` cases from PR A)
   - `bun run lint:actions`
   - Local SDK build: `bun run --cwd packages/sdk build`
   - Local playground build: `bun run --cwd packages/playground build`
5. Eyeball `bun.lock` diff for unrelated drift.

### PR-gate workflows that must pass

After PR A merges, the `accelerator.yml` paths filter will include `packages/sdk/package.json` — so PR B's @aztec bump will fire the accelerator E2E suite (WebDriver + Mocked + Local Network). All four expected: `sdk.yml`, `app.yml`, `accelerator.yml`, `actionlint.yml`.

### Risks

- **Lockfile transitive drift**: bun may re-resolve other deps. Mitigation: review lockfile diff.
- **@aztec runtime regressions between rc.1 and stable**: we trust upstream's semver. The PR-gate's full E2E catches integration issues.
- **CI auto-publish trigger surprise**: post-PR-A, `publish-testnet.yml` no longer fires on push, so merging PR B does NOT auto-publish the SDK. Step 3b is now explicit and intentional.

---

## Track 3b — SDK publish

### Trigger

```bash
gh workflow run publish-testnet.yml --ref main
```

### What runs

`publish-testnet.yml` → calls `_publish-sdk.yml` with `dist_tag: testnet, latest: true`.

`_publish-sdk.yml` then:
1. Reads `@aztec/stdlib` version from `packages/sdk/package.json` (now `4.2.0` post-PR-B).
2. Runs `bun scripts/get-sdk-publish-version.ts 4.2.0`.
3. Per the PR-A fix, returns `4.2.0-revision.1` (because `4.2.0` is already on npm but `4.2.0-revision.*` is not).
4. Patches `packages/sdk/package.json` to `version: "4.2.0-revision.1"`.
5. `npm publish --provenance --tag testnet --workspaces=false`.
6. `npm dist-tag add @alejoamiras/aztec-accelerator@4.2.0-revision.1 latest` (because `inputs.latest == true`).
7. Creates GitHub release `@alejoamiras/aztec-accelerator@4.2.0-revision.1` marked `--latest`.

### Pre-flight (must verify before triggering)

1. `bun scripts/get-sdk-publish-version.ts 4.2.0` locally returns `4.2.0-revision.1` (the PR-A script fix is in effect).
2. `npm view @alejoamiras/aztec-accelerator dist-tags --json` shows current `latest = 4.2.0-rc.1` (so we're moving it forward by code recency, even if semver-wise `4.2.0-revision.1` < `4.2.0-rc.1`).
3. npm OIDC trusted publisher unchanged (test by checking that the last successful publish was via trusted publisher).
4. **Playground deploy on manual dispatch — be explicit**: `publish-testnet.yml`'s `deploy-app` job (line 53) is gated by `if: ${{ !cancelled() && needs.e2e.result != 'failure' }}` — it does NOT check `github.event_name`. So on every manual dispatch of `publish-testnet.yml`, **the playground WILL deploy** as long as E2E doesn't fail. Codex final-pass clarified: the question isn't "should it also fire", it's "do you want it to fire on every manual SDK publish?". **Decision needed at approval gate**: leave as-is (every SDK dispatch redeploys playground) OR add a workflow input toggle to opt out.

### Risks

- **`-revision.N` users in dApps**: any consumer doing `npm install @alejoamiras/aztec-accelerator@latest` after this gets `4.2.0-revision.1`. The version semantically reads as "a revision of 4.2.0" which is honest. Semver ordering: `4.2.0-revision.1 < 4.2.0` (because prereleases sort before stable). So pinning to `^4.2.0` would NOT pick `4.2.0-revision.1`. dApp consumers using `^4.2.0` keep getting `4.2.0` stable. Consumers using `latest` get `4.2.0-revision.1` (the new code).
- **`latest` semver weirdness**: moving `latest` from `4.2.0-rc.1` to `4.2.0-revision.1` is a sideways move (both are prereleases of 4.2.0). Not a regression — it's the published code's recency that matters, not the version-string ordering. dApps using `latest` were already getting a prerelease, so they continue to. No semver violation. Document in release notes.
- **npm rejects `4.2.0-revision.1`**: theoretical risk, but semver-valid. Verified with `node-semver.valid('4.2.0-revision.1')` → returns the version (will run this check pre-flight).
- **First-time publishing of a new revision-suffix scheme**: trusted publisher attestation may behave differently. Mitigation: same OIDC + provenance flags as before; only the version string differs.

---

## Track 3c — Accelerator release

### Order

1. **`1.0.1-rc.2` dry-run** (now genuinely safe per PR-A fixes):
   ```bash
   gh workflow run release-accelerator.yml -f version=1.0.1-rc.2
   ```
   With prerelease handling in place, this:
   - Tags only after build + build-headless + smoke succeed
   - Creates GitHub release marked `--prerelease`, NOT `--latest`
   - **Does NOT** generate or upload `latest.json` (auto-updater not advertised this version)
   - **Does NOT** open a bump-source PR

2. **Verify `-rc.2`**:
   - All 4 headless platforms green
   - Manual `curl + tar -tz` on one tarball
   - `shasum -a 256 -c` against a sidecar
   - Inspect the GitHub release page: marked prerelease, no `--latest`

3. **`1.0.1` stable**:
   ```bash
   gh workflow run release-accelerator.yml -f version=1.0.1
   ```
   This runs the full pipeline: tag, build, build-headless, smoke, release (latest + auto-updater + bump-source).

### Pre-flight (must verify before triggering `1.0.1`)

1. `-rc.2` succeeded end-to-end including bump-source skipped
2. The `accelerator-v1.0.0` GitHub release is real (`gh release view accelerator-v1.0.0` — confirms `1.0.1` is the right next stable)
3. The `_publish-sdk.yml` publish already created `@alejoamiras/aztec-accelerator@4.2.0-revision.1` on npm (so the SDK and accelerator are version-aligned for users)

### Risks

- **ARM64 first-run failure**: builds with LTO can be 25-30 min on ARM64. Timeout bumped to 45 (PR A.4). If it still times out, drop LTO from the headless target or split the matrix.
- **`-rc.2` release dangling**: it's a real GitHub release tagged `accelerator-v1.0.1-rc.2`. Even though marked prerelease, it shows up in the releases list. Acceptable — releases are immutable history; we don't delete them.
- **`1.0.1` failure mid-flight**: tag-after-build (PR A.3) means a failed build does NOT push the tag. Re-running is safe.

### Auto-updater behavior on `1.0.1`

Existing `1.0.0` users with auto-update enabled will receive `1.0.1` via the `latest.json` published to S3. The signature path (TAURI_SIGNING_PRIVATE_KEY) is unchanged from `1.0.0`. Expected.

### Adversarial check (focused on 3c specifically)

- **A failed `1.0.1` could leave orphan partial artifacts.** Acceptable — release pipeline is `gh release delete` + re-run idempotent.
- **An attacker compromising the GitHub release between artifact upload and `latest.json` upload to S3** would be detected because `latest.json` includes signatures embedded from `.sig` files (Tauri Ed25519). Existing protection.
- **ARM64 runner image trust**: GitHub-hosted standard runner. Same trust as `ubuntu-latest`.

---

## Sequencing

```
┌──────────────────────────────────────────────────────────┐
│ PR A — Deprecation + workflow fixes                      │
│  ├─ A.1 deprecate triggers; A.2 fix publish-version      │
│  │  script; A.3 fix release-accelerator (tag-after-      │
│  │  build + prerelease); A.4 ARM64 timeout 30→45;        │
│  │  A.5 accelerator.yml paths filter; A.6 CLAUDE.md      │
│  └─ merge                                                │
└──────────────────────────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│ Track 2 — Branch cleanup (shell session)                 │
│  ├─ close PRs #222 #212                                  │
│  ├─ delete 7 auto-generated remote branches              │
│  ├─ manual triage 2 orphans                              │
│  └─ local prune                                          │
└──────────────────────────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│ PR B — Forward-roll                                      │
│  ├─ bun scripts/update-aztec-version.ts 4.2.0            │
│  ├─ both SDK + playground updated                        │
│  └─ merge after CI green (incl. accelerator E2E)         │
└──────────────────────────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│ Track 3b — gh workflow run publish-testnet.yml           │
│  └─ SDK 4.2.0-revision.1 published, latest+testnet tags  │
└──────────────────────────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│ -rc.2 dry-run — gh workflow run release-accelerator.yml  │
│                  -f version=1.0.1-rc.2                   │
│  └─ Validates 4-platform headless build matrix; no       │
│     latest.json overwrite, no bump-source PR             │
└──────────────────────────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│ Track 3c — gh workflow run release-accelerator.yml       │
│             -f version=1.0.1                             │
│  └─ Accelerator 1.0.1 stable with headless tarballs      │
└──────────────────────────────────────────────────────────┘
```

## Validation gates (must pass before each merge / trigger)

| Step | Gate |
|---|---|
| PR A merge | actionlint + bun test (incl. publish-version test) + visual diff review |
| Branch cleanup | dry-run output reviewed before any push --delete |
| PR B merge | sdk.yml + app.yml + **accelerator.yml** (now includes sdk/package.json) + actionlint.yml |
| 3b trigger | publish-version script locally returns `4.2.0-revision.1` |
| -rc.2 trigger | n/a (this IS the gate) |
| -rc.2 verify | all 4 headless artifacts present, sha256 verifies, prerelease marker set, no latest.json mutation |
| 1.0.1 trigger | -rc.2 fully successful + accelerator-v1.0.0 release confirmed + SDK 4.2.0-revision.1 published |

## Out of scope

- @aztec 5.x track (upstream is on `5.0.0-nightly.*`; not pursuing here).
- Re-enabling Dependabot.
- Republishing `4.2.0` (npm version immutability + we're now publishing `4.2.0-revision.1`).
- Windows headless build.
- npm wrapper package.
- Code signing for headless tarballs.
- `cargo audit` in CI.

---

## Security & Adversarial Considerations

(Per-track adversarial sections above; consolidated review.)

### Threat surface changes (vs current state)

- **REMOVING** schedule + push triggers reduces attack surface (less code firing). Net positive.
- **PRESERVING** workflow_dispatch keeps same surface but only when YOU trigger. Net neutral.
- **MOVING** npm `latest` from `4.2.0-rc.1` → `4.2.0-revision.1` is controlled. Documented in release notes.
- **PUBLISHING** new SDK version `4.2.0-revision.1` adds an immutable npm artifact. SHA + provenance attestation. Same security model as every prior SDK publish.
- **RELEASING** accelerator `1.0.1` with 4 new headless tarballs (first public). SHA-256 sidecar only; documented as integrity-only.
- **WORKFLOW FIX** moving `tag` to after `build` REDUCES the orphan-tag attack surface (an attacker cannot point a previously-pushed tag to a malicious commit via a re-run because there's no longer a window where the tag exists pre-build).

### Least privilege

- `_aztec-update.yml`: `PAT_TOKEN`, `contents: write`. Unchanged.
- `_publish-sdk.yml`: `id-token: write` (OIDC). Unchanged. New `4.2.0-revision.1` follows same publish path.
- `release-accelerator.yml`: `id-token: write, contents: write, pull-requests: write`. Unchanged. Restructure preserves permission boundaries.

### Cryptography

- Tauri updater: Ed25519 (TAURI_SIGNING_PRIVATE_KEY) unchanged
- macOS code signing: Apple cert unchanged
- npm provenance: OIDC trusted publisher unchanged
- Headless tarballs: SHA-256 sidecars only (PR #225 limitation, accepted)

### Supply chain

- `@aztec/*` `4.2.0` is published, immutable on npm
- No new transitive deps from this work
- bb binary integrity check (`versions.rs:287-310`) unchanged

### Adversarial top-3 (revised)

1. **"Could an attacker exploit the `-revision.N` version-string scheme to publish a malicious version?"** No — `npm publish` is gated by trusted publisher / OIDC tied to this repo. Only this workflow can publish. The version string is computed deterministically from npm state + the package.json's `@aztec/stdlib` dep; an attacker would need to compromise both.
2. **"Could the orphan-tag bug be exploited if we DON'T fix it?"** Yes — a malicious PR that intentionally fails the `build` step (e.g. dependency injection) could push a tag to a known-bad commit, then a separate workflow could re-run from that tag. **Plan addresses this via PR A.3 (tag-after-build).**
3. **"Could moving `latest` to a prerelease version (`4.2.0-revision.1` is technically prerelease per semver) confuse consumers?"** Yes for `^X.Y.Z` consumers; no for `latest` consumers. Documented in release notes. dApp ecosystem already accepted `4.2.0-rc.1` as `latest`, so this is a sideways move.

---

## What changed from v1 (codex + opus audits adopted)

| Finding | Source | Status | Adopted change |
|---|---|---|---|
| `4.2.0.1` is invalid semver | opus + codex | MUST-FIX | A.2 script fix → `4.2.0-revision.1` |
| `-rc.2` is NOT a dry-run | codex | MUST-FIX | A.3 prerelease handling in release-accelerator.yml |
| Track 2 deletes CLOSED PRs too | codex | MUST-FIX | Tightened to SHA-verified merge commit only |
| PR B must touch playground | codex | MUST-FIX | Explicit dual edit; `update-aztec-version.ts` script handles |
| Remote branch count off | opus | factual | 129 → 11 corrected |
| `auto_merge: true` survives | codex | MUST-FIX | A.1 explicitly sets to false |
| accelerator.yml filter excludes sdk/package.json | opus | MUST-FIX | A.5 adds path |
| ARM64 30-min timeout too tight | opus | should-fix | A.4 → 45 min |
| `release-accelerator.yml` tag-before-build | both | must-fix | A.3 restructure |
| Sequencing: PR A before PR B is correct | codex | confirmed | unchanged |
| nightlies tip not reachable from main | opus | confirmed | unchanged |
| `accelerator-v1.0.0` is real stable release | both | confirmed | 1.0.1 is correct next stable |

### Final pass (v2 → v2.5) refinements

| Finding | Status | Adopted change |
|---|---|---|
| `bump-source` is a separate job, can't see step outputs from `release` | MUST-FIX | A.3 surfaces `is_prerelease` as a `validate` job output; downstream `if:` conditions reference `needs.validate.outputs.is_prerelease` |
| Track 2 SHA check should compare to PR head SHA, not merge commit SHA | should-fix | Track 2 step 2 predicate updated: `gh pr list --json headRefOid` + `git ls-remote --heads` |
| Playground deploy phrasing too soft | should-fix | 3b pre-flight item 4 rewritten: "playground WILL deploy on every dispatch unless E2E fails" — decision needed at approval gate |
| `isPrerelease = baseVersion.includes("-")` not fully generic semver | nit (rejected) | Current repo's version space is well-covered; future-proofing deferred |

### Rejected / not addressed in this plan

| Finding | Source | Reason for not addressing |
|---|---|---|
| Force-push history edge in branch cleanup | codex | Mitigated by SHA verification (not predicate complexity) |
| Tag-push via GITHUB_TOKEN may hit branch protection | opus | Pre-existing behavior; user has not reported issue; flag as follow-up if tag push fails |
| 35-day signature trust window from 1.0.0 → 1.0.1 | opus | Signing key unchanged; no auto-update infrastructure change |
| `cargo audit` in CI | both | Out of scope, flagged as follow-up |
