# Release-CI hardening — owner runbook (codex audit #9)

Item #9 ("dispatch-ref secret exposure; tag==github.sha; version-output-injection") splits into
**code (done in the YAML)** and **repo-settings (only the owner can apply, in GitHub)**.

## Done in `release-accelerator.yml` (validated with actionlint)
- **tag == github.sha**: the `tag` job asserts `HEAD == github.sha` before tagging, and refuses to
  proceed if a pre-existing tag points at any other commit. The pushed tag can only ever target the
  exact commit the whole build/smoke/e2e pipeline validated.
- **version-output-injection**: already hardened (pre-existing) — `inputs.version` enters via an
  `env:` var (never interpolated into the script body) and is regex-gated to strict semver
  `^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$` BEFORE being written to `GITHUB_OUTPUT`. No shell- or
  output-injection is possible (the charset forbids newlines/metacharacters). Verified, no change
  needed.

## MUST be applied by the owner in GitHub settings (I cannot, and should not, do these in YAML)
The real dispatch-ref secret-exposure fix is a repo-settings control, because `workflow_dispatch`
can target ANY ref, and the release pipeline holds prod secrets (Ed25519 signing key across
`build` / `sign-update-feed` / `update-smoke*`; AWS OIDC in `release-auth-preflight` / `release`).
Someone with dispatch + branch-push could otherwise run the pipeline against a malicious ref and
exfiltrate those secrets.

1. **Protected `release` environment with required reviewers.** Create a GitHub Environment (e.g.
   `release`), add required-reviewer protection, and move EVERY secret-bearing job onto it
   (`environment: release`). Partial coverage is not enough — the signing key also lives in
   `build`/`update-smoke*`, so all of them must be gated or the exfil path stays open. Doing this in
   YAML is deferred to the owner precisely because it must be paired with the environment's
   protection rules (a bare `environment:` with no rules is not mitigation) and it touches the whole
   release matrix (untestable here without a real dispatch).
2. **Restrict who can run `workflow_dispatch`** on `release-accelerator.yml` (limit write access /
   use a CODEOWNERS-gated deploy key), OR require the dispatch ref be an immutable, already-reviewed
   tag/SHA rather than an arbitrary branch.
3. Cross-refs the CONVERGED-SCOPE NEEDS-THE-OWNER items: **legacy-role retirement + protected
   environments** (C5 runbook) and **legacy-CA bounded rotation** — same "runbook is not mitigation
   until applied" caveat.

---

# Full-branch audit (2026-07-24) — owner-applied supply-chain items

The whole-branch codex audit (`full-audit-2026-07-24/FINDINGS.md`) surfaced four items that are
owner-applied (CI secret hygiene + infra), beyond the two code fixes already landed (F2, G2) and the
in-code hardening (A1, B1/B2, H3). Ordered by priority.

## F1 / G1 — production signing key exposed to smoke-test code (High/Critical)
**Files:** `_e2e-updater{,-linux,-windows}.yml`, `release-accelerator.yml` (sign job),
`packages/accelerator/scripts/sign-smoke-feed.sh`, `updater-smoke-windows.ps1`.
**Issue:** the updater smoke jobs export `TAURI_SIGNING_PRIVATE_KEY` (+ password) into the environment
and then (a) run repo-controlled scripts / `bunx tauri`, and (b) install + LAUNCH the N-1 release
artifact. Child processes inherit the secret env; a compromised N-1 asset or CLI dependency could read
and exfiltrate the production signing key — which defeats updater authenticity for every installed
client.
**Fix (owner):**
1. Generate an EPHEMERAL smoke keypair per run; build/patch the N-1 fixture with its PUBLIC key, and
   sign the smoke feed with the ephemeral PRIVATE key. The production key never enters a smoke job.
2. Keep the production key ONLY in an isolated, protected release-signing job (or an HSM/OIDC signer)
   that does NOT check out branch code, build, or run `bunx` fallback resolution — invoke a frozen,
   digest-pinned signer binary. Pass the signed manifest downstream as an artifact.
3. Never launch an application with signing secrets in its environment.
This pairs with the **protected `release` environment** (item #1 above) — do them together.

## H2 — legacy IAM role defeats the new per-pipeline isolation (High)
**File:** `infra/tofu/iam.tf` (legacy role, ~lines 249-275).
**Issue:** the retained legacy role trusts every workflow on `main` and keeps whole-bucket write. A
compromised action in the OIDC-enabled landing-deploy job can assume it and overwrite
`landing/releases/latest.json` — suppressing security updates or corrupting the feed. The new
landing-role's release-feed deny does NOT apply to the legacy role.
**Fix (owner):** cut over and DELETE the legacy role atomically; or, as an interim, restrict it to the
landing prefix with the SAME explicit release-feed deny the new roles carry. Do not merge to `main`
with the broad legacy role still active. (This is the CONVERGED-SCOPE legacy-role-retirement item.)

## H4 — IAM OIDC condition may be unassumable after cutover (release-blocking correctness)
**File:** `infra/tofu/iam.tf` (three new roles condition on both `token.actions.githubusercontent.com:sub`
AND `:workflow`).
**Issue:** codex asserts AWS will not evaluate the GitHub `workflow` OIDC claim as a condition key, so
after cutover the `StringEquals` on `:workflow` never matches and the roles become **unassumable**
(release pipeline breaks). `:sub` is the load-bearing, AWS-canonical claim regardless. My confidence is
only moderate that codex is right (AWS *does* support custom claims present in the token, and `workflow`
IS in GitHub's token) — but this must be **verified at the human-gated cutover** before relying on it.
**Fix (owner):** the robust, AWS-canonical pattern is to scope on `:sub` via **GitHub Environments**
(which appear in `sub` as `repo:OWNER/REPO:environment:<env>`) and DROP the fragile `:workflow` condition
— which also delivers the per-pipeline isolation item #1 wants. This ties H4 + F1/G1 + item #1 into one
"create protected environments, scope roles on `sub`" change. Verify assumability with a dispatch-time
dry-run before removing the legacy role.

## Note on H1 (branch protection 0-approvals) and the updater buffer (D1/D2)
- **H1** (`infra/rulesets/main-branch-protection.json` `required_approving_review_count: 0`) is the
  solo-dev risk-accept decision recorded above — revisit when a second maintainer joins.
- **D1/D2** (updater unbounded feed/artifact buffer) remain the accepted availability-only residual
  #345/M6; the fix (bounded streaming inside the plugin) needs an upstream/pinned-fork change and was
  rejected in a prior round as making hand-written verification the sole authenticity control.
