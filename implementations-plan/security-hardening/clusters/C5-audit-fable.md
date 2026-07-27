# C5 / F-005 — FABLE double-audit of the consolidated plan

VERDICT: **conditional approve** (5 conditions). All findings are plan-text corrections — no
architecture rethink. Verified against current AWS/GitHub docs + the real tree.

## Findings (ranked)
- **H1 (High) — D1 OIDC claim VALUE format wrong.** `job_workflow_ref` IS a valid AWS IAM condition
  key (AWS GitHub-tab: actor, actor_id, job_workflow_ref, repository, repository_id,
  repository_owner_id, workflow, ref, environment, enterprise_id). But the plan's value had a bogus
  `repo:` prefix (that prefix is for `sub` only). Correct value:
  `alejoamiras/aztec-accelerator/.github/workflows/<file>.yml@refs/heads/main`. With the wrong value,
  EVERY AssumeRole fails → deploy outage; the natural "drop the claim, keep sub=main" hotfix silently
  re-opens cross-pipeline assumption AND all runbook gates still pass (smokes green; simulate-principal-
  policy never evaluates TRUST policies). Also: `workflow_ref` is NOT an AWS key (real weaker fallback is
  `workflow` = name). FIX: correct the value (no `repo:`), delete `workflow_ref`, ADD a negative
  cross-role AssumeRole smoke to the runbook (only gate catching a wrong value or the degraded hotfix).
- **M1 (Med) — "nightlies unused" unverified in-plan.** origin/nightlies exists, publish-nightlies.yml is
  live/dispatchable, cloudfront.tf routes live nightly-playground → /playground-nightly. Dropping the
  trust refs + deleting the legacy secret silently makes /playground-nightly unwritable. FIX: Ask A6
  (confirm nightly deploy-path retirement before ref-drop/secret-delete; else 4th role or defer).
- **M2 (Med) — ListBucket prefix + post-sync assert footgun.** `StringEquals s3:prefix "landing/"` denies
  finer-prefix lists; the "assert feed survived" via head-object 403s (landing role has no GetObject) →
  permanent red deploys. FIX: `StringLike s3:prefix ["landing/*"]`; assert via `list-objects-v2` (not
  head-object), or drop the assert.
- **M3 (Med, latent) — Ask A2 environments footgun.** Adding `environment:` changes the default `sub` to
  `…:environment:NAME`, breaking the `sub=main` trust (outage if adopted). AWS now has a native
  `environment` condition key. FIX: annotate A2.
- **L1 — `allowed_merge_methods` placement.** It's a `pull_request.parameters` field (lowercase
  merge/squash/rebase), not top-level. FIX: state placement + live-ruleset jq assert.
- **L2 — Fact "OIDC sub stays main" overstated.** Reword: "when dispatched from main; wrong ref fails
  closed at the assert/AssumeRole."
- **L3 — publish-nightlies dangles** on the deleted legacy secret post-cutover. Note in README/runbook.
- **L4 — id-token scoping wording** vs Sigstore: "deploy-app + the publish-sdk call job" (both need
  id-token: write for provenance).

## Verified SOUND (no finding)
job_workflow_ref key choice + StringEquals; Deny-precedence carve-out (fail-loud); --delete latent-bug
diagnosis; exact-object release scope (D2); viewer-request cache-key/invalidation reasoning; every
mis-ordering fails closed (given H1 fixed); legacy-retire-after-main-cutover rule; ruleset API shapes;
NO owner lockout + NO campaign-PR deadlock (SDK/App/Accelerator Status + Actionlint Status are
always()-reporting aggregates on every PR to main — verified); 0 approvals; 3-role granularity; residuals.
