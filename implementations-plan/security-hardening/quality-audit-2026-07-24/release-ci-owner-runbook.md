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
