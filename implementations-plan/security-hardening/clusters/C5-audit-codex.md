The plan is not safe to implement as written. The central OIDC binding is invalid for these jobs, and the cutover silently retires a still-dispatchable deployment workflow.

## Critical

1. D1 will make all three new roles unassumable.

The chosen condition in [C5-CONSOLIDATED.md:37](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-infra-deploy-authz/implementations-plan/security-hardening/clusters/C5-CONSOLIDATED.md:37) is wrong in two independent ways:

- AWS now accepts `token.actions.githubusercontent.com:job_workflow_ref`, but GitHub emits `job_workflow_ref` only for jobs executing inside a reusable workflow. These three AWS jobs are top-level. `workflow_ref` is present for top-level jobs, but AWS does not expose it in its supported GitHub condition-key mapping. [GitHub OIDC claims](https://docs.github.com/en/actions/reference/security/oidc), [AWS GitHub condition keys](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_policies_iam-condition-keys.html)
- The proposed value incorrectly begins with `repo:`. The AWS-documented value has no prefix:
  `alejoamiras/aztec-accelerator/.github/workflows/FILE.yml@refs/heads/main`.

Therefore this is false:

> “for these top-level S3 jobs they coincide”

Concrete failure: after workflow cutover, `configure-aws-credentials` presents a token without `job_workflow_ref`; every `AssumeRoleWithWebIdentity` fails.

Smallest fixes:

- Minimal, without restructuring: use `StringEquals` on the supported `workflow` claim with exact names:

  - `Deploy Landing Page`
  - `Release Accelerator`
  - `Publish Testnet`

  Keep exact `aud` and `sub`. No wildcard is needed.

- Strong exact-file binding: make each credential-bearing job run through a reusable workflow, then use:

  `token.actions.githubusercontent.com:job_workflow_ref =
  alejoamiras/aztec-accelerator/.github/workflows/<called-file>.yml@refs/heads/main`

For clarity, the exact subject remains:

`repo:alejoamiras/aztec-accelerator:ref:refs/heads/main`

Only `sub` has the `repo:` prefix.

## High

2. The cutover breaks `publish-nightlies`, potentially after an irreversible npm publish.

The plan knowingly leaves the old secret reference in [publish-nightlies.yml:38](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-infra-deploy-authz/.github/workflows/publish-nightlies.yml:38) and [publish-nightlies.yml:71](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-infra-deploy-authz/.github/workflows/publish-nightlies.yml:71), then deletes both the old role and `AWS_ROLE_ARN`.

“Nightlies branch unused” is insufficient: `publish-nightlies.yml` is `workflow_dispatch` and can be dispatched on `main`; manual workflows allow the operator to select a ref. [GitHub manual workflow documentation](https://docs.github.com/en/enterprise-cloud%40latest/actions/how-tos/manage-workflow-runs/manually-run-a-workflow)

Concrete failure: `publish-sdk` can successfully publish to npm while `deploy-app`, running independently after E2E, fails to assume the deleted role. That is a partially successful, irreversible run.

Smallest fix: surface a mandatory retain-or-retire decision.

- Retain: add a fourth workflow-specific role and `AWS_ROLE_ARN_NIGHTLIES`, scoped to `playground-nightly/*`.
- Retire: remove/disable the dispatch workflow explicitly and document the nightly site/npm lifecycle. Do not retire it accidentally by deleting its secret.

Three roles are correct only after nightlies is explicitly retired; otherwise four is the correct granularity.

3. Release authentication still occurs after the first side effect, and the runbook has no safe trust smoke.

The plan moves AWS authentication before `gh release create`, but [release-accelerator.yml:555](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-infra-deploy-authz/.github/workflows/release-accelerator.yml:555) pushes the Git tag before the `release` job begins.

Concrete scenario: the first stable release after cutover builds for an hour, pushes `accelerator-vN`, then AWS authentication fails because of D1 or a bad secret. The repository is left with a release tag but no GitHub release or feed.

`simulate-principal-policy` does not test the trust policy, OIDC claims, secret wiring, resource policies, or a real AWS request. AWS explicitly recommends live verification after simulation. [AWS policy simulator limitations](https://docs.aws.amazon.com/IAM/latest/UserGuide/access_policies_testing-policies.html)

Smallest fix:

- Add a stable-release AWS preflight to the early `validate` job: job-scoped `id-token: write`, configure the release role, then call allowed `GetBucketLocation`.
- Ensure every tag/release/publish side effect depends on that validation.
- Add a safe `auth_probe` dispatch mode, or equivalent, so the human can test the exact release workflow’s OIDC token before deleting the legacy role.

## Medium

4. Phase 3 is operationally unsafe if its deletion commit reaches `main` before smoke tests.

[C5-CONSOLIDATED.md:106](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-infra-deploy-authz/implementations-plan/security-hardening/clusters/C5-CONSOLIDATED.md:106) permits the deletion source commit to be prepared alongside campaign work while relying on the human to apply a historical additive commit first. Once final `main` contains the deletion, an ordinary `tofu apply` from current source removes the fallback role before smoke completion.

Smallest fix: keep Phase 3 out of the final campaign integration and merge it as a separate post-smoke PR. That makes repository source, live state, and intended ordering agree.

The “no in-flight runs” check must include queued and running jobs for all four old-role consumers. Removing the role policy or role affects credentials already issued, not merely future assumptions. [AWS temporary-credential behavior](https://docs.aws.amazon.com/IAM/latest/UserGuide/id_credentials_temp_control-access_disable-perms.html)

5. The ruleset shape is valid, but the proposed protection is weaker than its security narrative.

The proposed rule types and fields are valid:

- `deletion`
- `non_fast_forward`
- `required_linear_history`
- `pull_request.parameters.allowed_merge_methods: ["squash","rebase"]`
- the existing status-check objects and integration IDs

GitHub requires at least one of squash/rebase to be enabled for linear history, as the plan notes. [GitHub ruleset API schema](https://docs.github.com/en/rest/repos/rules), [available rules](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets)

Other conclusions:

- It targets only `main`, so it will not block C5 PRs into `security-hardening`.
- `bypass_actors: []` does not permanently lock out the owner; an administrator can edit/disable the ruleset, but has no configured merge bypass.
- Zero approvals is operationally defensible for a genuinely solo repository, but it provides no human-review security boundary. It must not be cited as meaningful mitigation for malicious code merged by a compromised owner.
- Leaving `strict_required_status_checks_policy: false` allows stale-green commits to merge.

Smallest fix:

- Resolve A1 as yes: require `Actionlint Status`. Its workflow even claims at [actionlint.yml:111](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-infra-deploy-authz/.github/workflows/actionlint.yml:111) that it is required, while the JSON currently omits it.
- Set `strict_required_status_checks_policy: true`.
- Apply the backed-up, validated ruleset before merging the final PR into `main`, so it protects the cutover itself rather than only subsequent changes.

6. Multipart-upload cost exposure remains unbounded.

`PutObject` authorizes multipart initiation and completion; `AbortMultipartUpload` helps an honest CLI clean up failures but does not stop a compromised token from deliberately leaving uploaded parts incomplete. The site bucket has no managed abort lifecycle.

Concrete scenario: a compromised deploy job repeatedly creates multipart uploads and uploads large parts without completing them. The parts remain billable after the token expires.

Smallest fix: add a bucket-wide `AbortIncompleteMultipartUpload` lifecycle rule, ideally one day or another explicitly accepted retention period. AWS recommends this as a storage-cost control. [AWS S3 lifecycle guidance](https://docs.aws.amazon.com/AmazonS3/latest/userguide/mpu-abort-incomplete-mpu-lifecycle-config.html)

## Low

7. The S3 carve-out works for the normal feed, but two edge assertions are wrong or incomplete.

The important path is sound:

- Allowing `landing/*`, explicitly denying release objects, and adding `--exclude "releases/*"` protects `latest.json`.
- AWS documents that excluded sync paths are also excluded from `--delete`. [AWS CLI `sync`](https://docs.aws.amazon.com/cli/latest/reference/s3/sync.html)
- The release workflow’s only S3 write is the exact `landing/releases/latest.json`, so exact-object scope is correct.

Corrections:

- Because IAM also denies the exact object `landing/releases`, exclude both `releases` and `releases/*`; otherwise that unusual key would make sync return nonzero.
- Put bucket-level `ListBucket`/`GetBucketLocation` in separate statements from object actions. Use `s3:prefix` values such as `landing/` and `landing/*`; applying that condition to `PutObject` would make uploads implicitly denied.
- The optional post-sync `HeadObject` assertion will fail because the landing role lacks `s3:GetObject`. Omit it, use a suitably prefix-conditioned list check, or leave the S3 check to the human/admin runbook.
- “Feed deletion is impossible” should say “the CI roles cannot call `DeleteObject`.” The release role can still overwrite the key with empty, invalid, or replayed signed content, causing update denial or limited rollback effects.

## Assumptions audit

### Facts

Misstated:

- “Top-level job means `job_workflow_ref` binds the file” — false.
- “Dispatch-only means release OIDC sub stays main” — false until the proposed main-ref assertion is added; a dispatch ref is selectable.
- “Only landing and playground prefixes / nightlies unused” — incomplete. The repository contains a dispatchable `playground-nightly/` publisher and the remote `nightlies` branch.
- “Three roles are the minimum” — only after explicit nightlies retirement.

Accurate:

- Landing currently uses `sync --delete`, so the latent feed deletion is real.
- Release performs one exact S3 upload.
- The OpenTofu validation job already exists.
- The existing ruleset has zero approvals and three required checks.

### Inferences

- The nested `releases/*` exclusion is documented and safe; the exact `releases` key remains uncovered.
- The live custom OIDC `sub` configuration must still be read before applying trust changes.
- “No SCP or boundary because this is a solo account” is unsafe. A personal account can belong to an AWS Organization or use boundaries. Inspect the new roles and organization state, then perform real OIDC probes.
- `simulate-principal-policy` is supporting evidence, not a runtime authorization proof.
- Squash/rebase availability is appropriately a mandatory preflight.

### Asks

These cannot remain assumed:

- A1: resolve yes; require `Actionlint Status`.
- A2: environments are optional defense-in-depth, but they do not substitute for valid file/name binding and another workflow merged to `main` can declare the same environment.
- A3: split it. Hardware-key adoption is an owner choice; tfstate ownership, access policy, and versioning are mandatory preflight facts.
- A4: obtain explicit approval for the freeze.
- A5: make the live feed check mandatory before cutover, with a concrete recovery procedure using a previously signed and verified release asset.
- Add A6: retain or formally decommission nightlies.
- Add A7: choose between reusable-workflow exact-file binding and supported exact workflow-name binding before implementation.

## Residual authority

- Landing credentials can overwrite/delete the whole landing site except the releases carve-out.
- Playground credentials can overwrite/delete the playground.
- Release AWS credentials can overwrite only `latest.json`, including with garbage or a replay, but cannot delete it or alter either site. A compromised entire release job also has a GitHub `contents: write` token, which is substantially more powerful than the AWS role alone.
- Every deploy role can submit arbitrary invalidation paths for the shared distribution. This can cause cache misses, origin load, quota pressure, and cost—not merely harmless “cache churn.” It is acceptable given the single-distribution architecture, but should be documented accurately and monitored.
- The owner can change workflows, secrets, and rulesets and thereby obtain any role. No repository rule can create a second human authority.
- The site bucket is not configured for versioning in tofu, so site overwrite/delete recovery depends on rebuilding and redeploying. Versioning would materially improve recovery without giving CI `DeleteObjectVersion`.

VERDICT: reject (blocking findings: D1 uses a claim absent from top-level jobs and a malformed value; the cutover silently breaks publish-nightlies; release-role trust is not proven before tag creation)
---
## Final fresh-context verdict (post-fold)
Round 1: reject (7 findings). After folding all findings + resolving the D1 claim contradiction (→ `workflow`
name) + operative-consistency rounds: **round 5 VERDICT: approve.** Deep-tier audit complete.
