# CI Speed — fix the missing cache + Playwright reliability

**Status**: v2 (consolidated: main + codex xhigh + opus subagent) — pending final codex pass + approval
**Date**: 2026-05-29
**Type**: Tier B (contained CI infra), audited as Tier A alongside updater-validation.
**Scope (user)**: high-ROI, low-risk only. No sccache / LTO restructuring / release-matrix surgery.

## Goal

Cut PR-gate wall time (the confirmed uncached server build) and stop the recurring Playwright-install hang. Measure before/after.

## Confirmed findings

1. **The missing cache:** `_e2e.yml` builds the headless server *from* `packages/accelerator/server` (`cargo build`, working-dir server) but its `Swatinem/rust-cache` workspace is the stale `packages/accelerator/src-tauri -> target`. Since `_e2e.yml` builds only from `server/`, Cargo writes **all** artifacts (including the `src-tauri` path-dep) under `packages/accelerator/server/target` — so the cached dir and the built dir differ → fully uncached. Evidence: accelerator.yml `e2e` (build_accelerator) = **5m40s** vs sdk.yml SDK E2E (no accelerator) = **1m56s**.
2. **Only `_e2e.yml` is stale.** Both codex + opus audited every other Rust build: all go through `setup-accelerator` (caches both `src-tauri` and `server`). `_e2e.yml` is the lone drifted caller (it hand-rolls `dtolnay/rust-toolchain` + `rust-cache` instead of the composite).
3. **Playwright hang ≠ missing cache.** `_e2e-app.yml` has the `~/.cache/ms-playwright` cache + cache-hit gate; the hang is `bunx playwright install --with-deps chromium` stalling (apt `--with-deps` and/or CDN) on cache-miss. Cache keys are also **fragmented**: `accelerator.yml` keys on `packages/accelerator/package.json`; `app.yml` + `_e2e-app.yml` on `packages/playground/package.json` — both pin `@playwright/test ^1.58.2`.

## Workstreams

### WS-S1 — Fix the `_e2e.yml` server cache [the win]

In `_e2e.yml`, change the rust-cache to cache **only** the dir actually built:

```yaml
- uses: Swatinem/rust-cache@v2
  if: inputs.build_accelerator
  with:
    workspaces: packages/accelerator/server -> target
    key: e2e-accelerator
```

(Was `packages/accelerator/src-tauri -> target`.) **Cache `server -> target` only** — codex's correction over "mirror the composite": this job never builds *from* `src-tauri/`, so `src-tauri/target` is never written here; caching it is dead weight. The path-dep's artifacts live under `server/target/.../deps` and are covered. Distinct `key: e2e-accelerator` so it doesn't collide with composite caches. Confirm the cache **saves on `main`** (PR branches only restore from base-branch caches).

**Decision — targeted fix, not the composite refactor.** Opus argued for factoring a shared `setup-rust-cache` composite (toolchain + cache) used by both `_e2e.yml` and `setup-accelerator`, to root-cause the drift. Valid, but it's a broader refactor; per the user's low-risk directive we take the one-line correct fix now and record the shared-composite extraction as a **future follow-up** (with a `# NOTE:` in `_e2e.yml` pointing at the composite so the next editor keeps them in sync).

### WS-S2 — Playwright install reliability

- **One shared, version-precise cache key.** Replace the fragmented `hashFiles(...package.json)` keys with a key derived from the **resolved Playwright version** (codex: explicit version is cleaner than `bun.lock`). Add a tiny step that reads `@playwright/test` from `bun pm ls`/lockfile into `PLAYWRIGHT_VERSION`, key `${{ runner.os }}-playwright-${PLAYWRIGHT_VERSION}`. Apply to all 4 install sites (`accelerator.yml` desktop-ui, `app.yml` ×2, `_e2e-app.yml`).
- **Retry + per-step timeout (mandatory).** Wrap the install in 3× retry with backoff AND a step-level `timeout-minutes: 5`, so a stalled install fails fast into a retry instead of hanging to the 45-min job timeout. This is the direct fix for the chronic hang.
- **Consider `--only-shell chromium`** (codex): both Playwright configs use Playwright-managed headless Chromium (no `channel`/`executablePath`), so the smaller headless shell suffices and downloads faster. Validate it doesn't change the test surface.
- **Do NOT rely on runner-shipped Chromium** (codex): configs use Playwright-managed browsers, so preinstalled system Chromium is irrelevant. Keep the browser cache, justified as **hang-avoidance** (Playwright docs note caching isn't usually a raw-speed win).

### WS-S3 — Cache-coverage audit [done]

Both auditors confirmed `_e2e.yml` is the only stale rust-cache; all composite-based jobs (smoke, release-smoke, build, build-headless, webdriver) are correctly cached. No other missing caches. Recorded; no further change.

### WS-S4 — Measurement

Record before/after warm-cache durations (accelerator.yml `e2e`, SDK E2E, Local Network E2E, full PR-gate wall) in `lessons/`. First run after a cache-key change is cold — compare **second** runs. Success: accelerator `e2e` drops ~3 min warm; Playwright hang rate → ~0.

## Security & Adversarial Considerations

- Distinct cache `key:` per build kind — no cross-job cache poisoning; scope unchanged (per-repo).
- Retry/ timeout don't change trust surface (same CDN, same `--with-deps`). Browser binaries remain un-hash-pinned (out of scope; note as future supply-chain hardening).
- No secrets, no release-path changes. Pure CI wiring.

## Rollback

Revert the PR. No artifact/behaviour impact.

## Sequencing

Single PR (`ci/speed-caches`). Independent of the updater-validation plan.

## Open questions (final pass)

1. Confirm the `PLAYWRIGHT_VERSION` extraction one-liner works across `bun` (vs reading `package.json` `^1.58.2` directly — the caret means resolved version may differ; prefer lockfile-resolved).
2. `--only-shell` vs full chromium — verify against both playwright configs before switching.
