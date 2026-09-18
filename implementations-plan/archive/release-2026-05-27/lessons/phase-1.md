# Phase 1 — PR A (Automation deprecation + workflow fixes)

## Final scope (post-audit-revision)

Six sub-tasks all in one PR `chore/deprecate-automation-and-fix-release-workflow`:

- **A.1** Deprecate scheduled triggers
  - `aztec-stable.yml`: drop `on.schedule`, set `auto_merge: false`
  - `aztec-nightlies.yml`: drop `on.schedule`, set `auto_merge: false`
  - `backport-nightlies.yml`: deleted entirely
  - `publish-testnet.yml`: drop `on.push`
  - `publish-nightlies.yml`: drop `on.push`
- **A.2** Fix `scripts/get-sdk-publish-version.ts` for stable bases — use `-revision.N` suffix instead of `.N` (which produces invalid semver). Added 4 new test cases covering stable bases.
- **A.3** Restructure `release-accelerator.yml`:
  - Reordered job graph: `validate → e2e-webdriver → [build, build-headless] → smoke → tag → release → bump-source`
  - Added `is_prerelease` output to `validate` (computed via `[[ "$INPUT_VERSION" =~ - ]]`)
  - Gated `latest.json` generation step, S3 upload + CloudFront invalidation steps, and the entire `bump-source` job on `is_prerelease == 'false'`
  - `gh release create` branches on `IS_PRERELEASE` env var: `--prerelease --latest=false` vs `--latest`
- **A.4** ARM64 `build-headless` job `timeout-minutes: 30 → 45`
- **A.5** `accelerator.yml` paths filter: added `packages/sdk/package.json`
- **A.6** `CLAUDE.md`: updated CI listing + release pipeline summary

## Notable findings during execution

1. **Opus and codex both wrong on one detail**: opus said `aztec-nightlies.yml` "doesn't pass `auto_merge`" and codex inherited the same assumption. **Actually**, both `aztec-stable.yml` (line 30) and `aztec-nightlies.yml` (line 31) explicitly set `auto_merge: true`. Fix applied (set false in both) but the reasoning recorded.

2. **actionlint SC2129 style fix**: my first version of the `validate` step had multiple `echo X >> "$GITHUB_OUTPUT"` lines in sequence. shellcheck flagged the style (SC2129: prefer `{ ... } >> file`). Grouped into a single redirect block.

3. **Test count after A.2**: scripts/get-sdk-publish-version.test.ts went from 6 → 10 tests. All passing.

## Validation

- `bun run lint` — clean
- `bun run lint:actions` — clean (after SC2129 fix)
- `bun run test` — 73 tests pass
- `bun test scripts/get-sdk-publish-version.test.ts` — 10 tests pass

## Next phase

Commit PR A, push, open PR, wait for green CI.
