The workspace is read-only, so I could not save the requested `plan.md`. Below is the completed content; no repository files were changed.

# C5 / F-005 — infra-deploy-authz implementation plan

## Outcome

Replace `aztec-accelerator-ci-github` with three pipeline roles:

| Pipeline | GitHub secret | S3 authority |
|---|---|---|
| Landing | `AWS_ROLE_ARN_LANDING` | Put/delete `landing/*`, explicit deny on `landing/releases` and `landing/releases/*` |
| Release feed | `AWS_ROLE_ARN_RELEASE` | Put only `landing/releases/latest.json` |
| Playground testnet | `AWS_ROLE_ARN_PLAYGROUND` | Put/delete `playground/*` |

All three roles require:

- `aud = sts.amazonaws.com`
- `sub = repo:alejoamiras/aztec-accelerator:ref:refs/heads/main`
- An exact `token.actions.githubusercontent.com:workflow` value matching its pipeline

AWS now exposes GitHub’s `workflow` claim as an IAM condition key. This prevents a stolen landing token from assuming the release role merely because both runs originate from main. It does not protect against malicious workflow code already merged to main, which can imitate a workflow name. [AWS IAM OIDC condition keys](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_policies_iam-condition-keys.html)

No role trusts nightlies, chore branches, pull requests, or tags.

## Corrections to the brief

Two stated facts are stale on this worktree:

- [deploy-landing.yml](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-infra-deploy-authz/.github/workflows/deploy-landing.yml:40>) currently uses `sync --delete`. Landing therefore needs `DeleteObject`, plus both a CLI exclusion and IAM deny protecting the release feed.
- [actionlint.yml](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-infra-deploy-authz/.github/workflows/actionlint.yml:92>) already runs pinned OpenTofu 1.10.0, `fmt -check`, `init -backend=false`, and `validate` on PRs to `main` and `security-hardening`. Reuse and tighten it; do not add a duplicate workflow.

The release workflow is dispatch-only. Its tag is created later inside the run, so a run dispatched with `--ref main` retains a main branch OIDC subject. No tag subject is needed. Add an early main-ref assertion so a release dispatched from another ref fails before it creates tags/releases or reaches AWS. GitHub documents branch and tag subjects as deriving from the ref that originated the workflow run. [GitHub OIDC reference](https://docs.github.com/en/actions/reference/security/oidc)

## Security invariants

- Only the release role can put `landing/releases/latest.json`.
- Landing’s `sync --delete` uses `--exclude "releases/*"`.
- IAM explicitly denies landing puts/deletes under `landing/releases`.
- Release has no `ListBucket`, `DeleteObject`, site wildcard, or playground authority.
- Playground and landing cannot mutate each other.
- Each role checks `aud`, exact main `sub`, and exact `workflow`.
- No live workflow falls back to legacy `AWS_ROLE_ARN`.
- The legacy role is deleted only after new roles, secrets, final workflow code, and smokes are in place.
- Final Terraform contains no `aws_iam_role.ci`, `aws_iam_role_policy.ci`, or `ci_role_arn`.
- All applies, secret mutations, and ruleset writes remain human-only.

## Authorization design

### Terraform resources

Add these to [iam.tf](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-infra-deploy-authz/infra/tofu/iam.tf:1>):

- `aws_iam_role.landing_deploy`
- `aws_iam_role_policy.landing_deploy`
- `aws_iam_role.release_feed`
- `aws_iam_role_policy.release_feed`
- `aws_iam_role.playground_testnet`
- `aws_iam_role_policy.playground_testnet`

Recommended AWS names:

```text
aztec-accelerator-ci-landing
aztec-accelerator-ci-release-feed
aztec-accelerator-ci-playground-testnet
```

Exact workflow claim values:

```text
Deploy Landing Page
Release Accelerator
Publish Testnet
```

Use `StringEquals`, not `StringLike`, for all trust values.

### Landing policy

Allow:

- `s3:GetBucketLocation` on the bucket
- `s3:ListBucket` with `s3:prefix` limited to `landing`, `landing/`, or `landing/*`
- `s3:PutObject` and `s3:DeleteObject` on `bucket/landing/*`
- `cloudfront:CreateInvalidation` on the site distribution

Explicitly deny `PutObject` and `DeleteObject` on:

```text
bucket/landing/releases
bucket/landing/releases/*
```

The overlapping allow-and-deny is intentional: IAM does not provide a clean negative resource wildcard, and explicit deny wins. `s3:prefix` constrains the requested `ListObjectsV2` prefix, not each returned key, so listing `landing/` may reveal release key metadata. The role still cannot read or mutate those objects. [AWS S3 prefix policy guidance](https://docs.aws.amazon.com/AmazonS3/latest/userguide/amazon-s3-policy-keys.html)

### Release-feed policy

Allow only:

- `s3:GetBucketLocation` on the bucket
- `s3:PutObject` on `bucket/landing/releases/latest.json`
- `cloudfront:CreateInvalidation` on the distribution

Do not grant `ListBucket`, `DeleteObject`, or the broader `landing/releases/*` unless a future workflow proves another object is needed.

### Playground policy

Allow:

- `s3:GetBucketLocation`
- `s3:ListBucket` with `s3:prefix` limited to `playground`, `playground/`, or `playground/*`
- `s3:PutObject` and `s3:DeleteObject` on `bucket/playground/*`
- `cloudfront:CreateInvalidation` on the distribution

### CloudFront residual

`CreateInvalidation` supports the distribution ARN but no invalidation-path condition key. IAM therefore cannot enforce `/landing/*`, `/releases/latest.json`, or `/playground/*` inside the shared distribution. [AWS CloudFront service authorization](https://docs.aws.amazon.com/service-authorization/latest/reference/list_amazoncloudfront.html)

All roles retain distribution-wide invalidation authority. A compromised role can cause cache churn, origin load, quota pressure, or invalidation cost, but cannot use invalidation to alter S3 content. True path isolation requires separate distributions or an invalidation broker and is outside F-005.

## Phase 0 — Preflight

### Work

- Verify the live OIDC subject customization.
- Verify at least squash or rebase merging is enabled; linear history otherwise cannot be enforced.
- Verify the three role names are unused.
- Record legacy role trust/policy and Terraform state.
- Check for live AWS/Terraform drift and external policies.
- Freeze manual deploys and relevant merges during operational cutover.
- Confirm no old-role workflow is in progress.

### Validation gate

```bash
tofu -chdir=infra/tofu init -backend=false -input=false
tofu -chdir=infra/tofu fmt -check -diff
tofu -chdir=infra/tofu validate
bun run lint:actions
jq -e . infra/rulesets/main-branch-protection.json >/dev/null
git diff --check
```

Stop if:

- The subject format differs from the expected legacy format.
- Neither squash nor rebase merging is enabled.
- A role name is already managed elsewhere.
- Terraform reports unrelated drift.

## Phase 1 — Additive IAM migration commit

Create a separately identifiable commit called `ADD_ROLES_COMMIT`.

### Edits

In `iam.tf`:

- Add all six new role/policy resources.
- Temporarily retain the legacy role and broad policy.
- Narrow the legacy trust list to exact main, removing nightlies and both chore wildcards. Its permissions remain broad temporarily because all current live workflows still share it.

In [outputs.tf](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-infra-deploy-authz/infra/tofu/outputs.tf:1>), add:

```text
landing_deploy_role_arn
release_feed_role_arn
playground_testnet_role_arn
```

Retain `ci_role_arn` only in this migration commit.

Update the Terraform README with the temporary dual-role state and new secrets.

### Validation gate

```bash
tofu -chdir=infra/tofu fmt -check -diff
tofu -chdir=infra/tofu init -backend=false -input=false
tofu -chdir=infra/tofu validate
bun run lint:tofu
git diff --check
```

Human plan gate: expect six IAM resources added, one legacy trust update, zero destroys, and no S3/CloudFront changes.

## Phase 2 — Workflow cutover and final Terraform state

### Landing

Edit [deploy-landing.yml](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-infra-deploy-authz/.github/workflows/deploy-landing.yml:33>):

- Use `${{ secrets.AWS_ROLE_ARN_LANDING }}`.
- Add `--exclude "releases/*"` to `sync --delete`.
- Add an early main-ref assertion for manual dispatch.

AWS documents that `--exclude` filters deletion during `s3 sync --delete`; the IAM deny remains the authoritative boundary. [AWS CLI S3 sync filters](https://docs.aws.amazon.com/cli/latest/userguide/cli-services-s3-commands.html)

### Release

Edit [release-accelerator.yml](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-infra-deploy-authz/.github/workflows/release-accelerator.yml:896>):

- Assert `GITHUB_REF == refs/heads/main` in the initial validation job.
- Use `${{ secrets.AWS_ROLE_ARN_RELEASE }}`.
- Move stable-release AWS credential configuration before `gh release create`, so broken trust/secret wiring fails before publishing the GitHub release.
- Keep the exact S3 key and viewer invalidation path.

### Playground

Edit [publish-testnet.yml](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-infra-deploy-authz/.github/workflows/publish-testnet.yml:80>):

- Use `${{ secrets.AWS_ROLE_ARN_PLAYGROUND }}`.
- Scope AWS `id-token: write` to `deploy-app` instead of unrelated jobs.
- Preserve the reusable SDK publisher’s Sigstore provenance permissions.
- Add/propagate a main-ref validation dependency to side-effecting jobs.

Leave `_publish-sdk.yml` and `publish-nightlies.yml` untouched.

### Final Terraform state

- Remove the legacy role, policy, and output.
- Keep the new roles identical to their already-applied Phase 1 definitions.
- Update the README to list only the three new secrets and outputs.

### Existing CI gate

In `actionlint.yml`:

- Expand the tofu change filter to all `infra/tofu/**`, including `.terraform.lock.hcl`.
- Include the validation workflow itself if its OpenTofu setup changes.
- Preserve the pinned setup and `fmt → init -backend=false → validate` sequence.

### Validation gate

```bash
tofu -chdir=infra/tofu fmt -check -diff
tofu -chdir=infra/tofu init -backend=false -input=false
tofu -chdir=infra/tofu validate
bun run lint:tofu
bun run lint:actions
git diff --check
```

Review checks:

```bash
rg -n 'secrets\.AWS_ROLE_ARN([^_]|$)' .github/workflows
rg -n 'nightlies|chore/aztec' infra/tofu/iam.tf
rg -n 'landing/releases' \
  infra/tofu/iam.tf \
  .github/workflows/deploy-landing.yml \
  .github/workflows/release-accelerator.yml
```

Only the intentionally dormant nightly workflow may retain the old secret reference. No live workflow may contain a legacy fallback.

## Phase 3 — Main ruleset

Edit [main-branch-protection.json](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-infra-deploy-authz/infra/rulesets/main-branch-protection.json:1>):

- Keep `refs/heads/main` as the only target.
- Keep `bypass_actors: []`.
- Keep zero approvals.
- Keep the three existing checks and integration ID `15368`.
- Set `required_review_thread_resolution: true`.
- Add `required_linear_history`.
- Add `non_fast_forward`.
- Add `deletion`.
- Do not add signatures, nightlies protection, bypass actors, or one approval.

GitHub documents the rule names and requires squash or rebase merging before linear history can be enforced. [Available rules for rulesets](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets)

### Validation gate

```bash
jq -e '
  .name == "Main branch protection" and
  .target == "branch" and
  .enforcement == "active" and
  .bypass_actors == [] and
  .conditions.ref_name.include == ["refs/heads/main"] and
  .conditions.ref_name.exclude == [] and
  ([.rules[].type] | sort) ==
    ["deletion","non_fast_forward","pull_request",
     "required_linear_history","required_status_checks"] and
  ([.rules[] | select(.type == "pull_request")][0].parameters |
    .required_approving_review_count == 0 and
    .required_review_thread_resolution == true)
' infra/rulesets/main-branch-protection.json >/dev/null

bun run lint:actions
git diff --check
```

The JSON is only desired state; this gate does not change live GitHub configuration.

## Phase 4 — Final source validation

```bash
tofu -chdir=infra/tofu fmt -check -diff
tofu -chdir=infra/tofu init -backend=false -input=false
tofu -chdir=infra/tofu validate
bun run lint:tofu
bun run lint:actions
jq -e . infra/rulesets/main-branch-protection.json >/dev/null
git diff --check
```

Inspect the migration commits separately:

- Phase 1 must be deploy-compatible and additive.
- The final commit must use only new secrets.
- Final Terraform must contain no legacy role.
- Neither plan may recreate or replace the bucket/distribution.

## Phase 5 — Human-applies runbook

Operational retirement must wait until the final workflow code has reached `main`. Merging C5 only into `security-hardening` is not sufficient: live main workflows would still reference the old secret.

### 5.1 Preconditions

```bash
set -euo pipefail

export REPO='alejoamiras/aztec-accelerator'
export RULESET_NAME='Main branch protection'
export GH_API_VERSION='2026-03-10'

aws sts get-caller-identity
gh auth status

gh api "repos/$REPO" \
  --jq '{default_branch,allow_squash_merge,allow_rebase_merge,allow_merge_commit}'

gh api "repos/$REPO/actions/oidc/customization/sub" || true

gh run list --repo "$REPO" --workflow deploy-landing.yml --limit 10
gh run list --repo "$REPO" --workflow publish-testnet.yml --limit 10
gh run list --repo "$REPO" --workflow release-accelerator.yml --limit 10

aws s3api head-object \
  --bucket aztec-accelerator-site \
  --key landing/releases/latest.json \
  > /tmp/latest-before.json

curl -fsS https://aztec-accelerator.dev/releases/latest.json \
  | sha256sum > /tmp/latest-public-before.sha256
```

Require `main`, squash or rebase enabled, expected OIDC subject format, and no in-progress old-role deployment.

### 5.2 Back up the live ruleset

```bash
gh api -H "X-GitHub-Api-Version: $GH_API_VERSION" \
  "repos/$REPO/rulesets" > /tmp/rulesets.json

RULESET_COUNT="$(jq --arg name "$RULESET_NAME" \
  '[.[] | select(.name == $name and .source_type == "Repository")] | length' \
  /tmp/rulesets.json)"

if [ "$RULESET_COUNT" -eq 1 ]; then
  RULESET_ID="$(jq -r --arg name "$RULESET_NAME" \
    '.[] | select(.name == $name and .source_type == "Repository") | .id' \
    /tmp/rulesets.json)"

  gh api -H "X-GitHub-Api-Version: $GH_API_VERSION" \
    "repos/$REPO/rulesets/$RULESET_ID" \
    | jq '{name,target,enforcement,bypass_actors,conditions,rules}' \
    > /tmp/main-ruleset.before.json
elif [ "$RULESET_COUNT" -eq 0 ]; then
  unset RULESET_ID
else
  echo "Expected zero or one repository ruleset named '$RULESET_NAME'" >&2
  exit 1
fi
```

The update endpoint requires repository Administration write permission. [GitHub repository ruleset API](https://docs.github.com/en/rest/repos/rules)

### 5.3 Apply the additive commit

```bash
git checkout "$ADD_ROLES_COMMIT"

tofu -chdir=infra/tofu init -input=false
tofu -chdir=infra/tofu fmt -check -diff
tofu -chdir=infra/tofu validate
tofu -chdir=infra/tofu plan \
  -input=false \
  -out=/tmp/c5-add-roles.tfplan

tofu -chdir=infra/tofu show -no-color /tmp/c5-add-roles.tfplan
```

Only after confirming six adds, one trust update, zero destroys, and no bucket/distribution changes:

```bash
tofu -chdir=infra/tofu apply /tmp/c5-add-roles.tfplan
```

### 5.4 Wire secrets from live AWS ARNs

```bash
LANDING_ARN="$(aws iam get-role \
  --role-name aztec-accelerator-ci-landing \
  --query 'Role.Arn' --output text)"

RELEASE_ARN="$(aws iam get-role \
  --role-name aztec-accelerator-ci-release-feed \
  --query 'Role.Arn' --output text)"

PLAYGROUND_ARN="$(aws iam get-role \
  --role-name aztec-accelerator-ci-playground-testnet \
  --query 'Role.Arn' --output text)"

gh secret set AWS_ROLE_ARN_LANDING \
  --repo "$REPO" --body "$LANDING_ARN"
gh secret set AWS_ROLE_ARN_RELEASE \
  --repo "$REPO" --body "$RELEASE_ARN"
gh secret set AWS_ROLE_ARN_PLAYGROUND \
  --repo "$REPO" --body "$PLAYGROUND_ARN"

gh secret list --repo "$REPO" \
  | rg 'AWS_ROLE_ARN_(LANDING|RELEASE|PLAYGROUND)'
```

GitHub cannot read secret values back. Querying the ARN directly and successfully assuming it from the workflow are the verification.

### 5.5 Simulate permissions

At minimum prove:

| Decision | Expected |
|---|---|
| Landing put/delete `landing/index.html` | allowed |
| Landing put/delete `landing/releases/latest.json` | explicit deny |
| Landing put/delete playground | implicit deny |
| Release put exact `latest.json` | allowed |
| Release delete exact `latest.json` | implicit deny |
| Release put another landing/playground object | implicit deny |
| Playground put/delete playground | allowed |
| Playground put/delete landing | implicit deny |
| Each role invalidates the distribution | allowed |

Example:

```bash
aws iam simulate-principal-policy \
  --policy-source-arn "$LANDING_ARN" \
  --action-names s3:PutObject s3:DeleteObject \
  --resource-arns \
    arn:aws:s3:::aztec-accelerator-site/landing/index.html \
    arn:aws:s3:::aztec-accelerator-site/landing/releases/latest.json \
    arn:aws:s3:::aztec-accelerator-site/playground/index.html \
  --query \
    'EvaluationResults[].{Action:EvalActionName,Resource:EvalResourceName,Decision:EvalDecision}' \
  --output table
```

Read each role’s trust and confirm exact `aud`, `sub`, and `workflow`. Abort on wildcard trust or unexpected policies.

### 5.6 Land final source and smoke

After the final C5 code is on `main`:

```bash
gh workflow run deploy-landing.yml --repo "$REPO" --ref main
gh run list --repo "$REPO" --workflow deploy-landing.yml --limit 1

gh workflow run publish-testnet.yml \
  --repo "$REPO" \
  --ref main \
  -f skip_sdk_publish=true

gh run list --repo "$REPO" --workflow publish-testnet.yml --limit 1
```

Watch returned IDs:

```bash
gh run watch RUN_ID --repo "$REPO" --exit-status
```

Then prove the landing sync did not alter the feed:

```bash
aws s3api head-object \
  --bucket aztec-accelerator-site \
  --key landing/releases/latest.json \
  > /tmp/latest-after-landing.json

curl -fsS https://aztec-accelerator.dev/releases/latest.json \
  | sha256sum > /tmp/latest-public-after.sha256

diff -u \
  /tmp/latest-public-before.sha256 \
  /tmp/latest-public-after.sha256
```

Do not manufacture a release merely to test IAM. The release policy is simulated, its ARN comes directly from AWS, and the credential step is moved before `gh release create` so the next legitimate release fails early if wiring is wrong.

Before legacy deletion, rollback is reversible: temporarily point only the affected new secret at the still-live legacy ARN, diagnose, and rerun. Do not add fallback logic to source.

### 5.7 Retire the legacy role

From final source:

```bash
tofu -chdir=infra/tofu init -input=false
tofu -chdir=infra/tofu fmt -check -diff
tofu -chdir=infra/tofu validate
tofu -chdir=infra/tofu plan \
  -input=false \
  -out=/tmp/c5-retire-legacy.tfplan

tofu -chdir=infra/tofu show -no-color \
  /tmp/c5-retire-legacy.tfplan
```

Gate: only the legacy inline policy and role are destroyed. New roles remain unchanged.

```bash
tofu -chdir=infra/tofu apply /tmp/c5-retire-legacy.tfplan

if aws iam get-role --role-name aztec-accelerator-ci-github; then
  echo 'Legacy role still exists' >&2
  exit 1
fi

gh secret delete AWS_ROLE_ARN --repo "$REPO"
```

IAM evaluates role permissions on each request, so removing the broad policy disables previously issued sessions as the change propagates. Complete or cancel old workflows first. If compromise is suspected, explicitly revoke old sessions before deletion. [AWS role-session revocation](https://docs.aws.amazon.com/IAM/latest/UserGuide/id_roles_use_revoke-sessions.html)

After deletion, fix individual roles for rollback; do not routinely recreate the broad role.

### 5.8 Apply and verify the ruleset

```bash
if [ "${RULESET_ID+x}" = x ]; then
  gh api --method PUT \
    -H "X-GitHub-Api-Version: $GH_API_VERSION" \
    "repos/$REPO/rulesets/$RULESET_ID" \
    --input infra/rulesets/main-branch-protection.json \
    > /tmp/main-ruleset.applied.json
else
  gh api --method POST \
    -H "X-GitHub-Api-Version: $GH_API_VERSION" \
    "repos/$REPO/rulesets" \
    --input infra/rulesets/main-branch-protection.json \
    > /tmp/main-ruleset.applied.json

  RULESET_ID="$(jq -r '.id' /tmp/main-ruleset.applied.json)"
fi

gh api -H "X-GitHub-Api-Version: $GH_API_VERSION" \
  "repos/$REPO/rulesets/$RULESET_ID" \
  > /tmp/main-ruleset.live.json
```

Assert the live configuration:

```bash
jq -e '
  .name == "Main branch protection" and
  .target == "branch" and
  .enforcement == "active" and
  .bypass_actors == [] and
  .conditions.ref_name.include == ["refs/heads/main"] and
  ([.rules[].type] | sort) ==
    ["deletion","non_fast_forward","pull_request",
     "required_linear_history","required_status_checks"] and
  ([.rules[] | select(.type == "pull_request")][0].parameters |
    .required_approving_review_count == 0 and
    .required_review_thread_resolution == true) and
  ([.rules[] | select(.type == "required_status_checks")]
    [0].parameters.required_status_checks |
    map({context,integration_id})) == [
      {"context":"SDK Status","integration_id":15368},
      {"context":"App Status","integration_id":15368},
      {"context":"Accelerator Status","integration_id":15368}
    ]
' /tmp/main-ruleset.live.json >/dev/null
```

If necessary, restore the sanitized backup:

```bash
gh api --method PUT \
  -H "X-GitHub-Api-Version: $GH_API_VERSION" \
  "repos/$REPO/rulesets/$RULESET_ID" \
  --input /tmp/main-ruleset.before.json
```

Do not test force-push or deletion against `main`.

## Ordering safety

AWS IAM, GitHub secrets, and workflow source cannot be updated transactionally. A strict promise of both zero overlap and zero downtime is therefore impossible with the current shared secret.

The safe sequence is:

```text
add roles
→ set three secrets
→ land workflow cutover on main
→ smoke new roles
→ destroy old policy/role
→ delete old secret
→ unfreeze
```

There is no point where a live workflow references a deleted role. The old broad role remains reachable only during the frozen, time-boxed migration window.

Deleting it earlier produces a predictable `AssumeRoleWithWebIdentity` failure. Cutting workflow code over before roles/secrets exist produces the same failure.

## Security and adversarial considerations

### Why three roles instead of two?

A `feed` versus `sites` split protects `latest.json`, but a compromised site token could still cross-deface both landing and playground. The pipelines also have distinct deletion behavior. Three roles are the minimum useful granularity aligned with separate public damage domains.

Because all roles have the same main `sub`, exact workflow conditions are important. Without them, any main OIDC token could attempt every role and secret separation would be convention rather than authorization.

### Residual capabilities

- Landing compromise: landing defacement/phishing or DoS, excluding releases; distribution-wide invalidation.
- Playground compromise: playground defacement/phishing or DoS; distribution-wide invalidation.
- Release-token compromise: overwrite `latest.json`, replay/corrupt the feed, and cause update-feed DoS. F-004 should reject unsigned or rollback content when the signing key is not also compromised.
- Release-workflow/main compromise: may reach signing and release secrets as well as the feed role, exceeding the stolen-OIDC-token case.
- Owner/admin compromise: can change workflow names, rulesets, secrets, or repository state. There is no second human authority.
- Dormant nightlies: loses AWS access after legacy deletion but retains any unrelated GitHub/npm capability already present. This plan does not claim it is disabled.
- Any deploy role: invalidate arbitrary distribution paths because CloudFront cannot enforce path scope.

### Solo-owner ruleset limitations

Zero approvals avoids self-approval deadlock and preserves automated version-bump PRs, but provides no independent reviewer.

Conversation resolution only helps when conversations exist. Linear history improves auditability but does not make a malicious squash safe. Empty bypass actors bind normal admin actions, but an administrator can still edit/delete the ruleset.

The three mandated required checks do not include `Actionlint Status` or the OpenTofu validator. Infra validation is therefore visible on PRs but not a required main merge gate under this JSON. That is a material residual in a zero-review repository.

## Assumptions

### Facts

- Landing and testnet use `sync --delete`.
- Release puts one exact feed object.
- Release has no tag trigger.
- OpenTofu PR validation already exists.
- The bucket policy grants CloudFront read only.
- The committed ruleset targets only main with zero approvals and three required checks.

### Inferences requiring validation

- The repository still uses the pre-immutable OIDC subject format.
- Operators dispatch live workflows from main.
- The live state has no IAM/S3/CloudFront drift.
- No external AWS policy broadens these roles.
- Squash or rebase merging is enabled.

### Asks/blockers

- Confirm the live OIDC customization/immutable-subject state.
- Confirm effective organization/classic protection does not conflict with this repository ruleset.
- Confirm no SCP, permissions boundary, bucket policy, or unmanaged role policy changes the simulated decisions.
- Confirm the brief deploy/merge freeze is acceptable.
- Confirm distribution-wide invalidation authority is accepted.
- Confirm the owner accepts that OpenTofu validation remains optional under the fixed three-check ruleset.

Baseline checks performed on the untouched worktree: `bun run lint:actions`, ruleset JSON parsing, and `git diff --check` passed. OpenTofu is not installed in this local read-only environment; the existing PR workflow installs and runs it.