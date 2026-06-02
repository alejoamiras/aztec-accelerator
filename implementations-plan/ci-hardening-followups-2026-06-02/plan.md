# CI hardening follow-ups: PAT retirement + path-filter md-skip + supply-chain min-age

## Context
Three follow-ups surfaced by the 1.0.3 audit, bundled into one light plan (plan → one codex review → implement). They're independent; each ships as its own PR.

1. **PAT_TOKEN deprecation** — the release path already moved off the long-lived `PAT_TOKEN` (bump-source uses a GitHub App token), but the @aztec bump workflows still use it. Migrate them to the same App token, then delete the PAT.
2. **(a) path-filter in-subtree md-skip** — the B2 fix (deleting the `!**/*.md` negations) fixed docs OUTSIDE packages, but a markdown-only edit *inside* a package still runs the full ~15-min gate. Use dorny `predicate-quantifier: every` + `any:` groups to skip those too. *(User chose to do this despite the skip recommendation — it is the high-blast-radius item; validate hard.)*
3. **(b) supply-chain min-age** — the stated 7-day `minimumReleaseAge` default is configured NOWHERE in the repo. Add it.

**Status / codex review outcome (2026-06-02):** codex verdict on the plan = *rework*, because **(a) as designed was syntactically broken** — the `any:` object isn't a valid dorny boolean group (it's parsed as a `{changeType: pattern}` status filter), so under `every` the filter would be false for everything → **real code changes would skip the gate**. Correct form needs two separate dorny steps + `desktop_code || desktop_shared` across 8 jobs — meaningful complexity + danger for a tiny payoff. **→ (a) DROPPED.** **(b) shipped** (PR #262; verified bun reads+enforces the bunfig key, frozen-lockfile unaffected). **Part 1 (PAT)** proceeding with codex's fixes (App `issues:write`; only `create-pr` migrated — the `update` job's push already uses the default token, not the PAT).

---

## Part 1 — PAT_TOKEN deprecation
**Verified usage:** only `_aztec-update.yml` (reusable) consumes the PAT — `GH_TOKEN` for `gh pr create`/`gh pr merge` (line 149) + the checkout/push that opens the @aztec-bump PR — and `aztec-stable.yml` + `aztec-nightlies.yml` pass it via `secrets: { PAT_TOKEN }`. Same shape as `bump-source`, so the release-bot App token covers it.

**Steps:**
- `_aztec-update.yml`: change the `secrets:` block from `PAT_TOKEN` → `RELEASE_BOT_APP_ID` + `RELEASE_BOT_PRIVATE_KEY` (both `required: true`). In each job that uses the token, add `actions/create-github-app-token@bcd2ba49218906704ab6c1aa796996da409d3eb1 # v3.2.0` (with `permission-contents: write` + `permission-pull-requests: write`) and use `steps.app-token.outputs.token` for the checkout `token:` and `GH_TOKEN:`.
- `aztec-stable.yml` + `aztec-nightlies.yml`: pass the two App secrets instead of `PAT_TOKEN`.
- **Confirm App scope:** @aztec updates touch deps (`package.json`/lockfiles), not `.github/workflows/**`, so `contents`+`pull_requests` write suffice. If any path under the update could touch a workflow file, add `workflows: write` to the App (and `permission-workflows: write` to the mint) — verify first.
- **Validate (TESTABLE, unlike bump-source):** `workflow_dispatch` `aztec-nightlies` after the swap → confirm it opens the @aztec PR via the App token and the PR triggers CI.
- **Only after a green dispatch:** delete the `PAT_TOKEN` secret + the underlying PAT.

## Part 2 — (a) path-filter in-subtree md-skip
**Mechanic:** set `predicate-quantifier: every` on the `dorny/paths-filter` step in `accelerator.yml`, `app.yml`, `sdk.yml`. Under `every`, a file matches a filter iff it matches EVERY top-level entry — so restructure each filter to one `any:` group (the positive allowlist) plus the `!<subtree>/**/*.md` negation(s):
```yaml
# accelerator.yml desktop (integration analogous, with packages/sdk/src/**)
desktop:
  - any:
      - 'packages/accelerator/**'
      - 'packages/sdk/package.json'
      - 'biome.json'
      - 'package.json'
      - 'bun.lock'
      - '.github/workflows/accelerator.yml'
      - '.github/workflows/_e2e.yml'
      - '.github/workflows/_e2e-webdriver.yml'
      - '.github/actions/**'
  - '!packages/accelerator/**/*.md'
```
Semantics: `(in allowlist) AND (not an accelerator md)`. app/sdk `relevant` get an `any:` group + `!packages/playground/**/*.md` and/or `!packages/sdk/**/*.md`.

**RISK (the reason I flagged skipping):** a *flat* `every` over the current mixed list would require a file to match EVERY path → skips almost everything (real code changes would skip the gate). The `any:`-group wrapper is mandatory and must be exactly right.

**Validate via the `changes`-job output (fast — read desktop/integration, don't wait for the full gate) on throwaway PRs:**
| change | expect |
|---|---|
| `packages/accelerator/src/**` (code) | desktop **true** (gate RUNS) |
| `packages/accelerator/**/*.md` only | desktop **false** (gate SKIPS) ← the new behavior |
| `implementations-plan/**` docs | all **false** |
| `biome.json` | desktop **true** |
| `packages/sdk/src/**` only | desktop **false**, integration **true** |

Only merge once all five match. (Reuse the lint-gate test pattern: branch → trivial change → open PR → read the changes job → close.)

## Part 3 — (b) root bunfig minimumReleaseAge
New `bunfig.toml` at repo root:
```toml
[install]
minimumReleaseAge = 604800  # 7 days — refuse dep versions published < 7d ago (supply-chain)
```
**Verify:** (1) it applies workspace-wide (Bun merges root + nearest bunfig; the playground/sdk bunfigs don't set it → inherit); add it to those two if a sub-dir install doesn't pick up root. (2) `bun install --frozen-lockfile` still succeeds (the gate filters *resolution* of new/changed deps, not already-locked ones) — run it locally before merge.

---

## Security & adversarial considerations
- **PAT retirement** (least-privilege): removes a long-lived account-bound credential from the @aztec automation; the App token is ~1h TTL + installation-scoped, minimal perms. Attacker value of a leaked CI credential drops sharply.
- **(a)** the single adversarial risk is a filter misconfig that lets *real code changes skip the gate* (ungated merge to a protected branch). Fully mitigated only by the validation matrix above run BEFORE merge — non-negotiable.
- **(b)** supply-chain hardening: the 7-day min-age is the documented defense against freshly-published malicious dep versions (the @tanstack-worm class). Closes a stated-but-unenforced policy.
- No new secrets beyond the already-present App secrets; no crypto changes.

## Validation summary
- **Part 1:** `workflow_dispatch aztec-nightlies` → PR opens via App token + triggers CI (then delete PAT).
- **Part 2:** the 5-row `changes`-output matrix via throwaway PRs.
- **Part 3:** local `bun install --frozen-lockfile` succeeds with the new config.

## Implementation order
1. **(b)** bunfig — trivial, independent → PR.
2. **(a)** path-filter refactor → PR, gated behind the 5-row validation.
3. **PAT retirement** → PR → dispatch-validate → delete the PAT secret + token.

See [eli5.html](eli5.html).
