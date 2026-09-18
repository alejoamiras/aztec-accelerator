# Recon — SDK 5.2.0 npm release (4 sonnet explorers, 2026-08-26, base 8ab7df9)

## Reuse map

| Capability | Existing code | Verdict |
|---|---|---|
| Publish dispatch | `publish-testnet.yml` (assert-main ∥ changes ∥ e2e → publish-sdk + deploy-app) | reuse-as-is |
| Version derivation | `scripts/get-sdk-publish-version.ts` — 5.2.0 unpublished ⇒ mints bare `5.2.0` | reuse-as-is |
| Manifest rewrite | `scripts/prepare-sdk-publish.ts` (dist exports/main/types; unit-tested) | reuse-as-is |
| Latest promotion + rollback | `promote-latest.yml` — tag-only, verifies published, re-dispatch w/ prior version = rollback | reuse-as-is |
| Consumer-fidelity proof | `scripts/sdk-tarball-consumer.sh` (real npm pack+install+tsc+runtime on rewritten manifest) — runs in `sdk.yml` PR gate only, NOT in the publish pipeline | reuse-as-is, run locally pre-dispatch (precedent: v2 release did exactly this) |
| Acceptance smoke | UNDEFINED in tooling — per-cycle manual convention only (grep: two prose comments, no script/workflow/runbook) | owner waived this cycle ("trust the pipeline") |
| Release runbook | `docs/RELEASE_RUNBOOK.md` — SDK section stale (says 5.0.0-rc.x/testnet-only), ZERO mention of promote-latest.yml, rollback section accelerator-only | adapt (docs PR, Phase 1) |
| Dist-tag consumer docs | `README.md:67` + `packages/sdk/README.md:18` — same stale rc.x callout; sdk README SHIPS IN THE TARBALL | adapt (docs PR, Phase 1 — must merge BEFORE dispatch) |

## Load-bearing facts

1. **Pipeline chain** (agent 1, files read in full): dispatch → `assert-main` (refuses non-main ref) ∥ `changes` (decorative on dispatch) ∥ `e2e` (`_e2e.yml`, **local sandbox** node at localhost:8080 — NOT live testnet — with `build_accelerator: true`: cargo-built headless accelerator, native-bb sdk `test:e2e`, `ACCELERATOR_DOWNLOAD_TEST=true`) → `publish-sdk` (`_publish-sdk.yml`, `dist_tag: testnet` hardcoded) ∥ `deploy-app` (playground from workspace SOURCE → S3 + CloudFront invalidation; **not `needs:`-gated on publish-sdk** despite a comment claiming ordering — harmless, source-built).
2. **Publish internals**: build (tsc) → read `@aztec/stdlib` dep (5.2.0) → `get-sdk-publish-version` vs live registry → `prepare-sdk-publish.ts` rewrite (runner-tree only) → `npm publish --provenance --access public --tag testnet --workspaces=false` (auth = static `NPM_TOKEN`; `id-token: write` is for Sigstore provenance, NOT npm trusted publishing) → git tag `@alejoamiras/aztec-accelerator@5.2.0` (ls-remote squat guard, hard-fail) + `gh release create --latest=false` + MIGRATION.md asset.
3. **npm live state** (checked during recon): `latest=5.0.1`, `testnet=5.0.1-revision.1`, no `5.2.0*` exists ⇒ first dispatch mints bare **5.2.0**. `-revision.N` only on redispatch-after-partial-failure.
4. **promote-latest.yml**: regex `X.Y.Z(-revision.N)?`, verifies version exists on npm, `npm dist-tag add`, prints post-state. No smoke enforcement, no monotonicity check (backward move = intended rollback). Shares non-cancelling `publish-npm` concurrency group with `_publish-sdk.yml`.
5. **Historical failure modes** (agent 3, lessons read): (a) **NPM_TOKEN expiry** — 2026-05-27 first-ever publish: E404 at registry PUT AFTER provenance signing; fixed by owner token rotation; never blind-retry a confirmed-non-transient failure. (b) Fail-after-publish ⇒ redispatch mints `-revision.N` — check `npm view … versions` first (workflow header mandates). (c) SDK GitHub release once stole the accelerator's Latest badge + poisoned updater N-1 (fixed permanently: `--latest=false` + N-1 filters — load-bearing, don't touch). (d) GitHub-outage flakes need in-run retries (SDK line does no asset downloads — low exposure). (e) Append-only: never delete/re-cut tags or releases.
6. **What "trust the pipeline" buys** (agent 4): real consumer proof (pack + rewrite + fresh npm install + tsc + runtime import + stdlib-singleton F13 gate) exists but ONLY as `sdk.yml`'s PR-merge check — nothing re-proves the artifact at dispatch time, and nothing verifies post-publish. Last real publish: operator ran `sdk-tarball-consumer.sh` locally right before dispatching (lessons/release.md). The swap-sdk script packs PRE-rewrite (its "exact artifact" claim is inaccurate) and lives on the accelerator release line — irrelevant here.
7. **Consumer impact of publishing**: `^5.0.x`-range consumers resolve 5.2.0 the moment it publishes to ANY tag (ranges ignore dist-tags). The promotion step only changes bare/`@latest` installs. So the publish dispatch itself is the bigger exposure event, not the promote.
8. **Version-coherence surfaces**: playground = build-time auto from sdk package.json (deploys correct in same dispatch); landing = accelerator-feed only, no SDK surface (grep-verified); sdk MIGRATION.md + SKILL.md = version-free; README npm badges = auto (shields.io). Only the three stale prose callouts need edits.
9. **Non-blockers confirmed**: `@types/bun` min-age trailing commit (~08-28) is devDep-only, decoupled; packaged-e2e leg is accelerator-release-only; aztec-standards stays 5.0.1 (no 5.2.0 upstream exists — prior owner call A1).
10. **Full-repo suite green on this exact content**: merged main (476+477 combined) passed the full chain locally 2026-08-25; #478 green in CI. No SDK public-API change since (public-contract untouched by 478).

## Dedup risks
- Do NOT hand-roll any publish/promote/verify step that the workflows already do (version choice, tag creation, release notes) — dispatch and verify outputs only.
- Do NOT conflate the repo's three version axes: Aztec/SDK npm version (this release) ≠ `ACCELERATOR_API_VERSION=1` (HTTP contract) ≠ accelerator app version (signed feed / landing).
- `_publish-sdk.yml` has its own direct dispatch that SKIPS assert-main + e2e — never use it; always go through `publish-testnet.yml`.

## Absence claims (search trails in agent transcripts)
- No in-pipeline post-publish verification (grep sdk-tarball-consumer across .yml; full reads of both publish workflows).
- No acceptance-smoke definition in tooling (grep "acceptance" repo-wide: 2 prose comments only).
- No promote-latest.yml mention in RELEASE_RUNBOOK.md (grep, exit 1).
- No npm trusted-publisher OIDC config; no .npmrc; no prepack script; no .npmignore (finds/greps listed in agent 4).
