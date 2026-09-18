# C1 — workflow-input-hardening (F-006) · /blueprint light

**Cluster:** C1 of the security-hardening campaign · **Branch:** `sechard/workflow-input-hardening` off `security-hardening` · **Tier:** light.

## Summary
`.github/workflows/_publish-sdk.yml` takes a `workflow_dispatch` input `dist_tag` and interpolates it **unquoted** into `run:` shell steps that carry `NPM_TOKEN` and `GH_TOKEN`. Because GitHub expands `${{ … }}` **before** bash runs, a dispatch-capable actor can set `dist_tag` to a shell payload and exfiltrate the tokens or publish a malicious package (F-006, CWE-78). Fix: validate `dist_tag` against a strict allowlist and pass it via `env:` referenced quoted, so it is inert shell data.

## Tier justification (Phase 0.5 rubric)
Novelty L · Blast-radius (publish pipeline → all SDK consumers) — but the FIX is a well-known, contained pattern · Irreversibility L (workflow edit, reversible) · Migration L · External-coupling L · Security-sensitivity **H** (token-bearing publish). 1 high, single file, single finding, known fix → **light**.

## Phase 1 — validate + env-quote `dist_tag` (single phase)
1. Add an early **Validate dist_tag** step (before Install/Build/Publish — i.e. before any token-bearing step) that reads `dist_tag` via `env:` and fails unless it matches `^[a-z0-9._-]+$` (npm dist-tag legal charset; forbids whitespace, `;`, backticks, `$`, quotes).
2. Publish step (L101): move `dist_tag` to `env: DIST_TAG: ${{ inputs.dist_tag }}` and use `--tag "$DIST_TAG"` (quoted, no `${{ }}` in `run:`).
3. Release-notes step (L110-136): the step already sets `GH_TOKEN`; add `DIST_TAG` to its `env:` and replace the two `${{ inputs.dist_tag }}` occurrences in `NOTES=` (L124/126) with `"$DIST_TAG"`.
4. Leave L104 `if: inputs.latest && inputs.dist_tag != 'latest'` unchanged — GHA **expression** context, not shell; no injection. (Validation upstream makes it moot regardless.)
5. Consider L108 `npm dist-tag add "…@${{ steps.sdk-version.outputs.version }}"` — a COMPUTED output (not user input) from `get-sdk-publish-version.ts`; low risk, but env-quote it too as cheap defense-in-depth if Codex concurs.

### Validation gate (Phase 1)
- **Commands:** `bun run lint:actions` (actionlint over the edited workflow).
- **Pass criteria:** exit 0; no `${{ inputs.dist_tag }}` remains inside any `run:` block (grep proof: `! grep -nE 'run:.*\$\{\{ *inputs.dist_tag' … `); the validate step precedes every token-bearing step.
- **Layers:** lint (actionlint). CI: `actionlint.yml` green on the PR into `security-hardening`.

## Security & Adversarial Considerations
- **Threat model:** actor with `workflow_dispatch`/write capability (collaborator or a compromised owner/CI token — this is a single-owner repo, so not an anonymous outsider) runs `_publish-sdk.yml` with a crafted `dist_tag` → shell injection in a token-bearing step → `NPM_TOKEN`/`GH_TOKEN` exfiltration or malicious publish.
- **Least privilege:** unchanged (job perms `id-token: write` + `contents: write` are required for provenance + release; not widened).
- **Input validation:** the core fix — strict `^[a-z0-9._-]+$` allowlist at the trust boundary, before any secret is in scope.
- **Supply chain:** npm publish stays `--provenance` (OIDC-attested); the fix prevents hijacking that identity via injection.
- **Crypto:** none.
- **Residual:** a dispatch-capable actor can still trigger a *legitimate* publish (that's the workflow's purpose); F-006 is only about denying **arbitrary shell**. Restricting who can dispatch is org/branch-protection policy, out of this cluster's scope.

## Assumptions
**Facts (verified):**
1. `_publish-sdk.yml` declares `workflow_dispatch` with a `dist_tag` string input (L3-10) and is also `workflow_call`able.
2. L101 `run: npm publish … --tag ${{ inputs.dist_tag }} …` runs with `env: NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}` (L99-101).
3. L124/126 interpolate `${{ inputs.dist_tag }}` into a double-quoted `NOTES=` in a step carrying `env: GH_TOKEN: ${{ github.token }}` (L110-127).
4. Local lint gate is `actionlint` via `bun run lint:actions` (root package.json script).
5. CI gate `actionlint.yml` runs on `pull_request: [main, security-hardening]` (post-C0).
6. `dist_tag` is an npm dist-tag; npm dist-tags are constrained (no spaces; not a valid semver) — `^[a-z0-9._-]+$` is a safe superset of the project's real tags (`testnet`, `nightlies`, `latest`).

**Inferences (unverified):**
- The release-notes backtick usage `\`${AZTEC_VERSION}\`` is already escaped/literal; adding `"$DIST_TAG"` won't interact with it. (Attack in audit.)
- No other workflow interpolates `inputs.dist_tag` into shell (scope stays single-file). (grep to confirm.)

**Asks (user):** none — fix is fully determined by the finding; autonomous per the campaign goal.

## Seeds
Not applicable per-cluster — the campaign-level `/goal` + `/loop` (in `../plan.md`) drive this. This cluster is one loop iteration.
