# Playwright cache fix — kill the chronic CDN flake (light plan)

**Status:** planned · **Type:** CI reliability · **Codex audit:** session `019e7f2f` (design confirmed)

## Problem (root cause, verified)

App E2E jobs (`Local Network E2E`, `Mocked E2E`) install Chromium from
`cdn.playwright.dev` on every run. The cache **never hits**, so every run does a
fresh CDN download; when the CDN is slow the install times out (~21 min) and the
required job fails. Rerun usually passes (~2.5 min). Pure infra roulette — it has
blocked #241, #242, #243, and #231.

Why the cache never hits (codex + verified against the repo):

1. **Nothing warms `main`.** The 3 Playwright sites (`app.yml`, `_e2e-app.yml`,
   `accelerator.yml`) are all PR-gate / reusable workflows — none runs on `push`
   to `main`. GitHub scopes a PR's cache to that PR's own reruns; other PRs can
   only restore from `main`/base. With no cache ever written on `main`, **every
   PR cold-misses by construction.** (primary cause)
2. **Key too broad, no fallback.** `key: ${{ runner.os }}-playwright-${{ hashFiles('bun.lock') }}`
   busts on any lockfile change even though Playwright stays `1.58.2`, and there's
   no `restore-keys` fallback.
3. *(secondary)* failed installs save nothing; *(unproven)* big Rust caches may
   evict it under the 10 GB cap.

Mirror (`PLAYWRIGHT_DOWNLOAD_HOST`) just swaps CDNs on the required path;
pre-bake is heavier ops. **Fixing the cache is the real fix.**

## Plan

### 1. Shared composite action `.github/actions/playwright-cache/action.yml`
Centralizes the key so warm + PR jobs can never drift:
- **Compute version** from `package.json`/`bun.lock` → `1.58.2` (so the key
  auto-tracks Playwright bumps; no hardcoded version to go stale).
- **`actions/cache/restore@v5`** with `key: ${{ runner.os }}-playwright-<version>-shell`
  and `restore-keys: ${{ runner.os }}-playwright-` (fallback to the last good
  browser despite lockfile churn).
- **Install only on miss:** `if: cache-hit != 'true'` → the existing
  `timeout 600 bunx playwright install --with-deps --only-shell chromium`
  (retry ×2) stays as the fallback.
- Input `save` (default `false`): when `true`, `actions/cache/save@v5` after a miss.

### 2. New `warm-playwright-cache.yml`
- Triggers: `push: main` (so a merged Playwright bump repopulates), `schedule`
  weekly (a restore *touches* the cache → resets the 7-day eviction timer),
  `workflow_dispatch`.
- `runs-on: ubuntu-latest`, `permissions: { contents: read }`.
- One step: `uses: ./.github/actions/playwright-cache` with `save: true`.

### 3. Swap the 3 PR sites to the composite (restore-only)
`app.yml`, `_e2e-app.yml`, `accelerator.yml`: replace the `actions/cache@v5` +
inline install block with `uses: ./.github/actions/playwright-cache` (save
defaults false → restore + install-on-miss, **no save**).

## Security & adversarial considerations

- **Cache-poisoning / least privilege:** PR jobs **restore-only** (never save), so
  a malicious PR cannot overwrite the shared browser cache. The cache is written
  **only** by `warm-playwright-cache.yml` on `push: main` — i.e. only
  already-merged, trusted code. This is the correct trust model and is itself a
  security improvement over a save-on-PR setup.
- **Supply chain:** the browser still comes from Playwright's official CDN at warm
  time (same trust we already accept); the version-pinned key makes *which* browser
  we cache deterministic. No new secret, no new external host. `contents: read`
  only — no token escalation.
- **Integrity:** version in the key is derived from the committed lockfile, so a
  cache entry always matches the Playwright version the tests expect (prevents a
  stale-browser-vs-new-Playwright mismatch).

## Validation & rollout

- **Bootstrapping:** until the warm job runs on `main` once, PRs still cold-miss.
  Sequence: merge this PR → the `push: main` warm job populates the cache → the
  **next** PR (or a rerun) restores it.
- The PR that *adds* this will itself still flake (cold) — rerun if needed.
- **Confirm success:** on a post-merge PR, the Playwright step logs a cache **hit**
  and skips the CDN download (no `Downloading Chrome Headless Shell` line).
- Local: `actionlint` + `bun run lint:actions` on the new/edited workflows.

## Risks / notes

- `restore-keys` fallback could serve a slightly older browser after a version
  bump until the warm job re-runs — acceptable (Playwright is back-compatible for
  a patch, and the version key changes on the next bump anyway).
- Weekly schedule cadence must be < 7 days (GitHub eviction window). Weekly is fine.
- Keep the install fallback: if the cache ever genuinely misses, jobs still work
  (just slower) — no hard dependency on the cache existing.

## Codex implementation review (session 019e7f2f cont.)

Verdict: **works, no blocking findings** — warm/restore topology correct, PRs
can't poison `main`'s cache (PR caches scoped to `refs/pull/.../merge`), bootstrap
merge triggers the warm job, and `--only-shell` matches our headless test configs.

Applied:
- **[FRAGILE]** version resolver: switched from `grep bun.lock` (format-coupled +
  the error branch was dead under `set -e`) to `bunx playwright --version` (callers
  all run `bun install` first) with `|| true` so a no-match hits the error branch.
- **[NIT]** cron Mon→Mon+Thu (margin against scheduler delay vs 7-day eviction).
- **[NIT]** documented the composite as Linux/ubuntu-only.
Kept (codex called defensible): always-run `--with-deps` (deps don't persist on
ephemeral runners; `playwright install` skips the browser redownload on a hit).
