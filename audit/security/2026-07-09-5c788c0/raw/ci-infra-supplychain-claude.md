# CI / Infra / Supply-Chain — Claude findings

Cluster: ci-infra-supplychain
Scope audited: `.github/workflows/*.yml`, `.github/actions/*/action.yml`, `scripts/check-aztec-update.ts`,
`scripts/update-aztec-version.ts`, `scripts/get-sdk-publish-version.ts`, `infra/tofu/{iam,s3,cloudfront,acm,variables,backend}.tf`,
`infra/rulesets/main-branch-protection.json`.

---

## Finding 1: AWS-credentialing and toolchain-critical GitHub Actions are pinned to mutable tags, not commit SHAs

1. **Title**: Mutable-tag (not SHA) pinning of third-party Actions, including the one that mints AWS OIDC credentials

2. **Impact factors**: Integrity + Confidentiality violated; blast radius = **all users / the whole deploy surface** (a compromised tag is pulled by every workflow run across the repo the next time it fires — CI secrets: `AWS_ROLE_ARN`-assumable session, `NPM_TOKEN`, `RELEASE_BOT_PRIVATE_KEY`, `GITHUB_TOKEN`, code-signing material used by `release-accelerator.yml`). Data sensitivity: high (cloud credentials, npm publish token, code-signing/updater key material all pass through jobs that reference these actions). Exploitability: attack vector = network (upstream GitHub repo/org takeover, not this repo), attack complexity = low-to-moderate (well-precedented: `tj-actions/changed-files` and `reviewdog/action-setup` were both compromised at their tag refs in March 2025 and used to dump CI secrets across thousands of repos), privileges required = none (repo owner does nothing wrong; the compromise happens upstream and is pulled in automatically), user interaction = none (any future workflow run silently pulls the new tag content).

3. **Evidence confidence**: high (directly observable in the YAML; the attack class is empirically documented, not speculative).

4. **OWASP category + CWE**: OWASP A08:2021 – Software and Data Integrity Failures. CWE-1357 (Reliance on Insufficiently Trustworthy Component), related CWE-829 (Inclusion of Functionality from Untrusted Control Sphere).

5. **Trace**: Source = the upstream Action repository at the referenced tag (e.g. `aws-actions/configure-aws-credentials`'s `v6` tag) being force-moved to a malicious commit by an attacker who compromises that org/maintainer. Sink = the next scheduled/dispatched/PR run of any workflow in this repo that references the tag, which executes the attacker's code inside the job's runner — and for the AWS-credentials action specifically, that job has already been granted (or is about to be granted) `permissions: id-token: write` and is passed `secrets.AWS_ROLE_ARN`:
   - `.github/workflows/deploy-landing.yml:11` (`id-token: write`) → `.github/workflows/deploy-landing.yml:31` (`uses: aws-actions/configure-aws-credentials@v6`)
   - `.github/workflows/publish-testnet.yml:17` (`id-token: write`) → `.github/workflows/publish-testnet.yml:78`
   - `.github/workflows/publish-nightlies.yml:11` (`id-token: write`) → `.github/workflows/publish-nightlies.yml:69`
   - `.github/workflows/release-accelerator.yml:573` (`id-token: write`) → `.github/workflows/release-accelerator.yml:809`

6. **Missing control**: SHA-pinning (immutable commit reference) for third-party Actions, at minimum for any action that runs inside an `id-token: write` job or otherwise touches a secret. The repo already demonstrates it knows this technique — `.github/workflows/_aztec-update.yml:158` pins `actions/create-github-app-token@bcd2ba49218906704ab6c1aa796996da409d3eb1 # v3.2.0` to a full commit SHA with the version kept as a comment — but this discipline was not applied to any other action, including the one that is arguably more security-critical (it mints cloud credentials, not just a repo-scoped app token).

7. **Exploit/violation scenario**: An attacker compromises the GitHub account/CI publishing pipeline of `aws-actions/configure-aws-credentials` (or `dtolnay/rust-toolchain`, whose ref here is literally the floating `stable` branch — see Instances) and re-points `v6` (or `stable`) at a commit that, alongside performing the credential exchange, exfiltrates the resulting `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`/`AWS_SESSION_TOKEN` (or the raw OIDC JWT) to an attacker-controlled endpoint. The very next `workflow_dispatch` of `publish-testnet.yml`, `publish-nightlies.yml`, or `release-accelerator.yml`, or the next `push` to `main` touching `packages/landing/**`, silently leaks a live session for the `aztec-accelerator-ci-github` role — which (see Finding 2) can write/delete any object in the whole site bucket, including `releases/latest.json`.

8. **Preconditions**: A supply-chain compromise of the specific upstream Action repository/maintainer account. No privileges inside this repo are required by the attacker; the repo owner takes no unsafe action beyond re-running CI as normal.

9. **Why existing mitigations fail**: None of the documented SEC-0N mitigations (bb-digest fail-closed, Host allowlist, deny-by-default origin authorization) apply to the CI/CD supply chain at all — this is an orthogonal trust plane. The one place SHA-pinning IS applied (`create-github-app-token`) does not generalize to cover this gap.

10. **Instances** (all third-party Actions referenced by mutable tag/branch, repo-wide):
    - `aws-actions/configure-aws-credentials@v6` — `.github/workflows/deploy-landing.yml:31`, `publish-testnet.yml:78`, `publish-nightlies.yml:69`, `release-accelerator.yml:809`
    - `dtolnay/rust-toolchain@stable` (floating branch, not even a version tag) — `.github/actions/setup-accelerator/action.yml:104`
    - `actions/checkout@v6` — every workflow
    - `actions/cache@v5`, `actions/cache/restore@v5`, `actions/cache/save@v5` — most workflows and `.github/actions/playwright-cache/action.yml:26,54`
    - `dorny/paths-filter@v4` — `accelerator.yml:23`, `sdk.yml:22`, `app.yml:22`, `publish-testnet.yml:30`, `publish-nightlies.yml:24`, `actionlint.yml:19`
    - `Swatinem/rust-cache@v2` — `.github/actions/setup-accelerator/action.yml:110`
    - `foundry-rs/foundry-toolchain@v1` — `.github/actions/setup-aztec/action.yml`
    - `opentofu/setup-opentofu@v1` — `actionlint.yml:84`
    - `oven-sh/setup-bun@v2`, `actions/setup-node@v6`, `actions/download-artifact@v8`, `actions/upload-artifact@v7` — throughout

---

## Finding 2: The CI/CD deploy role's S3 grant covers the whole bucket, so any of 4 differently-trusted pipelines can overwrite the auto-updater feed and any page on the live site

1. **Title**: Overly broad `s3:PutObject`/`s3:DeleteObject` scope on the shared CI role lets low-rigor deploy jobs touch the auto-updater feed and unrelated site prefixes

2. **Impact factors**: Integrity + Availability violated (Confidentiality not directly, since the bucket holds only public static assets); blast radius = **all users of the public site AND all desktop-app users who check for updates** (a single compromised job can deface the production domain or tamper with the update feed everyone's client reads). Data sensitivity: the `releases/latest.json` object specifically gates what version every installed desktop client is told is "latest" — a security-relevant control plane, not just a static asset. Exploitability: attack vector = network/adjacent (requires code execution inside one of the four AWS-credentialed CI jobs — see Finding 1, or a compromised build-time dependency of `packages/playground`/`packages/landing`), attack complexity = moderate, privileges required = none beyond what's needed to get code running in that one job, user interaction = none.

3. **Evidence confidence**: high on the policy over-breadth itself (directly read from Tofu); moderate on the "how a job actually gets compromised" leg (realistic mechanism, not a demonstrated live compromise in this repo).

4. **OWASP category + CWE**: OWASP A01:2021 – Broken Access Control. CWE-269 (Improper Privilege Management).

5. **Trace**:
   - Source: `infra/tofu/iam.tf:46-75` — the single `aws_iam_role_policy.ci` `Sid: "S3Deploy"` statement grants `s3:PutObject`, `s3:DeleteObject`, `s3:ListBucket`, `s3:GetBucketLocation` on `Resource: [aws_s3_bucket.site.arn, "${aws_s3_bucket.site.arn}/*"]` — i.e. **every object in the bucket**, no prefix condition (no `s3:prefix` / resource-ARN scoping to `landing/*`, `playground/*`, `playground-nightly/*` separately).
   - This one role (`aws_iam_role.ci`) is assumed, via OIDC, by four workflows of very different rigor:
     - `deploy-landing.yml:31-39` — pushes to `s3://…/landing/` on every `push` to `main` touching `packages/landing/**`, gated only by whatever the `main` PR required-status-checks caught.
     - `publish-testnet.yml:78-85` and `publish-nightlies.yml:69-76` — `workflow_dispatch`-triggered, build and push `packages/playground/dist/` to `s3://…/playground/` or `.../playground-nightly/`, after running `bun run --cwd packages/playground build` (a Vite build that imports and executes the full playground devDependency tree).
     - `release-accelerator.yml:815-824` — the only one of the four gated by the full e2e-webdriver + multi-platform smoke pipeline — writes `s3://…/landing/releases/latest.json`, the file the desktop app's Tauri updater fetches to decide "is there a newer version."
   - Sink: because the IAM grant is bucket-wide, a compromise of **any one** of the three lower-rigor jobs (e.g. via Finding 1's action-supply-chain vector, or a malicious/compromised transitive devDependency of `packages/playground`/`packages/landing` whose own module code runs the moment the Vite build imports it — this is NOT blocked by Bun's default `postinstall`-script gating, since no `trustedDependencies` list exists in the root `package.json` and the risk here is code that runs on **import/build**, not on `npm install` lifecycle hooks) can, once `aws-actions/configure-aws-credentials` has run earlier in that same job, issue `aws s3 cp`/`aws s3 rm` against `landing/releases/latest.json` or any other key — not just the prefix that job is nominally responsible for.

6. **Missing control**: Per-pipeline scoping — either separate IAM roles per workflow/prefix, or a session policy narrowing each `AssumeRoleWithWebIdentity` call to the one prefix that workflow legitimately needs (`s3:prefix` condition / resource ARN restricted to `landing/*` for `deploy-landing.yml`, `playground/*` and `playground-nightly/*` for the two publish workflows, and a distinct, more tightly held credential for `releases/latest.json` that only `release-accelerator.yml` can obtain). Also: neither `s3.tf` nor `cloudfront.tf` configure any access logging, so if this scope is ever abused there is no log trail to detect or investigate it — the only signal would be a visitor noticing a defaced page or a client failing its update check.

7. **Exploit/violation scenario**: Attacker achieves code execution inside the `deploy-app` job of `publish-nightlies.yml` (e.g. via Finding 1, or a compromised playground build-time dependency). After the job's `aws-actions/configure-aws-credentials` step runs (line 69), the attacker's code — which is still running in the same job/runner — calls `aws s3 cp` itself against `s3://aztec-accelerator-site/landing/releases/latest.json`, replacing it with JSON that reuses the **genuine, already-public** `.sig` and download URL of an older, legitimately-signed release (old release sigs are public GitHub-release assets, not secret — the attacker cannot forge a new signature since minisign verification is client-side against a pinned Ed25519 key baked into the app, and that control holds). Concrete achievable damage, reasoned precisely per the three realistic outcomes:
   - **Downgrade/version-pinning DoS**: any client below that stale version (fresh installs, machines that haven't updated in a while) is told the old build is "latest" and never learns a newer, possibly security-relevant release exists — a real suppression-of-patch attack, not a full rollback of already-updated clients (Tauri's updater only prompts when remote-version > local-version).
   - **Availability DoS of the update mechanism**: replacing `latest.json` with malformed/garbage content, or deleting it, breaks every client's update check outright.
   - **Site defacement / phishing**: the identical over-broad grant also covers `landing/*` and `playground/*` — the attacker can overwrite the production landing page or the playground SPA (which embeds a wallet UI) at the real, publicly trusted `aztec-accelerator.dev` / `playground.aztec-accelerator.dev` origins. This is the most severe concrete outcome of the over-broad grant: full content control at a trusted domain, independent of anything updater-related.

8. **Preconditions**: Code execution inside one of the three lower-rigor AWS-credentialed jobs (via a compromised Action tag per Finding 1, or a compromised playground/landing build-time dependency). No additional AWS-side privilege is needed once that precondition holds, because the IAM policy itself does not narrow scope per caller.

9. **Why existing mitigations fail**: SEC-02 (bb binary/digest share one GitHub trust plane) and SEC-03 (updater size cap read from the same feed it guards) are about the **content/size** of what the client verifies — they say nothing about who can write the feed object in the first place, and minisign verification (a real, effective control against content forgery) does not prevent the feed from being pointed at a different, older, still-validly-signed artifact, nor does it prevent deletion/corruption. This is a distinct, new angle: an availability/staleness attack on the control-plane object itself, enabled purely by IAM scope, not by any updater/crypto weakness.

10. **Instances**: `infra/tofu/iam.tf:54-66` (the over-broad resource list) is the single root cause; the four call sites that inherit it are `deploy-landing.yml:31`, `publish-testnet.yml:78`, `publish-nightlies.yml:69`, `release-accelerator.yml:809`. The missing-logging companion gap: no `logging_config` block anywhere in `infra/tofu/cloudfront.tf` or `infra/tofu/s3.tf`.

---

## Finding 3 (narrow, low-confidence — presented with explicit non-bypass caveats): OIDC trust-policy branch wildcards are broader than any live trigger needs, and the `nightlies` branch they also name has zero ruleset coverage

1. **Title**: `chore/aztec-*-*` wildcard entries in the OIDC trust policy are unused-but-assumable scope; the explicitly-named `nightlies` branch has no branch-protection ruleset at all

2. **Impact factors**: Would-be Integrity/Availability violation *if* ever exercised; blast radius today assessed as **none externally** — see below for why I could not complete a concrete external-attacker bypass. Exploitability: attack vector = local/adjacent (requires pre-existing repo write access), attack complexity = high (multiple missing preconditions must also be true — see field 9), privileges required = high (must already control a credential with `contents:write` on this repo), user interaction = none.

3. **Evidence confidence**: high on the facts (trust policy contents, ruleset scope, workflow triggers); low on there being a working exploit path today — I could not construct one and am flagging the scope-creep itself, not a demonstrated compromise.

4. **OWASP category + CWE**: OWASP A01:2021 – Broken Access Control / A05:2021 Security Misconfiguration. CWE-284 (Improper Access Control).

5. **Trace**: `infra/tofu/iam.tf:32-39` — the OIDC trust policy's `StringLike` condition on `token.actions.githubusercontent.com:sub` allows `refs/heads/main`, `refs/heads/nightlies`, `refs/heads/chore/aztec-nightlies-*`, and `refs/heads/chore/aztec-stable-*`. Cross-referencing:
   - `infra/rulesets/main-branch-protection.json:8` — the ruleset's `conditions.ref_name.include` lists **only** `refs/heads/main`. Neither `nightlies` nor any `chore/aztec-*` branch has *any* ruleset coverage — no required reviewers, no required status checks, no restriction on force-push or deletion.
   - The only workflow that creates `chore/aztec-nightlies-*` / `chore/aztec-stable-*` branches is `.github/workflows/_aztec-update.yml:91-141` (job `update`), whose effective `GITHUB_TOKEN` permissions are capped by the caller workflows (`aztec-nightlies.yml:15-17`, `aztec-stable.yml:15-17`) to `contents: write, pull-requests: write` only — **no `id-token: write`** is requested anywhere in `_aztec-update.yml`, so this job cannot itself mint an AWS session even though its ref would satisfy the trust policy.
   - The only workflows that actually call `aws-actions/configure-aws-credentials` (`deploy-landing.yml`, `publish-testnet.yml`, `publish-nightlies.yml`, `release-accelerator.yml`) trigger on `push: branches: [main]` or `workflow_dispatch` — none trigger automatically on `push`/`pull_request` scoped to a `chore/aztec-*` ref, and PR-triggered runs (`pull_request` event) get an OIDC `sub` of `repo:OWNER/REPO:pull_request`, not a ref-based `sub`, so they would not match the wildcard even if they requested `id-token` (which they don't — `accelerator.yml`/`sdk.yml`/`app.yml` request no `id-token` permission anywhere).
   - The GitHub App used for these branches (`RELEASE_BOT_APP_ID`/`RELEASE_BOT_PRIVATE_KEY`, minted via `actions/create-github-app-token` at `_aztec-update.yml:158-165` and `release-bot-token-check.yml:30-37`) is requested with exactly `permission-contents: write`, `permission-pull-requests: write`, `permission-issues: write`. It has **no `workflows` permission** (required by GitHub's Contents API to create/update anything under `.github/workflows/`) and **no `actions` permission** (required to `workflow_dispatch` an existing workflow against an arbitrary ref). So even a full compromise of this specific credential could not, by itself, inject a new id-token-requesting workflow step onto a `chore/aztec-*` branch, nor manually fire `publish-testnet.yml`/`release-accelerator.yml` against one.

6. **Missing control**: Least-privilege scoping of the trust policy to only the refs a live, automated deploy trigger actually uses (`main`, and dropping `nightlies`/the two wildcards unless something is later wired to need them), plus branch-ruleset coverage for `nightlies` matching what `main` already has (0-approval-but-required-status-checks is itself worth revisiting separately, but at minimum parity with `main` would close the biggest gap).

7. **Exploit/violation scenario** (why this remains a **gap**, not a demonstrated bypass): the only way I could find to actually exercise the wildcard's extra scope is a human who already holds `actions: write` on this repo manually choosing a `chore/aztec-stable-*` (or `nightlies`) ref in the `workflow_dispatch` UI/API for `publish-testnet.yml`/`publish-nightlies.yml`/`release-accelerator.yml` — which, in a single-owner repo, is the owner themselves, deploying unreviewed/unmerged code by deliberate choice. That is a foot-gun (deploying from a branch with zero required status checks and zero required review), not an attacker escalating privilege they don't already have. I am flagging the trust-policy scope-creep and the `nightlies` ruleset gap as latent risk: a **future** change (e.g., someone adding `id-token: write` to a job that fires on `push` to a `chore/aztec-*` branch, or later broadening the RELEASE_BOT App's installation permissions to include `workflows`/`actions`) would turn this into a real externally-reachable escalation with no additional review, because the trust policy and the branch ruleset are silently already permissive enough.

8. **Preconditions**: For any real exploitation: either (a) the repo owner deliberately dispatches a deploy workflow against a non-default ref, or (b) a future, currently-hypothetical workflow/App-permission change. Neither holds today.

9. **Why existing mitigations fail**: N/A in the sense that no SEC-0N control claims to cover this; I am explicitly **not** claiming a bypass of any documented mitigation — this is a novel, narrowly-scoped least-privilege observation, included because the cluster brief specifically asked for this chain to be traced to a concrete conclusion, and the honest conclusion is "not exploitable today given two independent missing preconditions, but broader than necessary and one workflow edit away from mattering."

10. **Instances**: `infra/tofu/iam.tf:32-39` (trust policy); `infra/rulesets/main-branch-protection.json:6-11` (ruleset scope limited to `main`); `.github/workflows/_aztec-update.yml` (branch creator, permissions capped by caller); `.github/workflows/aztec-nightlies.yml:15-17`, `aztec-stable.yml:15-17` (permission ceiling); `.github/workflows/release-bot-token-check.yml:33-37` and `_aztec-update.yml:161-165` (RELEASE_BOT App's actual requested scopes, missing `workflows`/`actions`).

---

## Not flagged (checked, no concrete bypass found)

- **Script injection via `${{ github.* }}` interpolation into `run:` blocks**: checked every workflow/action for direct interpolation of attacker-influenceable strings (`github.event.pull_request.title/body`, `github.head_ref`, `github.event.issue.*`, etc.) into shell `run:` steps. All instances found route untrusted-ish values through `env:` blocks (e.g. `INPUT_VERSION: ${{ inputs.version }}` then `$INPUT_VERSION` in the script), which is the correct pattern. No `pull_request_target` trigger exists anywhere in the repo. Non-finding.
- **npm publish via static `NPM_TOKEN` instead of OIDC Trusted Publishing** (`_publish-sdk.yml:97-101`): a real hardening opportunity, but there is no concrete source→sink trace of an exploited leak of this token in the current configuration (the token is only referenced inside the one `npm publish` step, after the build step has already completed, minimizing exposure window) — this is a "could use a stronger mechanism" observation without a demonstrated bypass, so per the audit's own rules it is not written up as a numbered finding.
- **CloudFront missing WAF**: the distribution forwards `query_string = false` and `cookies { forward = "none" }` (`cloudfront.tf:108-113`), i.e. there is no injectable server-side surface for a WAF to protect against on this static-SPA origin, and CloudFront gets AWS Shield Standard L3/L4 protection by default. No concrete attack scenario a WAF would stop here; not flagged as its own finding (the logging gap is folded into Finding 2 instead, since it's a detective control tied directly to that blast radius).
