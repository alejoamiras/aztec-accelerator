# C5 / F-005 — infra-deploy-authz — CONSOLIDATED plan (deep tier)

Consolidation of three independent plans (`C5-plan-main.md`, `C5-plan-codex.md`, `C5-plan-fable.md`).
They converged strongly; the Decision Ledger below records where they diverged and why the consolidated
choice was made. **Commit + validate only — a human runs `tofu apply`, `gh secret`, and the ruleset API.**

> **GATE 1 status:** deep-tier audit COMPLETE — 3 plans → double audit (Fable *conditional approve* + Codex *reject*, both fully folded) → final fresh-context Codex verdict **APPROVE** (round 5). Awaiting the user's approval-gate decision before GATE 2.

## Outcome
Replace the single over-broad `aztec-accelerator-ci-github` role (trusted by 4 refs, whole-bucket write)
with **three per-pipeline roles**, each trusted ONLY to its own workflow FILE running on `main`, so no
pipeline — nor a compromised *other* workflow on main — can write another prefix. Only the release
pipeline may write the F-004-critical `landing/releases/latest.json`. Narrow OIDC to `main` (drop the
unused `nightlies` + `chore/aztec-*`), harden `main` protection (solo-repo-appropriate), and fix a latent
feed-deletion bug in the landing sync. Nightlies is out of scope (branch unused).

| Role (`aztec-accelerator-ci-*`) | Trust: workflow-file @ main | S3 authority | Consumed by · secret |
|---|---|---|---|
| `…-landing` | `deploy-landing.yml` | Put/Delete `landing/*`; **Deny** Put/Delete on `landing/releases` + `landing/releases/*`; ListBucket prefix `landing/`; GetBucketLocation; CreateInvalidation | deploy-landing.yml · `AWS_ROLE_ARN_LANDING` |
| `…-release-feed` | `release-accelerator.yml` | Put ONLY `landing/releases/latest.json`; GetBucketLocation; CreateInvalidation (no List, no Delete) | release-accelerator.yml · `AWS_ROLE_ARN_RELEASE` |
| `…-playground-testnet` | `publish-testnet.yml` | Put/Delete `playground/*`; ListBucket prefix `playground/`; GetBucketLocation; CreateInvalidation | publish-testnet.yml · `AWS_ROLE_ARN_PLAYGROUND` |

Each role trust conditions on `StringEquals`: `aud=sts.amazonaws.com`, `sub=repo:alejoamiras/aztec-accelerator:ref:refs/heads/main`, **and the `workflow` NAME claim** (see Ledger D1 — resolved after the audits contradicted each other on `job_workflow_ref`) binding it to exactly one workflow: `Deploy Landing Page` / `Release Accelerator` / `Publish Testnet`.

## Corrections to the original brief (verified in-tree — all three legs caught these)
1. **`deploy-landing.yml:38-41` uses `sync --delete`** (not "no --delete"). `packages/landing/dist/` has no
   `releases/`, so the sync `--delete` DELETES `landing/releases/latest.json` (the F-004 feed) after every
   post-release landing deploy — a **latent live bug**. The IAM Deny on `landing/releases/*` would also
   make landing deploys FAIL on that delete. Fix BOTH with `--exclude "releases/*"` on the sync.
2. **The OpenTofu PR gate already exists** — `actionlint.yml`'s `tofu` job (pinned OpenTofu 1.10.0,
   `fmt -check` + `init -backend=false` + `validate`, paths-filter on `infra/tofu/**/*.tf`, runs on PRs to
   `main` + `security-hardening`, `.terraform.lock.hcl` committed). REUSE + tighten it; do NOT add a
   duplicate workflow (my main-plan Phase 4 is dropped).
3. **`release-accelerator.yml` is dispatch-only**; the run's OIDC `sub` stays `refs/heads/main` (the tag it
   creates does not re-trigger). No tag ref needed. Add an early `refs/heads/main` assertion so a release
   dispatched from another ref fails BEFORE side effects.

## Decision Ledger (divergences resolved)
- **D1 — workflow-binding claim (RESOLVED after a double-audit CONTRADICTION).** All roles share
  `sub=main`, so `sub` alone does NOT isolate pipelines from each other; bind each role to its workflow.
  The two audits DISAGREED on the file-path claim: **Fable** said `job_workflow_ref` is present for
  top-level jobs (== the workflow ref) and AWS-supported; **Codex** said `job_workflow_ref` is emitted
  ONLY for jobs inside a *reusable* workflow — these three S3 jobs are TOP-LEVEL, so their token carries
  `workflow_ref` (which AWS does NOT expose as a condition key), NOT `job_workflow_ref` → every AssumeRole
  would fail. This dispute is load-bearing (a wrong choice = total deploy outage), so we do NOT guess.
  **CHOSEN: the `workflow` (NAME) claim** — `token.actions.githubusercontent.com:workflow`, StringEquals,
  exact values `Deploy Landing Page` / `Release Accelerator` / `Publish Testnet` (the `name:` of each
  workflow). BOTH audits + the AWS GitHub-tab doc agree `workflow` is AWS-supported AND present for ALL
  jobs (top-level or reusable). It addresses the actual threat this binding targets — a STOLEN token
  exfiltrated from one pipeline's run cannot assume another role. Its only weakness (a PR that RENAMES a
  workflow's `name:` to impersonate another) requires MERGING to main, i.e. it is a strict subset of the
  already-accepted "malicious code on main" residual (unstoppable by ANY claim). The stronger file-path
  binding (`job_workflow_ref` value `alejoamiras/aztec-accelerator/.github/workflows/<file>.yml@refs/heads/main`
  — NO `repo:` prefix, which is `sub`-only) is deferred as an OPTIONAL upgrade (ASK A7) requiring EITHER
  empirical proof the claim is present for these top-level jobs, OR wrapping each S3 job in a reusable
  workflow so `job_workflow_ref` is unambiguously populated. MANDATORY runbook gate regardless: a
  **negative cross-role AssumeRole smoke** — from main, a scratch/other workflow attempts
  `role-to-assume: <release ARN>` and MUST be DENIED. It is the ONLY gate that catches a mis-bound claim or
  a "drop the claim to unblock deploys" degraded hotfix (positive smokes + `simulate-principal-policy`
  never evaluate TRUST policies → they'd pass silently). Residual (all variants): malicious code *already
  merged to main* runs as the legit workflow — mitigated by main protection + F-004 client-side manifest
  verification.
- **D2 — release policy scope.** Codex: exact object `landing/releases/latest.json`. main/fable:
  `landing/releases/*`. **CHOSEN: exact object** (tightest least-privilege; the only object the release
  writes). Note in README: adding a second release-feed object requires a policy update.
- **D3 — 4th required check (OpenTofu validate / Actionlint Status).** fable: recommend YES. codex: flags
  its absence as a material residual in a zero-review repo. main: didn't require. **CHOSEN: RESOLVED YES**
  — Phase 1 requires `Actionlint Status` (integration_id 15368, a fail-closed aggregate that runs the tofu
  gate) as the 4th required check, so infra validation gates the merge rather than being advisory. It is
  surfaced to the user as A1 only as a **veto opportunity** (drop the one line if they disagree), NOT as an
  undecided question.
- **D4 — runtime cross-assumption isolation.** fable proposed GitHub `environment:`-scoped subs.
  main/codex close the same gap via a workflow claim (D1). **CHOSEN: the `workflow` NAME claim (D1) is
  primary** (no environment plumbing needed; stops the stolen-token threat); environments are an optional
  defense-in-depth ASK A2 (with the trust-subject footgun noted there), and the stronger file-path binding
  is ASK A7.
- **D5 — role granularity (3 vs 2).** All legs: keep 3 (per-pipeline). 2-role (feed vs sites) protects the
  feed but lets a compromised site token cross-deface landing↔playground; 3 roles are the minimum aligned
  with distinct public-damage domains and the correct shape for D1/D4. **CHOSEN: 3 roles.**

## Phases

### Phase 1 — PR-1: additive IAM split + ruleset JSON (commit only; deploy-compatible)
Files: `infra/tofu/iam.tf`, `infra/tofu/outputs.tf`, `infra/tofu/s3.tf`, `infra/tofu/README.md`, `infra/rulesets/main-branch-protection.json`.
- `iam.tf`: keep the OIDC provider. **Narrow** the legacy `aws_iam_role.ci` trust to `StringEquals sub=main`
  (drop nightlies + both chore/aztec-* NOW) but LEAVE its broad policy (all live workflows still use it
  until PR-2 lands). **Add** the 3 new roles + inline policies exactly as the table above. **Trust
  (StringEquals):** `aud=sts.amazonaws.com`, `sub=repo:alejoamiras/aztec-accelerator:ref:refs/heads/main`,
  **and `token.actions.githubusercontent.com:workflow` = the exact workflow NAME** (`Deploy Landing Page` /
  `Release Accelerator` / `Publish Testnet`) — per D1 (the `workflow` NAME claim, NOT `job_workflow_ref`
  which the audits could not confirm is emitted for these top-level jobs). Policies: landing carries the
  explicit Deny + `s3:AbortMultipartUpload` on both Allow and Deny; release = Put exact latest.json +
  GetBucketLocation + invalidation only (no List/Delete); playground mirrors landing sans Deny. Keep
  `ListBucket`/`GetBucketLocation` in SEPARATE statements from the object actions; `ListBucket` uses
  `StringLike s3:prefix ["landing/*"]` / `["playground/*"]` (NEVER put an `s3:prefix` condition on
  `PutObject` — it would implicit-deny uploads).
- `s3.tf`: add a bucket-wide `aws_s3_bucket_lifecycle_configuration` rule
  `AbortIncompleteMultipartUpload` (1 day) so a compromised token can't park billable incomplete parts
  (Codex C6). [If ASK A8 accepted, also enable `aws_s3_bucket_versioning`.]
- `outputs.tf`: add `landing_deploy_role_arn`, `release_feed_role_arn`, `playground_testnet_role_arn`;
  keep `ci_role_arn` (removed in PR-3).
- `main-branch-protection.json`: keep target=main only, `bypass_actors: []`, 0 approvals, the 3 checks +
  integration_id 15368; ADD rule types `deletion`, `non_fast_forward`, `required_linear_history`; in the
  `pull_request` rule's `parameters` set `required_review_thread_resolution: true` and
  `allowed_merge_methods: ["squash","rebase"]` (it is a `pull_request.parameters` field, NOT top-level;
  lowercase; linear history needs squash/rebase); set `required_status_checks.parameters.strict_required_status_checks_policy: true`
  (Codex C5 — else stale-green merges). **Append the 4th required check `Actionlint Status`
  (integration_id 15368)** — A1 is RESOLVED YES (audit-recommended; runs the tofu gate; the user may veto
  it at the approval gate, in which case drop this one line).
- `README.md`: document the temporary dual-role window + the 3 new secrets + the `--exclude "releases/*"`.
- **Validation gate:** `tofu -chdir=infra/tofu fmt -check -diff && init -backend=false -input=false && validate`;
  `bun run lint:tofu`; `bun run lint:actions`; `jq -e . infra/rulesets/main-branch-protection.json`;
  `git diff --check` → all exit 0. Plus the existing `Lint OpenTofu` PR job green. (Layers: infra fmt +
  tofu-validate + actionlint + json-lint.)

### Phase 2 — PR-2: workflow cutover to per-pipeline secrets + safety asserts (commit only)
Files: `deploy-landing.yml`, `release-accelerator.yml`, `publish-testnet.yml`.
- `deploy-landing.yml`: `role-to-assume: ${{ secrets.AWS_ROLE_ARN_LANDING }}`;
  `sync --delete --exclude "releases/*" --exclude "releases"` (both, since IAM Denies the exact
  `releases` key too); early `refs/heads/main` assertion for the dispatch path; optional post-sync
  feed-survival assert via `aws s3api list-objects-v2 --prefix landing/releases/latest.json` (NOT
  head-object — the landing role has no `s3:GetObject`), or omit it.
- `release-accelerator.yml`: `${{ secrets.AWS_ROLE_ARN_RELEASE }}` at the S3-write step (~L899). **Codex
  C3 — trust must be proven BEFORE the git tag** (the `tag` job pushes `accelerator-vN` ~L555, BEFORE the
  `release` job authenticates): add an **AWS preflight to the EARLY `validate` job** (job-scoped
  `id-token: write`, `configure-aws-credentials` with the release role, a harmless `s3:GetBucketLocation`)
  and make the `tag`/`release`/publish jobs `needs:` that validation, so a post-cutover trust/secret
  failure aborts before any tag/release/feed side effect. Add an `auth_probe` `workflow_dispatch` boolean input to prove the release OIDC token
  assumes the release role BEFORE the legacy role is deleted. **The probe MUST have zero side effects:
  when `auth_probe == true`, ONLY the preflight auth job runs and EVERY build/tag/release/publish job is
  guarded `if: inputs.auth_probe != true` (or gated behind a `needs:` on a job that early-exits under the
  probe) so a probe run cannot build, tag, publish, or release.** Assert `GITHUB_REF == refs/heads/main` early; keep the exact S3 key; fix the invalidation path
  to `/landing/releases/latest.json` (viewer-function rewrite → cached key is `landing/releases/latest.json`),
  keeping `/releases/latest.json` too.
- `publish-testnet.yml` (~L80): `${{ secrets.AWS_ROLE_ARN_PLAYGROUND }}`; scope `id-token: write` to the
  `deploy-app` job **and the `_publish-sdk.yml` call job** (both need it — the latter for Sigstore
  provenance); main-ref validation gating the side-effecting jobs.
- **`publish-nightlies.yml`: EXPLICITLY DISABLE the dispatch path** (Codex C2) — its jobs guarded so it
  cannot run at all (e.g. an early `if: false`-style guard / top-level early-exit with a comment citing the
  nightlies-dropped decision). It runs `_publish-sdk.yml` (irreversible npm publish) + a separate AWS
  `deploy-app` job, so leaving it referencing the to-be-deleted secret risks a partial irreversible run.
  Do NOT merely leave it "untouched". `_publish-sdk.yml`: **untouched.**
- **Trigger-safety:** PR-2 changes only `.github/workflows/**`, which doesn't match deploy-landing's
  `paths: packages/landing/**` push filter → merging cannot itself fire a deploy.
- **Validation gate:** `bun run lint:actions`; `rg -n 'secrets\.AWS_ROLE_ARN([^_]|$)' .github/workflows`
  returns NO live-pipeline hit (publish-nightlies is now disabled); `Actionlint Status` green on the PR.

> Phases 1 + 2 land in the SAME C5 PR into `security-hardening` (both are commit-only). They are separated
> here because the human-apply runbook must apply them at different points (see Phase 4). The legacy-role
> DELETION (Phase 3) is a DISTINCT, SEPARATE PR — see below.

### Phase 3 — Legacy role removal (SEPARATE post-smoke PR — NOT in the campaign's main integration)
**Codex C4:** Phase 3 must NOT ride the campaign's `security-hardening→main` integration. If the deletion
commit reaches final `main`, an ordinary `tofu apply` from current source would remove the fallback role
BEFORE the human finishes smoking the new roles. Keep Phase 3 as its own PR, opened + merged only AFTER
the new roles are proven live (runbook step). This keeps repo source, live AWS state, and the intended
ordering in agreement.
Files: `infra/tofu/iam.tf` (delete `aws_iam_role.ci` + `_policy.ci`), `infra/tofu/outputs.tf` (delete `ci_role_arn`).
Files: `infra/tofu/iam.tf` (delete `aws_iam_role.ci` + `_policy.ci`), `infra/tofu/outputs.tf` (delete `ci_role_arn`).
- **Validation gate:** same tofu fmt/validate + lint:actions + git diff --check; PR/commit body states the
  expected plan: **2 destroy, 0 add, 0 change, −1 output**; no bucket/distribution changes.

### Phase 4 — Human-applies runbook (`clusters/C5-runbook.md`; written, NOT executed)
Adopt Codex's staged, fail-closed sequence + Fable's `simulate-principal-policy` proofs. Critical ordering
(the whole cutover-safety trick). The **ruleset is applied EARLY — BEFORE the final
`security-hardening→main` integration PR** (Codex C5) so it protects the cutover integration itself, NOT
only later changes; it is independent of the role cutover:
```
preflight (drift/squash-rebase/role-names/Org+boundary check) → back up live ruleset → APPLY RULESET
  (PUT existing / POST if absent) BEFORE the main-integration PR
→ narrow+add roles (apply) → set 3 secrets → land workflow cutover ON MAIN
→ smoke new roles (incl. the D1 negative cross-role AssumeRole smoke + the release auth_probe)
→ simulate-principal-policy proofs → [separate Phase-3 PR] destroy legacy role+policy → delete old AWS_ROLE_ARN secret
```
Key points baked into the runbook:
- **The legacy role is retired only AFTER the workflow cutover reaches `main`** — merging C5 into
  `security-hardening` alone is NOT enough (live main workflows still reference the old secret). So the
  final cutover completes during / after the campaign's main-integration.
- Preflight: `tofu plan` = no drift; verify squash/rebase enabled (linear history needs it); verify the 3
  role names are unused; confirm no old-role deploy in flight; brief deploy/merge freeze.
- `simulate-principal-policy` matrix: landing Allow `landing/index.html`, explicitDeny `landing/releases/latest.json`,
  implicitDeny playground; release Allow exact latest.json, implicitDeny delete + other objects; playground
  Allow playground, implicitDeny landing; each role Allow invalidation.
- Ruleset: back up the live ruleset first; PUT if it exists (capture id), POST if absent; assert live state
  with `jq`; restore-from-backup path documented. Never test force-push/deletion against main.
- Every mis-ordering fails CLOSED (empty/typo secret, role-deleted-first, cross-wired secrets all → visible
  red run, never a privilege grant). Reversible: keep old role+secret until smokes pass; the legacy ARN is
  name-derived and recreatable.

## Security & Adversarial Considerations
- **Core control:** per-pipeline roles + the `workflow` NAME claim binding (D1) → a stolen landing/playground
  token cannot assume the release role; only `release-accelerator.yml` on main can write `latest.json`. (A
  compromised *other* main workflow that RENAMES itself to impersonate is the accepted malicious-main
  residual.)
- **Residual — CloudFront invalidation is distribution-wide** (no IAM path condition key). Accepted: cache
  churn / cost only, never a content write. Per-path would need separate distributions (out of scope).
- **Residual — malicious code already merged to main** runs as the legit workflow (any claim variant).
  Mitigated by main protection (PR + checks + linear history + no-force + thread-resolution) and F-004
  client-side manifest verification (a forged/rolled-back feed is rejected client-side).
- **Residual — feed DELETION is now impossible from CI** (only the release role touches the prefix, and it
  has no Delete; landing is explicitly denied) — a net improvement over today, where routine landing
  deploys delete the feed.
- **Owner/admin compromise** can rewrite rulesets/secrets/workflows — no second human authority in a solo
  repo (hardware-key 2FA is the out-of-scope lever, ASK A3).
- **No secrets created by CI**; OIDC only; `contents: read` defaults; existing SHA-pinned actions retained.
- **Supply chain / crypto / least-privilege asks** all addressed above; the change is pure least-privilege.

## Assumptions
### Facts (verified in-tree)
- One role + whole-bucket policy + 4-ref StringLike trust in `iam.tf`; single bucket, `landing/` +
  `playground/` prefixes.
- `deploy-landing.yml` + `publish-testnet.yml` use `sync --delete`; release does one exact `cp`.
- OpenTofu PR gate already exists in `actionlint.yml` (pinned 1.10.0, fmt+init-backend=false+validate).
- release-accelerator is dispatch-only; OIDC sub is `refs/heads/main` **when dispatched from main** (a
  wrong-ref dispatch fails closed at the early main-ref assert / at AssumeRole) — the tag it creates does
  not re-trigger. [Fable L2: reworded from the earlier unconditional "sub stays main".]
- 4 workflows reference `secrets.AWS_ROLE_ARN` (deploy-landing, release, publish-testnet, publish-nightlies).
- Ruleset targets main only, 0 approvals, 3 checks (integration_id 15368 = GitHub Actions app), no
  linear/force-push/deletion.
- Each S3-writing job is top-level in its own workflow file → the workflow-file claim binds per pipeline.
### Inferences (verify at implementation / preflight)
- The workflow-binding claim is RESOLVED (D1): the two audits CONTRADICTED each other on whether
  `job_workflow_ref` is emitted for top-level jobs, so the plan uses the `workflow` NAME claim (AWS-supported
  + present for all jobs). The `job_workflow_ref` file-path binding (value
  `alejoamiras/aztec-accelerator/.github/workflows/FILE.yml@refs/heads/main` — NO `repo:` prefix;
  `workflow_ref` is NOT an AWS key) is the deferred stronger option (ASK A7), needing empirical proof or
  reusable-workflow wrapping. The negative cross-role AssumeRole smoke (D1) is the runtime proof either way.
- The repo still uses the pre-immutable OIDC `sub` format (`ref:refs/heads/main`) — preflight `gh api
  .../actions/oidc/customization/sub`.
- `aws s3 sync --exclude "releases/*"` protects `landing/releases/*` from `--delete` (documented CLI
  behavior, destination-relative for delete candidates; R6 `list-objects-v2` confirms empirically — NOT
  head-object, the landing role has no GetObject [Fable M2]).
- `simulate-principal-policy` predicts IDENTITY-policy authz only; it never evaluates TRUST policies/OIDC
  claims — hence the separate negative-trust smoke (no SCPs/permission boundaries in a solo account).
- Squash or rebase merging is enabled (required for linear history) — preflight confirms.
### Asks (surface at the approval gate)
- **A1 (RESOLVED YES — veto opportunity only):** Phase 1 requires `Actionlint Status` (integration_id
  15368, runs the tofu gate) as a 4th required check so infra validation gates the merge. The plan's
  position is YES (audit-verified it reports on every PR to main, no deadlock); flag here ONLY if you want
  it dropped.
- **A2:** additionally adopt per-pipeline GitHub `environment:` scoping (defense-in-depth)? [Optional —
  **FOOTGUN (Fable M3): adding `environment:` changes the default `sub` to `…:environment:NAME`, which
  BREAKS the `sub=main` trust unless the trust conditions are switched to the native AWS `environment`
  condition key at the same time.** Not needed given the `workflow`-name binding (D1) already isolates
  pipelines.]
- **A3:** enable hardware-key 2FA on the owner account + confirm tfstate bucket is owner-only + versioned?
  [Out of repo scope; single biggest remaining lever]
- **A4:** confirm the brief deploy/merge freeze during the human cutover is acceptable.
- **A5:** has a landing deploy already deleted the live `latest.json`? (preflight `list-objects-v2`; if
  missing, re-run the last stable release's upload after smokes.)
- **A6 (Fable M1/L3):** the user confirmed the `nightlies` BRANCH is unused (2026-07-14). Note the
  consequence: after the trust-ref drop + legacy-secret deletion, `publish-nightlies.yml` fails closed at
  `configure-aws-credentials`, and the live `nightly-playground.aztec-accelerator.dev` → `/playground-nightly`
  CloudFront route (still present in `cloudfront.tf`) becomes unwritable by any role. This is an ACCEPTED
  documented dormancy (not a silent retirement); if the nightly playground deploy path is ever revived, add
  a 4th `playground-nightly/*` role. Confirm acceptance at the gate.

## Double-audit fold (Fable — conditional approve, all 8 folded; Codex leg pending)
- **H1 → folded** in D1 (value format corrected; `workflow_ref` removed; negative cross-role AssumeRole
  smoke mandated in the runbook).
- **M1/L3 → A6** above (nightly dormancy documented + accepted; publish-nightlies fail-closed noted).
- **M2 → folded**: Phase-1 landing policy uses `StringLike s3:prefix ["landing/*"]` (not `StringEquals
  "landing/"`); the optional Phase-2 post-sync feed assert uses `aws s3api list-objects-v2 --prefix
  landing/releases/latest.json` (NOT head-object — the landing role has no `s3:GetObject`), or is dropped.
- **M3 → A2** annotated (environment→sub breakage + native `environment` key).
- **L1 → folded**: `allowed_merge_methods` is a `pull_request.parameters` field (lowercase
  `["squash","rebase"]`), NOT a top-level ruleset field; the runbook asserts it on the fetched live ruleset.
- **L2 → folded** in Facts (OIDC-sub wording qualified).
- **L4 → folded**: Phase-2 scopes `id-token: write` to the `deploy-app` job **and the `_publish-sdk.yml`
  call job** (both need it — the latter for Sigstore provenance).

## Double-audit fold (Codex — REJECT, all blockers folded)
Codex rejected the pre-fold plan; every finding is folded (all are plan-text / sequencing corrections):
- **C1 (Critical) → D1 rewritten** above: the Codex↔Fable contradiction on `job_workflow_ref` (present for
  top-level jobs?) is resolved by using the `workflow` NAME claim (AWS-supported + always present).
  file-path binding deferred to ASK **A7**. Negative cross-role AssumeRole smoke retained.
- **C2 (High) → nightlies must be EXPLICITLY retired, not silently broken.** `publish-nightlies.yml` is
  `workflow_dispatch` and CAN be dispatched on main (operators select the ref); it runs `_publish-sdk.yml`
  (irreversible **npm publish**) in one job and `deploy-app` (AWS) in another. Deleting the legacy role +
  secret would let a dispatch publish to npm SUCCESSFULLY while `deploy-app` fails — a partial, irreversible
  run. **FOLD (Phase 2):** explicitly disable the dispatch path of `publish-nightlies.yml` (e.g. a guard
  `if: ${{ github.event_name == 'workflow_dispatch' && false }}` on its jobs, or a top-level early-exit
  step, with a comment pointing at the nightlies-dropped decision) so it cannot run at all — NOT merely
  leave it referencing a to-be-deleted secret. Supersedes A6's "documented dormancy": dormancy is made
  ACTIVE (the workflow is inert), removing the irreversible-npm risk.
- **C3 (High) → release trust must be proven BEFORE the git tag.** `release-accelerator.yml` pushes the tag
  (`tag` job, ~L555) BEFORE the `release` job authenticates to AWS; moving AWS creds before `gh release
  create` is not enough — a post-cutover trust/secret failure leaves a dangling `accelerator-vN` tag with
  no release/feed. **FOLD (Phase 2):** add a stable-release AWS PREFLIGHT to the early `validate` job
  (job-scoped `id-token: write`, configure the release role, call a harmless `s3:GetBucketLocation`), and
  make the `tag`/`release`/publish jobs `needs:` that validation. Add an `auth_probe` dispatch input (or a
  scratch workflow) so the human can prove the release OIDC token assumes the release role BEFORE deleting
  the legacy role.
- **C4 (Med) → Phase 3 (legacy deletion) is a SEPARATE post-smoke PR**, NOT part of the campaign's
  security-hardening→main integration. Otherwise an ordinary `tofu apply` from final `main` removes the
  fallback role before smokes finish. The "no in-flight runs" preflight must include QUEUED + RUNNING jobs
  for all old-role consumers (deleting a role revokes already-issued sessions).
- **C5 (Med) → ruleset hardening.** Set `strict_required_status_checks_policy: true` (else stale-green
  commits merge). Resolve **A1 = YES** (require `Actionlint Status`; its own workflow already declares it
  required). Apply the ruleset BEFORE the final security-hardening→main PR so it protects the cutover
  itself.
- **C6 (Med) → multipart-upload cost.** `PutObject` authorizes multipart init/complete; a compromised
  token can leave incomplete parts billable after expiry (`AbortMultipartUpload` only helps honest CLIs).
  **FOLD (Phase 1):** add a bucket-wide `AbortIncompleteMultipartUpload` lifecycle rule (e.g. 1 day) to
  `s3.tf`.
- **C7 (Low) → S3 carve-out edges.** `--exclude "releases/*"` AND `--exclude "releases"` (cover the exact
  `releases` key the IAM Deny also names); keep `ListBucket`/`GetBucketLocation` in SEPARATE statements
  from object actions (never put the `s3:prefix` condition on `PutObject` — it would implicit-deny uploads);
  no head-object assert (use `list-objects-v2`); reword "feed deletion impossible" → "the CI roles cannot
  call `DeleteObject`" (the release role can still OVERWRITE `latest.json` with garbage/replay — F-004
  client verification is the backstop).
- **Residuals Codex added:** the site bucket has NO S3 versioning in tofu → site overwrite/delete recovery
  needs a rebuild+redeploy; consider enabling bucket versioning (materially improves recovery without
  giving CI `DeleteObjectVersion`) — ASK **A8**. The solo-account "no SCP/permission-boundary" assumption
  is unsafe (a personal account can be in an AWS Org) → preflight must inspect Org/boundary state before
  trusting `simulate-principal-policy`.
- **A7 (new):** choose the workflow-binding mechanism before implementation — `workflow` NAME (chosen
  default, simple, rename-weak) vs reusable-workflow wrapping + `job_workflow_ref` file binding (stronger,
  more refactor). **A8 (new):** enable S3 bucket versioning for site-recovery?

## Seeds (draft — finalized post-approval)
- **/goal (recommended):** All C5 phases ✓ in the consolidated plan, each backed by its validation gate
  (tofu fmt/validate, lint:tofu, lint:actions, ruleset jq assertion, the existing Lint-OpenTofu PR job
  green) reported in the transcript; the human-applies runbook (`C5-runbook.md`) written; post-impl codex
  xhigh audit folded; PR into security-hardening CI green; NOTHING applied (human applies tofu + secrets +
  ruleset).
- **/loop 15m (fallback):** drive C5 — read the consolidated plan, run tofu fmt/validate + lint:actions
  after each edit, commit, push, watch CI; consult codex xhigh on any IAM/OIDC condition-key detail; NEVER
  run tofu apply / gh secret / the ruleset API (human-only); mark phases ✓ only when their gate passes.
