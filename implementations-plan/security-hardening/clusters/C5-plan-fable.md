# C5 / F-005 — infra-deploy-authz — FABLE plan (deep leg 3 of 3)

> Independent "fable" planning leg. Caught two brief misstatements that materially change the plan
> (sync --delete latent bug; tofu CI gate already exists) + contributed the R1–R7 human-apply runbook
> with `simulate-principal-policy` validation + the fail-closed mis-ordering table. Verbatim below.

## 0. Corrections to the brief (verified against the tree)
1. **`deploy-landing.yml` DOES use `sync --delete`** (L38–41, since `1502940`). `packages/landing/` has no
   `releases/` output, so the sync `--delete` DELETES `landing/releases/latest.json` (F-004 feed) on every
   landing deploy after a stable release — a latent live bug. It also means an explicit `Deny` on
   `landing/releases/*` in the landing role would make every landing deploy FAIL (AccessDenied on delete)
   unless the workflow adds `--exclude "releases/*"` (fixes the bug AND makes the Deny viable).
2. **The tofu fmt+validate PR gate ALREADY EXISTS**: `actionlint.yml` has a `tofu` job (`fmt -check -diff`,
   `init -backend=false`, `validate`) gated by paths-filter on `infra/tofu/**/*.tf` (C0 #377, SHA-pinned
   C3 #386); `.terraform.lock.hcl` committed. So "add a validate workflow" → "verify + optionally make it
   a REQUIRED check".
3. `release-accelerator.yml` is dispatch-only; OIDC `sub` stays `refs/heads/main` for the whole run (the
   tag push doesn't change it) → NO tag ref needed in the release role trust.
4. `publish-nightlies.yml` also uses `secrets.AWS_ROLE_ARN` (L73, writes `playground-nightly/`); per the
   nightlies-dropped decision it stays untouched and fails closed at AssumeRole after cutover.

## 1. Target architecture (3 roles + legacy narrowed-then-deleted)
- `aztec-accelerator-deploy-landing` — sub main; Put/Delete/AbortMPU on `landing/*`; **Deny** same on
  `landing/releases/*`; ListBucket prefix `landing/`; CreateInvalidation. Consumed by deploy-landing.yml
  via `AWS_ROLE_ARN_LANDING`.
- `aztec-accelerator-release-feed` — sub main; Put/AbortMPU on `landing/releases/*` ONLY (no List, no
  Delete); CreateInvalidation. release-accelerator.yml via `AWS_ROLE_ARN_RELEASE`.
- `aztec-accelerator-deploy-playground` — sub main; Put/Delete/AbortMPU on `playground/*`; ListBucket
  prefix `playground/`; CreateInvalidation. publish-testnet.yml via `AWS_ROLE_ARN_PLAYGROUND`.
- `aztec-accelerator-ci-github` (legacy) — trust narrowed to `main` in PR-1, DELETED in PR-3.
- Cutover: 3 new repo secrets from `tofu output`; each workflow switched to its own secret in PR-2 which
  merges only AFTER the secrets exist.

## Phases (verbatim from the fable leg)

### Phase 0 — Preflight (human, read-only)
`tofu init && tofu plan` (expect no changes), `aws sts get-caller-identity`, `gh secret list`,
`gh api repos/.../rulesets` (capture RULESET_ID), `aws s3api head-object … landing/releases/latest.json`
(is the feed currently live given the --delete bug?). Gate: plan = no changes; ruleset ID captured.

### Phase 1 — PR-1: role split in tofu + ruleset JSON (commit only)
- `iam.tf`: keep the OIDC provider; narrow `aws_iam_role.ci` trust to `StringEquals sub = local.github_main_sub`
  (drop nightlies + chore/aztec-*), leave its policy until Phase 2; ADD the 3 roles+inline policies.
  Landing policy carries the explicit `Deny` on `landing/releases/*` + `s3:AbortMultipartUpload` on both
  Allow and Deny; ListBucket with `s3:prefix` condition `["landing/","landing/*"]`. Release policy is
  Put+AbortMPU on `landing/releases/*` only (no List/Delete). Playground mirrors landing sans Deny.
- `outputs.tf`: 3 new role ARNs (keep `ci_role_arn` until Phase 3).
- `main-branch-protection.json`: add `deletion`, `non_fast_forward`, `required_linear_history` rules;
  `required_review_thread_resolution: true`; `allowed_merge_methods: ["squash","rebase"]`; keep 0 approvals
  + 3 checks. ASK: also add `Actionlint Status` as a 4th REQUIRED check (else the tofu gate is advisory).
- README secrets table update (3 new secrets; `--exclude "releases/*"` in the deploy snippet).
- Gate: `bun run lint:tofu`; `tofu -chdir=infra/tofu init -backend=false && validate`; `bun run lint:actions`;
  the existing `Lint OpenTofu` PR job green.

### Phase 2 — PR-2: workflow cutover (commit only; merged mid-runbook)
- deploy-landing.yml: `role-to-assume: AWS_ROLE_ARN_LANDING`; sync gets `--delete --exclude "releases/*"`
  (bug fix + Deny-compat); optional post-sync assert that `landing/releases/latest.json` survived.
- release-accelerator.yml L899: `AWS_ROLE_ARN_RELEASE`; optional invalidation-path fix to
  `--paths "/landing/releases/latest.json" "/releases/latest.json"` (the CloudFront function rewrites the
  URI pre-cache, so the cached key is `/landing/releases/latest.json`).
- publish-testnet.yml L82: `AWS_ROLE_ARN_PLAYGROUND`. publish-nightlies untouched.
- Trigger-safety: PR-2 touches only `.github/workflows/**`, which doesn't match deploy-landing's
  `paths: packages/landing/**` → merging can't fire a deploy.
- Gate: `bun run lint:actions`; Actionlint Status green.

### Phase 3 — PR-3: legacy role removal (merged only at runbook R7)
Delete `aws_iam_role.ci` + policy + `ci_role_arn` output. Gate: lint:tofu + validate; PR body states
"2 destroy, 0 add, 0 change, -1 output".

### Phase 4 — Human-applies runbook (R1–R7, exact/ordered/reversible)
R1 merge PR-1 (code only). R2 `tofu plan` (expect 6 add / 1 change / 0 destroy / +3 outputs) → apply
(old role still backs deploys → nothing breaks). R3 apply ruleset via `gh api -X PUT …/rulesets/<id>
--input …` (independent). R4 `gh secret set AWS_ROLE_ARN_{LANDING,RELEASE,PLAYGROUND}` from `tofu output
-raw …` BEFORE PR-2 merges. R5 merge PR-2 (deploys now use scoped roles). R6 validate: real dispatch of
deploy-landing + publish-testnet (+ head-object confirms feed survived), and `aws iam
simulate-principal-policy` for the release role (Allow feed), landing role (explicitDeny feed), playground
role (implicitDeny landing). R7 after no deploys in flight: merge PR-3 → `tofu plan` (expect 2 destroy) →
apply → `gh secret delete AWS_ROLE_ARN`. Reversible at each step (legacy role ARN is name-derived, recreatable).
Mis-ordering table: EVERY mis-order fails CLOSED (empty/typo secret → assume errors; R7-before-PR2 →
AssumeRole fails; cross-wired secrets → S3 denied). No mis-order grants privilege.

### Phase 5 — OPTIONAL Ask: environment-scoped OIDC subs
Identical `sub=main` on all 3 roles ⇒ a compromised landing/playground JOB RUNTIME (malicious dep during
build, same job holds `id-token: write`) can `AssumeRoleWithWebIdentity` into the release-feed role. Fix:
GitHub `environment:` per deploy job + deployment-branch policy main + trust on
`sub = repo:…:environment:release-feed`. [NOTE for consolidation: the MAIN leg closes this SAME gap at
the config layer via a `job_workflow_ref` trust condition — reconcile: job_workflow_ref binds the token to
the workflow FILE, which a compromised step can't forge, so it also blocks cross-assumption. Pending
Codex's verification that job_workflow_ref is a valid IAM condition key.]

## 6. Security & Adversarial (self-attack answers)
(a) landing/* EXCEPT landing/releases/* — YES via Allow landing/* + explicit Deny landing/releases/*
    (Deny precedence, future-proof); footguns: sync --delete trips the Deny → `--exclude "releases/*"`;
    ListBucket is bucket-scoped (prefix condition gates the request prefix; sync lists at landing/ not
    landing/releases/); AbortMultipartUpload added to both Allow+Deny.
(b) CloudFront invalidation — NO path-level IAM condition; distribution-wide per role. Accepted residual
    (cache-bust only, weak cost-DoS needing an already-compromised job; per-path would need 1 distro per
    site). Invalidations match the REWRITTEN key → the feed path fix.
(c) Secret cutover — every mis-ordering fails closed; reversibility via keep-old-until-validated.
(d) Residuals: cross-role assumption (biggest; closed by Phase 5 or job_workflow_ref); push-to-main = all
    roles (owner/PR compromise; ruleset narrows mistakes, admin can edit ruleset — audit-logged); feed
    DELETION now impossible from CI (only release role touches the prefix, no Delete); human AWS creds out
    of scope; tfstate bucket is root-of-trust (Ask); publish-nightlies fails closed after R7.

## 7. Assumptions
Facts: sync --delete bug; tofu CI gate exists; release sub=main; 4 workflows use AWS_ROLE_ARN; one
role/whole-bucket/4-ref today; CloudFront function rewrites URIs pre-cache (feed key = landing/releases/
latest.json, max-age 300). Inferences: live ruleset matches committed JSON (Phase 0 GET confirms);
AWS_ROLE_ARN == ci_role_arn (Phase 0); --exclude destination-deletion semantics; simulate-principal-policy
predicts runtime authz (no SCPs/boundaries in a solo account). Asks: (1) Actionlint Status as 4th required
check? [rec YES]; (2) adopt Phase 5 environments? (3) include the 2 optional workflow hardenings in PR-2?
(4) has a landing deploy already deleted the live feed? (5) tfstate bucket access owner-only + versioned?
(6) hardware-key 2FA on the owner account?
