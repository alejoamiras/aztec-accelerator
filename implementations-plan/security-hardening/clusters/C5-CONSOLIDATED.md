# C5 / F-005 — infra-deploy-authz — CONSOLIDATED plan (deep tier)

Consolidation of three independent plans (`C5-plan-main.md`, `C5-plan-codex.md`, `C5-plan-fable.md`).
They converged strongly; the Decision Ledger below records where they diverged and why the consolidated
choice was made. **Commit + validate only — a human runs `tofu apply`, `gh secret`, and the ruleset API.**

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

Each role trust conditions on `StringEquals`: `aud=sts.amazonaws.com`, `sub=repo:alejoamiras/aztec-accelerator:ref:refs/heads/main`, **and a workflow-file claim** (see Ledger D1) binding it to exactly one workflow.

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
- **D1 — workflow-binding claim (main+codex agree on binding; differ on WHICH claim).** All roles share
  `sub=main`, so `sub` alone does NOT isolate pipelines from each other. Bind each role to its workflow.
  Codex used `token.actions.githubusercontent.com:workflow` (the workflow NAME — impersonatable by a
  rename). Main-leg used `job_workflow_ref` (the workflow FILE @ ref — immune to renames). **CHOSEN: the
  FILE-path claim** (`token.actions.githubusercontent.com:job_workflow_ref` =
  `repo:alejoamiras/aztec-accelerator/.github/workflows/<file>.yml@refs/heads/main`, StringEquals), it is
  strictly stronger. IMPLEMENTATION MUST VERIFY the exact claim name AWS accepts (`job_workflow_ref` vs
  `workflow_ref` for top-level jobs — for these top-level S3 jobs they coincide); fall back to `workflow`
  (name) only if the file claim proves unusable, documenting the weaker residual. Residual (all variants):
  malicious code *already merged to main* runs as the legit workflow — unstoppable by any claim; mitigated
  by main protection + F-004 client-side manifest verification.
- **D2 — release policy scope.** Codex: exact object `landing/releases/latest.json`. main/fable:
  `landing/releases/*`. **CHOSEN: exact object** (tightest least-privilege; the only object the release
  writes). Note in README: adding a second release-feed object requires a policy update.
- **D3 — 4th required check (OpenTofu validate / Actionlint Status).** fable: recommend YES. codex: flags
  its absence as a material residual in a zero-review repo but leaves it an Ask. main: didn't require.
  **CHOSEN: surface as ASK A1** (recommend adding `Actionlint Status` — integration_id 15368, already a
  fail-closed aggregate that runs the tofu gate — as a 4th required check so infra validation is a merge
  gate, not advisory). User decides at the approval gate.
- **D4 — runtime cross-assumption isolation.** fable proposed GitHub `environment:`-scoped subs (Phase 5).
  main/codex close the same gap via the workflow-file claim (D1). **CHOSEN: the workflow-file claim is
  primary** (no environment plumbing needed); environments are noted as an optional defense-in-depth ASK
  A2 (they add a branch-policy layer + protect against the malicious-rename residual if only `workflow`
  name were available).
- **D5 — role granularity (3 vs 2).** All legs: keep 3 (per-pipeline). 2-role (feed vs sites) protects the
  feed but lets a compromised site token cross-deface landing↔playground; 3 roles are the minimum aligned
  with distinct public-damage domains and the correct shape for D1/D4. **CHOSEN: 3 roles.**

## Phases

### Phase 1 — PR-1: additive IAM split + ruleset JSON (commit only; deploy-compatible)
Files: `infra/tofu/iam.tf`, `infra/tofu/outputs.tf`, `infra/tofu/README.md`, `infra/rulesets/main-branch-protection.json`.
- `iam.tf`: keep the OIDC provider. **Narrow** the legacy `aws_iam_role.ci` trust to `StringEquals sub=main`
  (drop nightlies + both chore/aztec-* NOW) but LEAVE its broad policy (all live workflows still use it
  until PR-2 lands). **Add** the 3 new roles + inline policies exactly as the table above (StringEquals
  trust: aud + main sub + job_workflow_ref file claim; landing carries the explicit Deny +
  `s3:AbortMultipartUpload` on both Allow and Deny; release = Put exact latest.json + GetBucketLocation +
  invalidation only; playground mirrors landing sans Deny; ListBucket with `s3:prefix` conditions).
- `outputs.tf`: add `landing_deploy_role_arn`, `release_feed_role_arn`, `playground_testnet_role_arn`;
  keep `ci_role_arn` (removed in PR-3).
- `main-branch-protection.json`: keep target=main only, `bypass_actors: []`, 0 approvals, the 3 checks +
  integration_id 15368; ADD rule types `deletion`, `non_fast_forward`, `required_linear_history`; set
  `required_review_thread_resolution: true`; add `allowed_merge_methods: ["squash","rebase"]` (linear
  history needs squash/rebase). (If ASK A1 accepted, also append the 4th required check.)
- `README.md`: document the temporary dual-role window + the 3 new secrets + the `--exclude "releases/*"`.
- **Validation gate:** `tofu -chdir=infra/tofu fmt -check -diff && init -backend=false -input=false && validate`;
  `bun run lint:tofu`; `bun run lint:actions`; `jq -e . infra/rulesets/main-branch-protection.json`;
  `git diff --check` → all exit 0. Plus the existing `Lint OpenTofu` PR job green. (Layers: infra fmt +
  tofu-validate + actionlint + json-lint.)

### Phase 2 — PR-2: workflow cutover to per-pipeline secrets + safety asserts (commit only)
Files: `deploy-landing.yml`, `release-accelerator.yml`, `publish-testnet.yml`.
- `deploy-landing.yml`: `role-to-assume: ${{ secrets.AWS_ROLE_ARN_LANDING }}`; `sync --delete --exclude "releases/*"`;
  early `refs/heads/main` assertion for the dispatch path; optional post-sync assert that
  `landing/releases/latest.json` still exists.
- `release-accelerator.yml` (~L899): `${{ secrets.AWS_ROLE_ARN_RELEASE }}`; assert `GITHUB_REF == refs/heads/main`
  in the early validation job; **move the AWS-cred `configure-aws-credentials` step BEFORE `gh release create`**
  so broken trust/secret wiring fails before a GitHub release is published; keep the exact S3 key; fix the
  invalidation path to `/landing/releases/latest.json` (the CloudFront viewer function rewrites the URI
  pre-cache, so the cached key is `landing/releases/latest.json`) — keep `/releases/latest.json` too.
- `publish-testnet.yml` (~L80): `${{ secrets.AWS_ROLE_ARN_PLAYGROUND }}`; scope `id-token: write` to the
  `deploy-app` job only (not repo/other jobs); preserve `_publish-sdk.yml`'s Sigstore provenance perms;
  main-ref validation gating the side-effecting jobs.
- `publish-nightlies.yml` + `_publish-sdk.yml`: **untouched.**
- **Trigger-safety:** PR-2 changes only `.github/workflows/**`, which doesn't match deploy-landing's
  `paths: packages/landing/**` push filter → merging cannot itself fire a deploy.
- **Validation gate:** `bun run lint:actions`; `rg -n 'secrets\.AWS_ROLE_ARN([^_]|$)' .github/workflows`
  shows ONLY `publish-nightlies.yml` (dormant); `Actionlint Status` green on the PR.

> Phases 1 + 2 land in the SAME C5 PR into `security-hardening` (both are commit-only). They are separated
> here because the human-apply runbook must apply them at different points (see Phase 4). The legacy-role
> DELETION (below) is a distinct later commit.

### Phase 3 — Legacy role removal (commit prepared; applied only at runbook step, after main cutover)
Files: `infra/tofu/iam.tf` (delete `aws_iam_role.ci` + `_policy.ci`), `infra/tofu/outputs.tf` (delete `ci_role_arn`).
- **Validation gate:** same tofu fmt/validate + lint:actions + git diff --check; PR/commit body states the
  expected plan: **2 destroy, 0 add, 0 change, −1 output**; no bucket/distribution changes.

### Phase 4 — Human-applies runbook (`clusters/C5-runbook.md`; written, NOT executed)
Adopt Codex's staged, fail-closed sequence + Fable's `simulate-principal-policy` proofs. Critical ordering
(the whole cutover-safety trick):
```
narrow+add roles (apply) → set 3 secrets → land workflow cutover ON MAIN → smoke new roles
→ simulate-principal-policy proofs → destroy legacy role+policy → delete old AWS_ROLE_ARN secret
→ apply ruleset (PUT existing / POST if absent, after backing up the live ruleset)
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
- **Core control:** per-pipeline roles + `job_workflow_ref` file binding → a stolen landing/playground
  token (or a compromised *other* main workflow) cannot assume the release role; only `release-accelerator.yml`
  on main can write `latest.json`.
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
- release-accelerator is dispatch-only; OIDC sub stays main.
- 4 workflows reference `secrets.AWS_ROLE_ARN` (deploy-landing, release, publish-testnet, publish-nightlies).
- Ruleset targets main only, 0 approvals, 3 checks (integration_id 15368), no linear/force-push/deletion.
- Each S3-writing job is top-level in its own workflow file → the workflow-file claim binds per pipeline.
### Inferences (verify at implementation / preflight)
- The exact AWS condition-key name for the workflow-file claim (`job_workflow_ref` vs `workflow_ref`) and
  its value format `repo/.github/workflows/FILE@refs/heads/main` — VERIFY against AWS+GitHub docs (D1).
- The repo still uses the pre-immutable OIDC `sub` format (`ref:refs/heads/main`) — preflight `gh api
  .../actions/oidc/customization/sub`.
- `aws s3 sync --exclude "releases/*"` protects `landing/releases/*` from `--delete` (documented CLI
  behavior; R6 head-object confirms empirically).
- `simulate-principal-policy` predicts runtime authz (no SCPs/permission boundaries in a solo account).
- Squash or rebase merging is enabled (required for linear history) — preflight confirms.
### Asks (surface at the approval gate)
- **A1:** add a 4th required status check (`Actionlint Status`, which runs the tofu gate) so infra
  validation gates the merge instead of being advisory? [Recommended]
- **A2:** additionally adopt per-pipeline GitHub `environment:` scoping (defense-in-depth beyond the
  workflow-file claim)? [Optional]
- **A3:** enable hardware-key 2FA on the owner account + confirm tfstate bucket is owner-only + versioned?
  [Out of repo scope; single biggest remaining lever]
- **A4:** confirm the brief deploy/merge freeze during the human cutover is acceptable.
- **A5:** has a landing deploy already deleted the live `latest.json`? (preflight head-object; if missing,
  re-run the last stable release's upload after smokes.)

## Seeds (draft — finalized post-approval)
- **/goal (recommended):** All C5 phases ✓ in the consolidated plan, each backed by its validation gate
  (tofu fmt/validate, lint:tofu, lint:actions, ruleset jq assertion, the existing Lint-OpenTofu PR job
  green) reported in the transcript; the human-applies runbook (`C5-runbook.md`) written; post-impl codex
  xhigh audit folded; PR into security-hardening CI green; NOTHING applied (human applies tofu + secrets +
  ruleset).
- **/loop 15m (fallback):** drive C5 — read the consolidated plan, run tofu fmt/validate + lint:actions
  after each edit, commit, push, watch CI; consult codex xhigh on any IAM/OIDC condition-key detail; NEVER
  run tofu apply / gh secret / the ruleset API (human-only); mark phases ✓ only when their gate passes.
