# C5 / F-005 — human-applies runbook

**This is the ONLY place the C5 changes touch live AWS/GitHub.** CI never has AWS creds and never applies.
Run these yourself (owner/admin AWS creds + `gh`). Every mis-ordering fails CLOSED (a visible red run,
never a silent privilege grant). Do a brief deploy/merge freeze for the duration.

Fill in: `REPO=alejoamiras/aztec-accelerator`; capture `RULESET_ID` (R2); AWS `ACCOUNT_ID` (`aws sts
get-caller-identity`).

## R0 — Preflight (read-only)
```bash
cd infra/tofu && tofu init && tofu plan        # EXPECT: no changes to EXISTING resources beyond the C5 additions
gh api "repos/$REPO" --jq '{default_branch,allow_squash_merge,allow_rebase_merge}'  # squash OR rebase MUST be true (linear history needs it)
gh api "repos/$REPO/actions/oidc/customization/sub" || true   # confirm the pre-immutable sub format (ref:refs/heads/main)
gh api "repos/$REPO/rulesets" --jq '.[] | {id,name,enforcement}'    # capture RULESET_ID for "Main branch protection"
aws s3api list-objects-v2 --bucket aztec-accelerator-site --prefix landing/releases/latest.json --query KeyCount  # is the feed live? (the latent sync-delete bug may have removed it)
# Also confirm no AWS Organization SCP / permission boundary narrows the new roles (simulate is not enough).
```

## R1 — Apply the additive IAM + S3 (legacy role RETAINED, trust-narrowed)
Merge the C5 PR into `security-hardening` first (code only — no runtime change). Then:
```bash
cd infra/tofu
tofu plan -out=/tmp/c5-add.tfplan
tofu show -no-color /tmp/c5-add.tfplan
# EXPECT: 3 roles + 3 role-policies + 3 assume-policy data + s3 lifecycle + s3 versioning ADDED;
#         aws_iam_role.ci trust CHANGED (narrowed to main); +3 outputs; 0 destroy; NO bucket/distribution replace.
tofu apply /tmp/c5-add.tfplan
```
Rollback: `git revert` the PR + re-apply. The legacy role still backs all live deploys → nothing breaks.

## R2 — Apply the hardened main ruleset EARLY (before the main-integration PR)
```bash
gh api "repos/$REPO/rulesets/$RULESET_ID" --jq '{name,target,enforcement,bypass_actors,conditions,rules}' > /tmp/ruleset.before.json  # backup
gh api --method PUT "repos/$REPO/rulesets/$RULESET_ID" --input infra/rulesets/main-branch-protection.json > /tmp/ruleset.after.json
gh api "repos/$REPO/rulesets/$RULESET_ID" --jq '{rules:[.rules[].type],enforcement}'
# EXPECT: deletion, non_fast_forward, required_linear_history, pull_request, required_status_checks; "active".
```
Rollback: `gh api --method PUT "repos/$REPO/rulesets/$RULESET_ID" --input /tmp/ruleset.before.json`.
Never test force-push/deletion against `main`.

## R3 — Wire the three new secrets (BEFORE the workflow cutover reaches main)
```bash
cd infra/tofu
gh secret set AWS_ROLE_ARN_LANDING    --repo "$REPO" --body "$(tofu output -raw landing_deploy_role_arn)"
gh secret set AWS_ROLE_ARN_RELEASE    --repo "$REPO" --body "$(tofu output -raw release_feed_role_arn)"
gh secret set AWS_ROLE_ARN_PLAYGROUND --repo "$REPO" --body "$(tofu output -raw playground_testnet_role_arn)"
gh secret list --repo "$REPO" | grep AWS_ROLE_ARN_
```

## R4 — Land the workflow cutover on MAIN
The workflow role references only take effect once the C5 workflow changes are on `main`. Complete the
campaign's `security-hardening → main` integration (or cherry-pick the workflow commit). Until then, live
main workflows still use the legacy role — that is why the legacy role is retired LAST.

## R5 — Smoke each pipeline against its new role
```bash
# Landing (real): after cutover on main
gh workflow run deploy-landing.yml --repo "$REPO" --ref main
gh run watch "$(gh run list --repo "$REPO" --workflow deploy-landing.yml --limit 1 --json databaseId --jq '.[0].databaseId')" --exit-status
aws s3api list-objects-v2 --bucket aztec-accelerator-site --prefix landing/releases/latest.json --query KeyCount  # feed SURVIVED the --delete

# Playground (real, SDK skipped)
gh workflow run publish-testnet.yml --repo "$REPO" --ref main -f skip_sdk_publish=true
gh run watch "$(gh run list --repo "$REPO" --workflow publish-testnet.yml --limit 1 --json databaseId --jq '.[0].databaseId')" --exit-status

# Release role — auth_probe (no tag/release/publish side effects):
gh workflow run release-accelerator.yml --repo "$REPO" --ref main -f version=0.0.0-authprobe -f auth_probe=true
# watch that `Release AWS trust preflight` PASSES; then cancel the run (build jobs may still be going).

# IAM policy simulation (trust policies are NOT simulated → also rely on the real assumes above + the
# negative test below):
aws iam simulate-principal-policy --policy-source-arn arn:aws:iam::$ACCOUNT_ID:role/aztec-accelerator-ci-landing \
  --action-names s3:PutObject s3:DeleteObject \
  --resource-arns arn:aws:s3:::aztec-accelerator-site/landing/releases/latest.json  # EXPECT explicitDeny
aws iam simulate-principal-policy --policy-source-arn arn:aws:iam::$ACCOUNT_ID:role/aztec-accelerator-ci-release-feed \
  --action-names s3:PutObject --resource-arns arn:aws:s3:::aztec-accelerator-site/landing/releases/latest.json  # EXPECT allowed
aws iam simulate-principal-policy --policy-source-arn arn:aws:iam::$ACCOUNT_ID:role/aztec-accelerator-ci-release-feed \
  --action-names s3:DeleteObject --resource-arns arn:aws:s3:::aztec-accelerator-site/landing/releases/latest.json  # EXPECT implicitDeny
aws iam simulate-principal-policy --policy-source-arn arn:aws:iam::$ACCOUNT_ID:role/aztec-accelerator-ci-playground-testnet \
  --action-names s3:PutObject --resource-arns arn:aws:s3:::aztec-accelerator-site/landing/index.html  # EXPECT implicitDeny
```

## R5b — MANDATORY negative cross-role AssumeRole test (D1)
This is the ONLY gate that catches a mis-bound `workflow` claim (a positive smoke + policy simulation both
pass silently on a wrong/dropped claim). From a scratch/other workflow on `main` (NOT "Release
Accelerator"), attempt `role-to-assume: <release ARN>` and confirm the AssumeRole is **DENIED**. If it
SUCCEEDS, the `workflow`-claim binding is not effective — STOP and fix before retiring the legacy role.

## R6 — Retire the legacy role (SEPARATE post-smoke PR)
Only after R5 + R5b pass. Prepare a distinct PR deleting `aws_iam_role.ci`, `aws_iam_role_policy.ci`, and
the `ci_role_arn` output. Confirm no old-role deploy (queued OR running) is in flight, then:
```bash
gh run list --repo "$REPO" --status in_progress --json workflowName --jq '.[].workflowName'   # none of the deploy workflows
cd infra/tofu && tofu plan -out=/tmp/c5-retire.tfplan && tofu show -no-color /tmp/c5-retire.tfplan  # EXPECT: 2 destroy, 0 add/change
tofu apply /tmp/c5-retire.tfplan
aws iam get-role --role-name aztec-accelerator-ci-github 2>&1 | grep -q NoSuchEntity && echo "legacy gone" || echo "STILL EXISTS"
gh secret delete AWS_ROLE_ARN --repo "$REPO"
```
Deleting the role revokes its already-issued sessions; that's why R6 waits for a quiet window. Rollback
(before you're confident): revert the PR + `tofu apply` recreates the identical-ARN role, then
`gh secret set AWS_ROLE_ARN`. Do NOT add fallback logic to workflow source.

## Residuals accepted (see C5-CONSOLIDATED.md §Security)
CloudFront invalidation is distribution-wide (no path IAM key) — cache-bust only. The release role can
still OVERWRITE `latest.json` with garbage/replay — F-004 client-side manifest verification is the
backstop (it cannot DELETE it). `publish-nightlies.yml` is inert (unset `NIGHTLIES_ENABLED`) — reviving it
needs a 4th `playground-nightly/*` role. Owner/admin compromise can rewrite rulesets/secrets — no second
human authority in a solo repo (hardware-key 2FA is the out-of-repo lever).
