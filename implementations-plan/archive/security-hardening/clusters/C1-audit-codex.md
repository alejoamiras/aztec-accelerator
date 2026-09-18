# C1 plan-audit — Codex gpt-5.6-sol @ xhigh

Verdict: **REJECT** → folded into the revised plan + implementation.

## Adopted
- Argument-injection: use `--tag="$DIST_TAG"` (=-form) + regex must start with a letter `^[a-z][a-z0-9._-]*$` (rejects leading `-` option-injection + semver-like tags).
- Validator robustness: env-only bash `[[ "$DIST_TAG" =~ ^...$ ]]` (whole-string, not grep-per-line) with `LC_ALL=C`; no `printf "$VAR"` format-string bug.
- Broaden scope within the file: route ALL run:/node interpolations through env + quoted refs — L80 (AZTEC_VERSION arg), L89 (node reads process.env.SDK_VERSION), L101 (DIST_TAG), L108 (SDK_VERSION), L114/115 + L124/126 (release step SDK_VERSION/AZTEC_VERSION/DIST_TAG). 'Computed output is not a trust boundary.'
- Validation is the FIRST step (before checkout), since job perms + checkout-persisted creds are live from step 1.
- Fact 6 corrected: the regex is a PROJECT-policy allowlist, not npm's charset.
- Proof: exact-context grep over run:/node blocks (not just the run: line) + enumerate test cases (empty, multiline, --dry-run, semver-like, shell payload).

## Deferred / out-of-scope (documented, not silently dropped)
- **persist-credentials:false / split unprivileged-build from privileged-publish** — bigger refactor; `git push` currently relies on checkout-persisted creds. Validate-first already blocks a malicious dist_tag before any step runs. Tracked as a follow-up hardening.
- **Trusted publishing / protected environment / remove direct workflow_dispatch / restrict actors** — org+release policy beyond F-006 (closing the injection). Tracked.
- **SHA-pin actions + mutable bun-version/rust** — that is cluster C3 (F-015).
- **Adjacent correctness (not F-006):** `git tag/push … || true` swallows failures; the `latest` input descriptions say GitHub `--latest` but the step hardcodes `--latest=false`. Noted for a follow-up; not fixed in this security cluster.
- **Semver-validate the computed versions** — env-quoting already makes them inert shell data (closes the injection, which is F-006); value-validation is robustness DiD, deferred.

---

## Full verdict transcript
```
reject (with blocking findings: leading-option values survive the allowlist, validation/proof is not newline-safe, and unvalidated computed outputs remain interpolated in token-bearing shell)

- [`^[a-z0-9._-]+$` plus `--tag "$DIST_TAG"`](implementations-plan/security-hardening/clusters/C1-workflow-input-hardening.md:12) does not fully close argument injection. It permits `--dry-run`, `--force`, `-ddd`, etc.; npm parses these as options when passed separately after `--tag`. Require `--tag="$DIST_TAG"` and either an exact enum (`testnet|nightlies|latest`) or at least `^[a-z][a-z0-9._-]*$`.

- The regex is not “npm’s legal charset.” It accepts values npm rejects as semver ranges, including `1.2.3` and `v1.4`; npm explicitly documents this restriction. It also excludes potentially valid uppercase tags. It is compatible with the three known project tags, but that is a project policy, not npm validation. [npm dist-tag documentation](https://docs.npmjs.com/cli/v11/commands/npm-dist-tag/)

- A validator implemented as `printf '%s\n' "$DIST_TAG" | grep -Eq ...` is bypassable with a multiline input containing one valid line. `printf "$DIST_TAG"` additionally creates a format-string bug. Use an env-only Bash `[[ ... =~ ... ]]`/`case` check with `LC_ALL=C`; a composite action is unnecessary. GitHub recommends the intermediate-environment-variable pattern. [GitHub secure-use guidance](https://docs.github.com/en/actions/reference/security/secure-use)

- Step-level env plus quoted expansion safely closes shell evaluation at [publish L101](.github/workflows/_publish-sdk.yml:101) and release-notes L124/L126. Variable contents are not recursively parsed; the escaped backticks remain literal.

- L104 is safe: `if: inputs.latest && inputs.dist_tag != 'latest'` is structural GitHub expression evaluation, not generated shell.

- L108 must not remain optional. “Computed output” is not a trust boundary: the version derives from checked-out `package.json`, repo code, and registry data. Raw expressions also enter shell at [L80/L89](.github/workflows/_publish-sdk.yml:80), [L108](.github/workflows/_publish-sdk.yml:108), and [L114/L115](.github/workflows/_publish-sdk.yml:114); L108 carries `NPM_TOKEN`, while L114/L115 carry `GH_TOKEN`. Move all outputs through env, quote them, validate versions as semver, and make the Node script read `process.env.SDK_VERSION`.

- The proposed grep proof only detects expressions on the `run:` line; it misses block-scalar bodies such as current L124/L126. Actionlint does not prove injection safety. Add exact-context inspection plus cases for empty, multiline, `--dry-run`, semver-like, and shell-payload inputs.

- Make validation the first step, before checkout. “Before Install/Build” is not equivalent to “before token-bearing”: `contents: write` and `id-token: write` are job-wide, and checkout receives/persists GitHub credentials by default. Split unprivileged build from npm publish and GitHub release, or at minimum use `persist-credentials: false` outside the release step.

- Facts 1–5 are accurate; Fact 6 is misstated. The backtick inference is sound. No other `inputs.dist_tag` shell interpolation exists in this target; current reusable-workflow callers use literal `testnet`/`nightlies`.

- “Asks: none” silently assumes arbitrary manual tags remain supported, direct dispatch is necessary, arbitrary refs may publish, and computed versions are trusted. Those are unresolved policy decisions.

- Stronger boundary: remove direct `workflow_dispatch` from the reusable workflow or expose choice-only wrappers, restrict publishing to a protected ref/environment with approval, and restrict workflow actors. By default, repository write access can manually dispatch workflows. [GitHub manual-dispatch documentation](https://docs.github.com/en/enterprise-cloud%40latest/actions/how-tos/manage-workflow-runs/manually-run-a-workflow)

- Supply-chain gaps remain: mutable action tags, `bun-version: latest`, privileged build/install steps, and a long-lived npm token. Prefer npm trusted publishing with token publishing disabled, environment binding, immutable action SHAs, and separated privileges. Provenance links a package to its workflow; it does not establish benign contents. [npm trusted publishing](https://docs.npmjs.com/trusted-publishers/), [npm provenance limitations](https://docs.npmjs.com/generating-provenance-statements/)

- Missed integrity/correctness issues: both `latest` input descriptions claim GitHub `--latest`, while the workflow hardcodes `--latest=false`; `git tag` and `git push` failures are swallowed with `|| true`.```
