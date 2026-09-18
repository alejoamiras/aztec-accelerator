# B6 — publish/promote split + recovery: implementation plan

Executes the audited design in `plan.md:164-220`. This file is the actionable change map + sequencing,
grounded in fresh recon (2026-08-17). Root design points referenced as (P1)…(P8).

## Current state (recon)

- `.github/workflows/release-accelerator.yml` `release` job (L777-998) does **publish AND promote in one job**,
  one `permissions` block (`contents: write` + `id-token: write`):
  - publish: flatten assets → `gh release create --verify-tag [--prerelease --latest=false | --latest]`
    (L901-979). A **separate `tag` job** (L607-652) pre-creates+pushes the git tag; release uses
    `--verify-tag`.
  - **DELETE-ON-REDISPATCH anti-pattern** (L895-899): `gh release delete "$TAG" --yes` before create — must go.
  - promote: `aws s3 cp latest.json … && cloudfront create-invalidation` (L981-998), `is_prerelease=='false'`
    gated only — no separate approval/gate. This is the seam to extract.
- `verify-live-feed` (L1006-1070) polls the public CDN feed post-promote; `bump-source` (L1072-1168) follows.
- Prereleases already skip: `sign-update-feed`, the S3 upload, `verify-live-feed`, `bump-source` — i.e. RCs
  already "publish without promote". ✅ (the /goal's rc publish→verify-NOT-promote is partly free already.)
- `promote-latest.yml` = **npm-SDK** dist-tag promotion only; unrelated to desktop feed. No desktop promote wf.
- Landing `packages/landing/src/main.ts:58-97` list-scans `api.github.com/…/releases`, picks first
  `accelerator-*` with assets — **no `prerelease:false` filter** (an RC could show as the download), and does
  NOT read the signed S3 feed. No landing tests exist.
- `docs/RELEASE_RUNBOOK.md:87-116` rollback = `gh release delete` + `git push --delete origin <tag>` + manual
  re-upload of a never-stored `previous-latest.json`. Stale 7-step summary (L28-35). CLAUDE.md:16 stale too.
- No workflow-contract tests. Pattern to match: `packages/accelerator/scripts/tauri-identity.test.ts`
  (`expectContains(rel, needle)` = `fs.readFileSync().toContain()`; no YAML parser in repo).

## Design (scoped; codex arbitrates over-engineering)

### 1. Split publish from promote in `release-accelerator.yml` (P1, P2)
- **Extract the S3 promote step** (L981-998) OUT of `release` into a new **`promote` job**. `release` keeps
  only `contents: write` (drop `id-token: write`); `promote` gets `id-token: write` + the AWS role, and its
  own `needs`. This is the least-privilege split (P8 partial: publish leg has NO AWS creds).
- **Stable gates as a DRAFT then finalizes (P2, the BLOCKER-2 resolution):**
  - `release` for **stable** creates a **DRAFT** with `--draft --target <github.sha>` (no tag, assets
    non-public), NOT `--verify-tag --latest`. RC path unchanged (public prerelease, `--latest=false`).
  - The existing 3-OS gates (smoke/update-smoke/webdriver already `needs` of release) + a new
    **verify-published-assets** step run against the draft's assets.
  - A **finalize** step on PASS: `gh release edit <tag> --draft=false --latest=false` (atomic: creates the
    tag at `--target`, makes assets public). On the release JOB failing/gate-fail: **delete the DRAFT**
    (`gh release delete` allowed on a draft — no tag, never public) → re-dispatch same 2.0.0.
  - ⇒ the separate **`tag` job is removed** (draft-publish creates the tag). `--verify-tag` removed.
- **promote job** = the S3 flip, only reachable after a published (non-draft) release exists. It runs its
  own pre-flight (P3): published release exists ∧ latest.json asset present ∧ production-verifier signature
  OK ∧ feed_version==target ∧ full asset completeness (count + every platform URL resolves) → `aws s3 cp` +
  invalidate → **PushNotification immediately** → `verify-live-feed` follows.
- **promote-only dispatch mode (P3):** add a `mode` input (`full` default | `promote-only`) OR a distinct
  `promote_version` input; `promote-only` skips build/gate/publish and runs just the pre-flight + promote +
  verify-live-feed against an already-published version. **Rollback = `promote-only promote_version:1.0.7`.**
- **Append-only policy (P6):** remove the delete-on-redispatch (L895-899) for PUBLISHED releases (draft
  delete only); ban `--clobber`; every `gh release create/edit` pins `--latest=false` (feed is the source of
  truth, not the GitHub Latest badge).

### 2. Landing reads the SIGNED S3 FEED (P4)
- `packages/landing/src/main.ts`: replace the GitHub-API list-scan with a fetch of
  `https://aztec-accelerator.dev/releases/latest.json`. Treat the feed as **UNTRUSTED**: strict-SemVer-parse
  `.version` (reject otherwise), and construct asset URLs ONLY against the canonical
  `github.com/alejoamiras/aztec-accelerator/releases/download/accelerator-v<version>/…` path — never
  interpolate a feed field into a host/path. Keep the static GitHub `/releases` fallback for fetch failure.
  This also fixes the no-prerelease-filter bug (the feed only ever carries the promoted stable).

### 3. Rehearsal (P7) — staging-key, no prod downgrade
- Parameterize the promote step on the S3 key (default `landing/releases/latest.json`). A pre-GA rehearsal
  path promotes BOTH 2.0.0 and 1.0.7 to `landing/releases/rehearsal/latest.json`, verifies each, removes —
  proving rollback machinery without serving a downgrade. (Implement as a `promote_key` input or a small
  rehearsal job; keep minimal — codex arbitrates.)

### 4. Docs (P8)
- Rewrite `docs/RELEASE_RUNBOOK.md` rollback → append-only/fix-forward + `promote-only 1.0.7` lever; refresh
  the stale 7-step summary. Update CLAUDE.md release-pipeline line + workflow inventory (add promote-latest,
  update-feed-health). Add the manual trust-install pre-release checklist stub.

## File-level change map
- `.github/workflows/release-accelerator.yml` — the restructure above (remove `tag` job + `--verify-tag`;
  draft-gate-finalize in `release`; new `promote` job; promote-only mode; append-only).
- `packages/landing/src/main.ts` — signed-feed sourcing + untrusted-input hardening.
- `packages/landing/src/*.test.ts` (NEW) — landing-reads-feed unit (feed parse + URL construction + reject
  bad SemVer + fallback).
- `packages/accelerator/scripts/release-contract.test.ts` (NEW) — workflow-contract rows (below).
- `docs/RELEASE_RUNBOOK.md`, `CLAUDE.md` — rewrites.
- Possibly a small `scripts/` helper for the promote pre-flight verifier if inlining is unwieldy.

## Test plan (each mutation-provable by a 1-line YAML/code edit; match tauri-identity.test.ts style)
Workflow-contract rows (`release-contract.test.ts`):
1. publish leg (`release` job) has NO `id-token: write` and NO `AWS_ROLE_ARN_RELEASE` (creds live only on
   `promote`).
2. stable creates a DRAFT (`--draft` + `--target`); no `--verify-tag`.
3. promote job (S3 `aws s3 cp latest.json` + invalidation) is `is_prerelease=='false'`-gated and lives in a
   job carrying `id-token: write`.
4. no `gh release delete` on a PUBLISHED release anywhere (draft delete allowed → assert the delete is
   guarded to draft-only / not present in the publish path).
5. no `--clobber` anywhere; every `gh release create/edit` carries `--latest=false`.
6. asset-completeness gate present (16 build assets; 17 with latest.json on the published stable).
7. `bump-source` still `needs` `verify-live-feed` (promotion proven before source bump).
8. `verify-live-feed` keyed to the promoted version.
Landing unit: feed→version parse; reject non-SemVer; canonical URL construction; GitHub fallback on fetch
fail. Promote pre-flight fixture tests (wrong version / unsigned / missing asset / feed-version mismatch →
refuse). `actionlint`; `auth_probe` graph dry-run.

## Security & Adversarial
- **Least privilege**: publish leg loses AWS creds; promote is the only leg that can flip the feed.
- **Append-only / immutability**: no delete/clobber of published releases → a bad promote is fixed by
  `promote-only <prev>` (rollback lever), never by rewriting history (respects the /goal NEVER list).
- **Untrusted feed at landing**: the Ed25519-signed feed is not signature-verified by the browser, so its
  payload shape is untrusted → strict SemVer + canonical-host URL construction (no host/path injection).
- **Draft privacy**: failed stable bytes never go public (draft), so recovery stays inside 2.0.0
  authorization — no burned version, no 2.0.1 (which would be STOP-and-surface).
- **Promote pre-flight**: refuse to flip the feed unless the published release + signed feed + version +
  full asset set all agree — prevents a half-built or version-mismatched feed going live.

## Sequencing (small, mutation-proved, codex-reviewed steps)
1. `release-contract.test.ts` scaffolding + the rows that pass against the CURRENT yaml (baseline), so I can
   see them flip as I edit. (Some rows will start RED — those encode the target state.)
2. Workflow restructure: extract promote job + drop id-token from release + append-only (remove L895-899).
3. Draft-gate-finalize for stable + remove `tag` job.
4. promote-only mode + rollback lever + rehearsal key.
5. Landing signed-feed + landing unit test.
6. Runbook + CLAUDE.md rewrites.
7. `bun run lint:actions` + `bun test` + `actionlint` green; codex loop until clean; PR → CI → merge.

**Open risk to watch:** the draft-gate changes tag-creation timing (draft-publish makes the tag, not the
`tag` job). Must confirm `gh release edit --draft=false` on a draft created with `--target <sha>` creates the
tag at that sha (GitHub docs: yes). Mutation-prove the finalize path in a fixture/dry-run where possible;
the real proof is an rc dry-run at release time.

## Codex design-review refinements (session codex-a12C8LkE — ADOPTED)

Codex confirmed the draft-gate + least-privilege split but corrected the TOPOLOGY. Adopted:

- **Modes = `publish | promote-only` (TWO dispatches), not `full`/auto-promote.** Auto-running promote after
  release is only a credential split; the audited OPERATIONAL split means stable *publication ends*, and a
  SEPARATE human `promote-only` dispatch flips the feed. Matches the /goal ("rc through publish→verify NOT
  promote" then separately "promote latest.json → 2.0.0").
- **Draft-asset gate is by SHA-256 equality, not reorder.** The existing 3-OS gates (smoke/update-smoke/
  webdriver) run on Actions ARTIFACTS before the draft exists, so they don't literally test draft assets.
  Rather than reorder the heavy gates behind an authenticated draft-download, the release job SHA-256-asserts
  every uploaded draft asset == the gated artifact bytes (they're identical by construction — the draft is
  built FROM those artifacts — the assert just makes it explicit). Root-plan "installed journey against
  draft assets" is revised to "gates test artifacts; SHA-256 proves draft == gated bytes." (B4's packaged
  E2E may later run against draft-downloaded assets for a true on-draft journey.)
- **RC also uses `--target "$GITHUB_SHA"`** (after removing the `tag` job) so an RC tag can't follow a moved
  `main`. Before finalize: assert the tag does NOT already exist (`target_commitish`/`--target` is IGNORED
  if the tag exists), finalize, then POST-assert the created tag's SHA == `github.sha`.
- **Draft cleanup guards `isDraft == true` immediately before delete** (never `gh release delete` by tag name
  — that could nuke a published release; respects the /goal NEVER-delete-published rule).
- **Permissions:** `release` = `contents: write` ONLY; `promote` = `id-token: write` + `contents: read`.
  Keep `release-auth-preflight` as a direct prerequisite of publication.
- **Mode guards (what-breaks-the-release list):** every build/sign/publish job gated on `mode == 'publish'`;
  `release` must NOT run under promote-only (tighten its `!cancelled()` if with a `mode` check); `promote`
  must NOT `needs: release` transitively in a way that skips it under promote-only; `bump-source` must key
  off the promote/verify path (organic-GA only), not a direct `needs: release`, or a separate promotion
  never bumps source.
- **promote-only pre-flight (strengthened):** exact tag ∧ `draft==false` ∧ `prerelease==false` ∧ canonical
  stable SemVer ∧ **exact asset-NAME set** (not just count) ∧ canonical feed URLs; download the release's
  exact `latest.json`, run the production verifier, size/signature-check all four updater payloads; upload
  THOSE verified bytes unchanged. **Declaration-equality (conf+Cargo == input) is PUBLISH-ONLY** — it must
  NOT run under promote-only, or rollback to `1.0.7` (whose declarations differ from the dispatched version)
  becomes impossible.
- **Rehearsal: CUT the staging-key S3 write.** Replace with a NO-WRITE preflight dry-run that runs the
  promote-only pre-flight against BOTH 2.0.0 and 1.0.7 (proves the rollback pre-flight passes without
  serving anything). Keep the contract + fixture tests. **`mark-GitHub-Latest` is the one deliberate
  exception to the `--latest=false` rule** (cosmetic, best-effort) — the contract test must exempt it.
- **Landing:** my `feed.ts` already matches codex's guidance (strict `X.Y.Z` only, malformed→null→static
  fallback, no host/path injection). Optional simplification (constant per-OS filename mappings instead of
  the GitHub-API asset lookup) deferred — the current lookup uses GitHub-origin `browser_download_url` and
  codex found "no remaining URL injection". Add a `landing.yml` PR gate (currently landing has NO PR test
  gate — deploy-landing runs post-merge only) so the feed unit test actually gates.
