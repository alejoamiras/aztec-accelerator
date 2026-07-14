# C5 / F-005 — infra-deploy-authz — MAIN plan (deep leg 1 of 3)

## Summary
Split the single over-broad GitHub-Actions→AWS deploy role into **per-pipeline, prefix-scoped roles**,
each trusted to only ITS workflow file on `main`, so no pipeline (nor a compromised main-branch workflow)
can overwrite another prefix — critically, only the release pipeline may write the F-004-critical
`landing/releases/latest.json`. Narrow OIDC to `main`, drop the unused `nightlies` + `chore/aztec-*`
subjects, and harden `main` branch protection (solo-repo-appropriate). **Commit + validate only; a human
applies `tofu apply` + the ruleset API** via a precise, deploy-safe cutover runbook.

## Threat model (recap)
Single-owner public repo. Reachable damage = overwrite the update feed (→ F-004 rollback), feed DoS,
landing/playground defacement/phishing. NOT remote RCE (minisign still blocks arbitrary-code installs).
"Attacker" = a compromised owner/CI token or an owner mistake, plus a malicious PR that lands a workflow
edit on `main`.

## Key design decision — bind roles to the WORKFLOW, not just the ref
All live deploy workflows run on `main` (dispatch or push-to-main), so `sub = refs/heads/main` ALONE does
NOT isolate pipelines from each other: any workflow on main could assume any role. Therefore each role's
assume-role trust conditions on BOTH:
- `token.actions.githubusercontent.com:sub` StringLike `repo:alejoamiras/aztec-accelerator:ref:refs/heads/main`, AND
- `token.actions.githubusercontent.com:job_workflow_ref` StringLike
  `repo:alejoamiras/aztec-accelerator/.github/workflows/<file>.yml@refs/heads/main`
so ONLY that specific workflow file (running on main) can assume the role. Verified: each S3-writing job
is top-level in its own file (`deploy-landing.yml:33`, `release-accelerator.yml:899`,
`publish-testnet.yml:80`), so `job_workflow_ref` resolves to that file.

## The three live roles (nightlies dropped)
| Role | Trusts (job_workflow_ref @ main) | S3 write | CloudFront |
|---|---|---|---|
| `…-ci-landing`   | deploy-landing.yml     | `landing/*` **Deny** `landing/releases/*` | invalidate (distribution-wide*) |
| `…-ci-release`   | release-accelerator.yml| `landing/releases/*` ONLY                 | invalidate (distribution-wide*) |
| `…-ci-playground`| publish-testnet.yml    | `playground/*` ONLY                       | invalidate (distribution-wide*) |

\* CloudFront `CreateInvalidation` has NO IAM condition key for invalidation paths → it is
distribution-wide per role. Accepted residual: invalidation only busts CDN cache (self-healing re-fetch
from S3); it cannot WRITE content. Documented, not fixable in IAM.

S3 IAM expresses "landing/* EXCEPT landing/releases/*" via an explicit **Deny** on
`arn:…:landing/releases/*` in the landing role (Deny beats Allow) + Allow `landing/*`; the release role
Allows only `landing/releases/*`. `ListBucket` is granted with an `s3:prefix` condition per role (needed
for `aws s3 sync`'s diff) so listing is also prefix-scoped.

## Phases

### Phase 1 — Split IAM into per-pipeline roles (`infra/tofu/iam.tf`)
- Replace the single `aws_iam_role.ci` + `aws_iam_role_policy.ci` with three role+policy pairs
  (`ci_landing`, `ci_release`, `ci_playground`) as above. Keep the one `aws_iam_openid_connect_provider.github`.
- Each assume-role policy: `aud` StringEquals + `sub` StringLike main + `job_workflow_ref` StringLike the file.
- Each inline policy: prefix-scoped `PutObject`/`DeleteObject` (+ Deny for landing), prefix-conditioned
  `ListBucket` + `GetBucketLocation`, and `cloudfront:CreateInvalidation` on the distribution.
- Emit `outputs.tf` values: `ci_landing_role_arn`, `ci_release_role_arn`, `ci_playground_role_arn`
  (so the human copies them into the new GH secrets).
- **Validation gate:** `cd infra/tofu && tofu fmt -check -diff . && tofu validate` → exit 0 (init with
  `-backend=false` if needed for validate without creds). Layers: infra-lint + tofu-validate.

### Phase 2 — Point each workflow at its own role secret
- `deploy-landing.yml`: `role-to-assume: ${{ secrets.AWS_ROLE_ARN_LANDING }}`.
- `release-accelerator.yml` (release job, ~L899): `${{ secrets.AWS_ROLE_ARN_RELEASE }}`.
- `publish-testnet.yml`: `${{ secrets.AWS_ROLE_ARN_PLAYGROUND }}`.
- Leave `publish-nightlies.yml` untouched (unused).
- **Validation gate:** `bun run lint:actions` → exit 0. Layers: actionlint.

### Phase 3 — Harden `main` branch protection (`infra/rulesets/main-branch-protection.json`)
- Keep the 3 required Status checks + `pull_request` rule with `required_approving_review_count: 0`
  (solo repo — GitHub blocks self-approval).
- ADD rule types: `required_linear_history`; `non_fast_forward` (block force-push); `deletion` (block
  branch deletion); set `required_review_thread_resolution: true`.
- **Validation gate:** JSON parses (`jq . infra/rulesets/main-branch-protection.json`) + the file is a
  valid GitHub ruleset shape (documented; API-applied by human). Layers: json-lint.

### Phase 4 — Add a tofu-validate PR gate (`.github/workflows/infra.yml`)
- New PR workflow (per repo CI conventions): a `changes` paths-filter on `infra/**`, then a job running
  `tofu fmt -check -diff` + `tofu init -backend=false` + `tofu validate`. `contents: read` only, NO AWS
  creds, NO `tofu plan`/`apply`. Add it to the `pull_request.branches: [main, security-hardening]` list.
- **Validation gate:** `bun run lint:actions` → exit 0; the new job runs green on this very PR. Layers:
  actionlint + the new tofu-validate job in CI.

### Phase 5 — Human-applies runbook (`implementations-plan/.../clusters/C5-runbook.md`)
Document (do NOT execute) the exact deploy-SAFE cutover. Ordering avoids both a broken-deploy window and
prolonged co-existence of the old over-broad role:
1. Merge C5 PR into `security-hardening` (workflows now reference the new secret names; but deploys only
   fire on dispatch/push — nothing breaks yet).
2. Human: `cd infra/tofu && tofu plan` (review: 3 new roles ADDED, old `ci` role still present) → `tofu apply`.
3. Human: read `tofu output`; create GH repo secrets `AWS_ROLE_ARN_LANDING/_RELEASE/_PLAYGROUND` from the
   three new ARNs (`gh secret set …`).
4. Human: smoke each pipeline (dispatch deploy-landing, publish-testnet; do a release dry-run or trust the
   next real release) → confirm each new role works.
5. Human: remove the old `ci` role from `iam.tf` (second commit/PR or same, applied after step 4) →
   `tofu apply`; delete the now-unused `AWS_ROLE_ARN` secret.
6. Human: apply the ruleset — `gh api repos/alejoamiras/aztec-accelerator/rulesets/<id> -X PUT --input infra/rulesets/main-branch-protection.json`.
Reversibility: each step is a discrete `tofu apply` / secret op; roll back by re-adding the old role +
secret. The old role is deleted ONLY after the new roles are proven (step 4), so there is no broken window.

## Security & Adversarial Considerations
- **Threat model / least privilege:** per-pipeline + `job_workflow_ref` binding is the core control; a
  compromised non-release workflow on main can no longer assume the release role.
- **Residual — CloudFront invalidation is distribution-wide** (no path IAM condition). Accept: cache-bust
  only, no content write.
- **Residual — a malicious edit to release-accelerator.yml on main** would still run as the release role
  (job_workflow_ref matches the file). Mitigated by main protection (PR + required checks + linear
  history) and the F-004 in-app manifest verification (a rolled-back/forged feed is rejected client-side).
- **Cutover footgun:** setting the new secrets / applying tofu out of order breaks the next deploy — the
  runbook forces the safe order and keeps the old role until the new ones are proven.
- **No secrets created by CI**; OIDC only. No static AWS keys introduced. `contents: read` default on the
  new infra workflow.
- **Supply chain:** the new infra workflow SHA-pins actions (checkout + a pinned `opentofu/setup-opentofu`)
  per the C3 convention.

## Assumptions
### Facts (verified)
- `iam.tf` single role + whole-bucket policy + 4-entry OIDC sub (read this file).
- 3 live deploy workflows + their prefixes + top-level S3 jobs (read the workflows).
- `main-branch-protection.json` = 0 approvals + 3 checks, no linear/force-push rules (read it).
- `lint:tofu` exists; no tofu validate/plan in CI; no AWS creds on PRs (read package.json + workflows).
- Each S3-writing job is top-level in its own file → `job_workflow_ref` binds per pipeline (verified).

### Inferences (unverified — attack these)
- All 3 pipelines are dispatched from / run on `main` (so `sub=main` suffices as the ref condition).
  release-accelerator is workflow_dispatch-only and creates a tag as a side-effect; the OIDC token's ref
  is the dispatch branch (main), NOT the tag — VERIFY this holds (if a future `on: push tags` is added,
  the release role's sub must include the tag ref).
- GitHub OIDC exposes `job_workflow_ref` as a conditionable claim in the assume-role policy with the
  `repo/.github/workflows/FILE@REF` shape — VERIFY exact claim name + value format against AWS+GitHub docs.
- `aws s3 sync` needs only `ListBucket` (prefix-conditioned) + `PutObject`/`DeleteObject`; a prefixed
  `ListBucket` does not break the sync diff — VERIFY.
- The ruleset is applied via `gh api …/rulesets/<id> -X PUT` (org/repo ruleset, not classic branch
  protection) — VERIFY the current apply mechanism the repo uses.

### Asks (surface to user — already answered in the brief, restated for the gate)
- Role granularity: per-pipeline (3 roles) chosen over 2-role (feed vs sites). [answered: per-pipeline]
- OIDC refs: main only, nightlies + chore/aztec-* dropped. [answered]
- main protection: checks+linear+no-force+0-approvals. [answered]
- CI: tofu fmt+validate, no plan/apply. [answered]

## Seeds (draft)
- `/goal`: All C5 phases ✓ in plan.md, each backed by its validation gate (tofu fmt/validate,
  lint:actions, the new infra CI job green) reported in the transcript; the human-applies runbook written;
  post-impl codex xhigh audit folded; PR into security-hardening CI green; NOT applied (human applies).
- `/loop 15m`: drive C5 — read plan.md, run tofu fmt/validate + lint:actions after each edit, commit,
  push, watch CI; consult codex xhigh on any IAM/OIDC decision; never run tofu apply or the ruleset API
  (human-only); mark phases ✓ only when their gate passes.
