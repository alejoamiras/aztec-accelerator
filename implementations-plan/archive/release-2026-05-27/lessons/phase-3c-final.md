# Phase 3c — `1.0.1` stable: SHIPPED

## Sequence as executed

1. **`-rc.3` dry-run** (run 26537134332) — all 13 jobs green. PR A.3's prerelease handling validated end-to-end:
   - `tag` ran AFTER all builds succeeded
   - GitHub release marked `--prerelease=true`, `--latest=false`
   - `latest.json` step SKIPPED (S3 stayed at 1.0.0)
   - `bump-source` SKIPPED
   - 14 assets attached (3 DMG/deb/AppImage + 2 Tauri updater + 4 headless + 4 sidecars + Tauri .deb)
2. **`1.0.1` stable** (run 26537924429) — first attempt: `Build (x86_64-apple-darwin)` failed at `bundle_dmg.sh` (Intel Mac DMG flake; same code worked for `-rc.3` and prior releases). Tag correctly held back. Re-ran the failed job only; everything subsequently green:
   - All 13 jobs green
   - Tag `accelerator-v1.0.1` pushed
   - GitHub release marked NOT prerelease (`--latest=true`)
   - 15 assets uploaded (14 from `-rc.3` + `latest.json`)
   - S3 `latest.json` updated from `1.0.0` → `1.0.1`
   - Tauri auto-updater signatures embedded for all 3 platforms
   - Bump-source PR #229 auto-opened, CI green, auto-merged → source now `1.0.2-rc.1` on main (commit `5d46f53`)

## Track 3b epilogue

After NPM_TOKEN rotation, SDK publish re-attempt succeeded:
- `@alejoamiras/aztec-accelerator@4.2.0-revision.1` published with provenance attestation
- Tagged as both `latest` and `testnet`
- Provenance attestation visible in Sigstore transparency log
- Playground also redeployed (per the user-approved decision)

## What went wrong vs the plan

Three things that the plan didn't predict (or got marginally wrong):

1. **`-rc.2` dry-run was the FIRST end-to-end exercise of the build-headless matrix.** The plan acknowledged this and prescribed the dry-run for exactly this reason. The new tag-after-build ordering (PR A.3) held back correctly. But the build-headless config from PR #225 had pre-existing under-provisioning bugs that the PR-gate's `release-smoke` job didn't catch — because the smoke job uses `setup-accelerator` composite (correct deps + prebuild) while the release job had its own minimal setup. PR #228 fixed it.
2. **NPM_TOKEN expiry** blocked the SDK publish. Plan flagged "OIDC trusted publisher unchanged" as a pre-flight item but I didn't verify TOKEN freshness specifically. User rotated mid-execution; publish then worked.
3. **macOS Intel `bundle_dmg.sh` flake**. Not a regression — random Tauri DMG-creation issue on `macos-15-intel`. Same code worked on `-rc.3` and prior 1.0.0 release. Just rerunning the failed job worked.

## What the plan predicted correctly

- Forward-roll @aztec stable on main: clean (PR #227)
- Move npm `latest`: cleanly transitioned `4.2.0-rc.1` → `4.2.0-revision.1`
- Tag-after-build prevents orphan tags: validated by `-rc.2` failure scenario
- Prerelease handling skips latest.json / bump-source: validated by `-rc.3`
- ARM64 runner works on `ubuntu-24.04-arm`: all 4 headless platforms green after #228
- The publish-version script fix produces valid semver: validated end-to-end

## Codex post-impl review

Verdict: **"Mostly clean with minor cleanups."** No must-fix items. Three follow-up issues codex recommended:

1. **Repo hygiene** — fetch/prune after auto-merged release-bump PRs to keep local in sync. Done immediately post-review.
2. **CI dedup** — extract shared release-build setup for `build` and `build-headless` into a composite/reusable action. The drift caught by #228 should be impossible.
3. **Publish preflight** — add a lightweight npm auth + provenance check before manual SDK publish, or schedule monitoring of NPM_TOKEN expiry.

All three are nice-to-haves for future work, not regressions on the current shipped state.

## Final state — all green

| Component | State |
|---|---|
| `@alejoamiras/aztec-accelerator@4.2.0-revision.1` | npm, tagged `latest` + `testnet`, with OIDC provenance |
| `accelerator-v1.0.1` | GitHub release, `--latest`, 15 assets including headless tarballs |
| S3 `latest.json` | `1.0.0` → `1.0.1`, signatures embedded |
| `accelerator-v1.0.1-rc.3` | GitHub prerelease, immutable history; auto-updater unaware |
| Bump-source PR #229 | Merged. Source now `1.0.2-rc.1` (commit `5d46f53`) |
| @aztec automation | `workflow_dispatch`-only, `auto_merge: false` |
| Remote branches | `main`, `nightlies` only |
| Local branches | `main`, `nightlies` only |
