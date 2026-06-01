# B2 path-filter fix + 1.0.3 stable release

## Context
1.0.3 is ready to ship. It carries the **Linux auto-update fix** (#251/#252/#253 — the headline: Linux 1.0.2 users currently lose their accelerator server's `:59833` port on auto-update), **verified-sites** friendly names (#231), and the full **CI/release overhaul** (parallelized gate, Rust cache, path-filters, blocking macOS + Linux updater gates). The release is outward-facing: the stable cut uploads `latest.json` to S3 and real 1.0.2 users auto-update to 1.0.3.

First fix a path-filter bug (the "B2 anomaly") that makes nearly every PR run the full ~15min gate. Then cut 1.0.3.

**Locked decisions:** cut from main as-is; **no** extra rc.14 (rc.13 + 1.0.1/1.0.2 history suffice); I dispatch the stable cut after pre-flight is green, then verify + merge the post-release bump.

## Consolidation (codex `019e851c` + opus audit — both "ship-with-changes")
**Adopted:** (1) B2 root-cause confirmed by both; keep the deletion fix but honestly scope it (below). (2) Mandatory live-feed verification post-publish (curl latest.json + HEAD all asset URLs). (3) Re-cut footgun → rerun-failed-jobs only; don't delete 1.0.2. (4) Pre-flight secret-existence + branch-protection + PAT-scope checks. (5) Wording fixes: CloudFront already invalidates (manual = contingency); "frozen lockfile" overstated.
**Rejected (with reason):** the `predicate-quantifier: every` + `any:`-group B2 refactor — opus warned a flat `every` "wrongly skips almost everything," and a gate misconfig is high-blast-radius right before a stable cut; the safe pure-allowlist deletion fixes the actual #254 symptom. Deferred as a validated follow-up.
**Verified-fine by both (de-risks the cut):** pubkey `B371381E…` identical 1.0.1→rc.13; endpoint unchanged; `bind_with_retry` HTTP+HTTPS retries concurrent (~5s), HTTPS can't crash the app; Linux gate genuinely in `tag.needs`+`release.needs`; rollback kill-switch real (≤5-min exposure via max-age=300).

---

## Part 1 — B2 path-filter fix (pre-release cleanup)

### Root cause (confirmed from dorny's log on #254)
dorny detected exactly the 2 changed docs files, yet `desktop`/`relevant` came out **true** and the gate ran. Under dorny's default `predicate-quantifier: some`, each `!packages/**/*.md` line is OR-combined as a standalone rule matching the **complement** of that glob — so `!packages/accelerator/**/*.md` matches *any non-accelerator-md file* (e.g. `implementations-plan/index.md`). The negations thus (a) never perform their intended md-exclusion and (b) make the filter match almost any change → B2's narrowing (docs-only + pure-sdk-src skipping the heavy gate) never actually worked.

### Fix — remove the four `!...**/*.md` negation lines
- `.github/workflows/accelerator.yml` — `desktop` and `integration` filters (`!packages/accelerator/**/*.md` in each)
- `.github/workflows/app.yml` — `relevant` (`!packages/playground/**/*.md`, `!packages/sdk/**/*.md`)
- `.github/workflows/sdk.yml` — `relevant` (`!packages/sdk/**/*.md`)

Post-fix: a change trips a filter only if it matches a positive allowlist path. **This fixes the observed #254 symptom** — changes OUTSIDE the package allowlists (`implementations-plan/**`, root docs, etc.) and pure-`packages/sdk/src/**` changes correctly skip the heavy gate. **Honest scope (both auditors):** the deletion does NOT make a markdown-only change *under* `packages/accelerator/**` / `packages/sdk/**` / `packages/playground/**` skip — those still match the positive `packages/<pkg>/**` glob and run their gate (rare, ~15min, cheap). Achieving true in-subtree md-exclusion needs the `predicate-quantifier: every` + `any:`-grouped-exclusion idiom — **deliberately deferred**: opus warned a flat `every` over the current mixed list "wrongly skips almost everything," and misconfiguring a gate that also guards pre-auto-merge CI is high-blast-radius right before a stable cut. Pure-allowlist deletion is the safe, predictable fix for the actual problem; the `every`/`any:` refactor is a validated follow-up if in-subtree md-skip is ever wanted.

### Verify
- The B2-fix PR changes workflows → correctly triggers + passes the gate.
- After merge: a trivial docs-only PR → all package gates skip (`changes=false`); a `packages/sdk/src` touch → accelerator `desktop` skips, `integration` runs.
- `bun run lint:actions` clean.

---

## Part 2 — 1.0.3 stable release runbook

### Pre-flight (all must hold before the cut)
1. B2 fix merged; main green.
2. Scope frozen = current main (everything since 1.0.2). Draft release notes (headline: Linux auto-update fix; verified-sites; CI hardening).
3. Confirm latest stable = `accelerator-v1.0.2` → N-1 for the blocking updater gate is 1.0.2. **Do NOT delete the 1.0.2 release** — the updater gate downloads its DMG/AppImage as the N-1 baseline.
4. Review the **stable-only** steps rc dry-runs skip (`release-accelerator.yml`, gated on `is_prerelease=='false'`): `latest.json` generation (all-sigs-present assert), GitHub release `--latest`, **S3 upload** (OIDC→AWS), CloudFront invalidation, `bump-source`. These last ran on 1.0.2 — confirm no overhaul-era regressions.
5. **Stable-only secrets/perms (never exercised by rc — audit MED):** confirm the secrets exist (`gh secret list`: AWS OIDC role/`S3_BUCKET_NAME`/`CLOUDFRONT_DISTRIBUTION_ID`/`PAT_TOKEN`/Apple/Tauri-signing) and that 1.0.2 used them successfully (precedent). Full IAM-trust verification is beyond repo access — rely on 1.0.2 precedent + the staged rollback. Note the split-failure mode: a failure at the S3 step leaves us **tagged + GitHub-released but with a stale prod latest.json** (recoverable via rollback).
6. **Branch protection (audit HIGH — PAT auto-merge):** confirm `main` protection requires the `*-status` checks, so the PAT-driven `bump-source` PR cannot auto-merge ungated; confirm `PAT_TOKEN` is minimally scoped (flag if a classic all-repo token).
7. Stage rollback asset: fetch the **1.0.2 `latest.json`** (from the `accelerator-v1.0.2` release assets) so the feed can be reverted instantly.
8. **Re-cut footgun (both auditors):** if `release`/`bump-source` fails AFTER the GitHub release exists, **rerun the failed jobs only / recover manually — do NOT fresh-dispatch `version=1.0.3`** (N-1 would then resolve to 1.0.3, tripping the updater-smoke no-op guard and hard-failing the gate). Fresh re-dispatch requires first deleting/un-latest-ing the 1.0.3 release.
9. Final codex review of the consolidated runbook (this plan), then proceed (approval waived).

### The cut (I dispatch, after pre-flight green + your plan approval)
- `gh workflow run release-accelerator.yml --ref main -f version=1.0.3` (no `-rc` → full stable path).
- Pipeline: validate → e2e-webdriver gate (parallel) → build (3 Tauri + 4 headless) → smoke + smoke-intel + **update-smoke (macOS arm64+Intel) + update-smoke-linux** (all BLOCKING, exercising 1.0.2→1.0.3) → tag `accelerator-v1.0.3` → release (latest.json + GitHub `--latest` + S3 upload) → bump-source.
- Watch the run; the blocking updater legs are the live proof the 1.0.2→1.0.3 auto-update path is safe on all platforms.

### Post-release verify
- GitHub release `accelerator-v1.0.3` marked **Latest**; assets present (macOS DMG×2 + app.tar.gz×2 + .sig, Linux deb + AppImage + .sig, headless×4, latest.json).
- **MANDATORY live-feed check (audit HIGH — both):** the pipeline already invalidates CloudFront (`max-age=300` + `create-invalidation`), so manual invalidation is contingency-only. But the invalidation is fire-and-forget and `latest.json` URLs are string-built — so before calling the cut good, `curl -fsS https://aztec-accelerator.dev/releases/latest.json` → assert `.version=="1.0.3"` and the three platform keys (`darwin-aarch64`, `darwin-x86_64`, `linux-x86_64`) present, then `curl -fsIL` **each** asset URL in that JSON → 200. This is the only check that proves the CDN serves a 1.0.3 feed pointing at live assets (the updater gate used a *synthesized* feed, not this one). Retry up to the 5-min cache window; if still stale, trigger a manual invalidation.
- `bump-source` opens `chore/bump-accelerator-1.0.4-rc.1` (`--auto --squash`, PAT-driven so its gate runs); confirm its gate passes + it auto-merges (manually merge if auto stalls). main → 1.0.4-rc.1.
- Real-world check (do it, not optional): install 1.0.2, confirm auto-update to 1.0.3 — especially **Linux** (the bind-retry fix's first production exercise). *(May be manual/local; if infeasible in this environment, rely on the blocking updater gate + the live-feed check above and note it.)*

### Rollback (kill-switch)
If 1.0.3 is bad: re-upload the staged **1.0.2 latest.json** to S3 (feed reverts → new update-checks see 1.0.2, halting 1.0.3 propagation) + invalidate CloudFront; mark the 1.0.3 GitHub release non-latest/prerelease. Users already on 1.0.3 can't be remotely downgraded, but the bleed stops.

---

## Security & adversarial considerations
- **Update trust chain:** artifacts are minisign-signed (+ Apple-notarized); clients verify against the embedded pubkey, so a tampered `latest.json`/artifact is rejected — S3/CloudFront integrity is not the sole trust root. Signing keys stay in CI secrets, used only at build.
- **Least privilege:** release AWS access via OIDC (`id-token: write`), scoped to the latest.json path; `bump-source` PAT scoped to contents + PRs.
- **Supply chain:** build from the tagged main commit. *(Wording correction per audit: the headless server build does not pass `--locked` (release-accelerator.yml:267), so "frozen lockfile" is overstated — cut from a commit whose source versions are already final.)* The blocking updater gate is itself the anti-regression guard for the exact 1.0.1-class failure. (Follow-up: root `bunfig.toml` lacks the self-declared `minimumReleaseAge = 604800`; irrelevant to this cut — no `npm publish` in the path.)
- **Blast radius:** the latest.json feed is the kill-switch — 1.0.2 latest.json staged before the cut.
- **Shipped `bind_with_retry`:** codex-reviewed + adversarially audited (HTTP + HTTPS listeners); bounded, AddrInUse-only — no new surface.

## Implementation order
1. Formalize into `implementations-plan/release-1.0.3-2026-06-01/plan.md` (repo convention).
2. B2 fix PR → merge → verify narrowing.
3. Release pre-flight: notes, stable-only review, codex runbook review, stage 1.0.2 latest.json.
4. Dispatch stable 1.0.3 → watch → post-release verify → merge bump → confirm main at 1.0.4-rc.1.
