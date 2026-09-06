# Final Aztec Accelerator retirement

Execution of the approved retirement phase, after Presto's production launch. This is not an
automatic migration: existing proving continues, and nothing here installs, edits or removes Presto.

## Entry evidence

- Presto `1.0.0` publish run `34057607592` passed, using RC1 as its same-key updater baseline.
- Promotion run `34058894564` verified the signed public feed and downloads. Its only failure was
  post-release housekeeping: auto-merge is disabled; the source-bump PR #13 was merged separately.
- Both Presto production sites passed browser loading checks; the feed matches the signed release.
- The guarded interactive SDK promotion and independent uncached npm read-back verified
  `@alejoamiras/presto` tags `latest` and `testnet` both resolve to `5.2.0`.
- Retirement branch starts from original main `983334eae9980831dbb704d1242730be5ce83ff7`.
  Published `accelerator-v3.0.0` exists as a non-draft stable release and remains available.

## Implementation checklist

- [x] Add a prominent root README migration notice and recognized Presto production origins.
- [x] Implement static landing/playground migration pages with corresponding Presto links and
  no old download/prove actions. Keep the existing proving UI available only as a development/CI
  fixture so final-release proof tests still exercise the functioning old native application.
  Both Wrangler dry-runs include only the two retirement files; deployment contract tests pass.
  Local browser checks passed at 320px width, including keyboard navigation and no horizontal overflow.
  Hosted production verification remains required after deployment.
- [x] Add a one-time dismissible native migration window and a permanent tray/menu link through
  `https://aztec-accelerator.dev`. Persist dismissal only in the existing legacy configuration.
- [x] Keep existing signing key, native identity, state paths and proving protocol unchanged.
- [x] Review changes, pass affected tests and required CI, and merge the exact reviewed candidate.
- [x] Publish final native `3.1.0` using the existing updater key and real `3.0.0` baseline.
- [x] Promote and verify `3.1.0`, then freeze the legacy feed and disable future publication.
- [x] Deploy and verify retirement pages and update original repository metadata.
- [x] Deprecate all legacy SDK versions with the successor package name; never unpublish.
- [ ] Observe production migration pages, links, downloads and frozen feed for 14 days; record the
  observation start only after retirement is actually live. Archive only after a healthy window.

## Safety boundaries

- Recognized-site entries are display metadata, not permission grants; normal consent remains.
- The migration window opens only a fixed old-domain URL on an explicit user action. It never
  downloads or launches an installer, copies state, removes certificates or edits another app.
- Production static-asset configuration must publish only retirement content, not the retained
  proving fixture or its generated bundles. Native/SDK proving tests must not be bypassed.
- All published versions and assets remain append-only. Do not rerun a successful publication,
  repoint a tag, replace an updater key, or route the legacy updater to a Presto artifact.

## Local validation and bounded fixes

- Full Bun lint/typecheck/unit checks, Actionlint, both Wrangler dry-runs, the native frontend build,
  all 71 Playwright desktop UI tests and 16 Tauri permission/CSP/window-size checks passed.
- macOS desktop `cargo check` and all-target Clippy passed. Three Clippy attempts surfaced existing
  platform-gating warnings; review against the shipped Presto fixes confirmed the cause. Backported
  only the same import/helper `cfg` gates and equivalent `else if` cleanup, without changing OS behavior.
- Core tests: the first full run passed 264/265; the unchanged process-reaping timing test failed.
  Its isolated rerun passed, then the full rerun passed all 265. No proving code or assertion changed.
- Core all-target Clippy and all 11 headless tests passed. Dependency audit: zero blocked findings;
  the existing nine accepted advisories, 15 moderate/low reports and 20 RustSec notices remain unchanged.
- Desktop tests passed (107 library, 10 binary, one TLS integration; real-OS suites stayed ignored).
  All three crates passed format checks and all-target Clippy. The required local Windows cross-check
  was attempted but cannot build `ring`: this Mac lacks `x86_64-w64-mingw32-gcc`. The disposable Windows
  CI build remains mandatory; no toolchain install or gate suppression is part of this retirement.
- Compared the native config against the base: only version and the new window capability differ.
  Updater key/endpoint, bundle metadata, proving implementation, certificates, trust, native updater
  and legacy feed Worker are unchanged. The release workflow changes only its migration release notes.
- Real certificate, autostart and uninstall integrations are reserved for disposable hosted CI.
  Mock UI tests do not substitute for native/release gates; hosted evidence is recorded below.

## Rollout evidence

- The owner unlocked 1Password; commit `1a87b028dff9bca0ce230bd64d7d55803ef8111a` has a verified SSH signature.
  PR #497 passed all 40 checks, including native Windows compilation, three-platform WebDriver,
  certificate integrations and proving. It merged at `2026-09-06T22:33:23Z` as
  `b55250de00b9c934c6ed2278837987e5d4a7aeb5`; the merged tree is identical to the reviewed candidate.
- Native `3.1.0` publication run `34064422661` uses that merged commit; the resolver selected the
  real `accelerator-v3.0.0` baseline. Production signing, both macOS notarization checks, all four
  positive updater tests and all three tamper-rejection tests passed. Packaged macOS/Linux proving,
  Linux/Windows uninstall and the existing upgrade/config checks also passed, including macOS cleanup.
  The workflow succeeded and published the stable release at `2026-09-06T23:03:05Z`, with exactly 17
  assets and tag commit `b55250d`. The signed `latest.json` SHA-256 is
  `21b1721586aa3ed868c62d00038dd6c21a8a6e154cff538ec6b96e270cafd286`.
- Promotion dry-run `34065850584` passed without changing the old `3.0.0` feed. Actual promotion
  `34065998828` passed, including production signature verification of the live feed and downloads.
  The public legacy feed now matches the signed `3.1.0` asset byte-for-byte; Presto's separate feed
  still matches its own signed `1.0.0` asset. GitHub Latest is `accelerator-v3.1.0`.
- Legacy `release-accelerator.yml`, `release-sdk.yml` and `deploy-release-feed.yml` are confirmed
  `disabled_manually`. Existing Workers, KV contents and credentials are retained; the daily health
  workflow remains enabled. No source-bump PR was created for this final release.
- Landing deployment `34064474630` succeeded: Worker version `35fd47ce-0289-4def-b8e7-5485ead7c2c3`.
- Playground-only deployment `34064442605` succeeded without npm publication:
  Worker version `d2f5db99-d6f7-4a1c-bcce-c2e2d072d820`.
- Both production retirement pages match the reviewed HTML byte-for-byte; live 320px browser checks
  passed keyboard links, isolation, no overflow, no old action controls and no script errors.
  The legacy feed still served `3.0.0` after those deployments, as required before final promotion.
- Repository description/homepage now direct users to Presto; it remains unarchived.
- Interactive npm deprecation succeeded. An uncached registry read-back at
  `2026-09-06T23:18:10Z` confirmed the exact migration warning on all 44 versions, including
  prereleases. Compared against the pre-operation registry snapshot: the complete version set,
  dist-tags and every version record apart from `deprecated` are unchanged (including tarball
  integrity and provenance metadata). Nothing was unpublished or republished.

## Observation and archive gate

Observation has **not started**. Native `3.1.0` promotion, publication freeze and legacy npm
deprecation are verified. Merge this monitoring follow-up and verify its first hosted run, then
record the UTC start and earliest archive time (start + 14 days) in a dated PR comment linked from
the launch checklist. The run's successful completion timestamp starts the window.

The existing `Update Feed Health` workflow runs daily at 07:00 UTC. Its retirement checks pin the
legacy feed and asset URLs to `3.1.0`, retain the download probes, and check both migration pages and
their Presto destinations. It uses no deployment/signing secrets and does not mutate production.
Keep the workflow enabled during observation and retain the dated run links as evidence.
The prepared follow-up passed full local Bun checks and Actionlint. The migration-page job also
passed when run against the live sites, and a negative control confirmed that the frozen legacy-feed
check refuses the live Presto feed before attempting any download. After promotion, the actual feed
job also passed locally against legacy `3.1.0`, including all four artifact download probes and the
GitHub release comparison. This follow-up is not deployed yet.

Before archiving, confirm 14 elapsed days and healthy daily checks, repeat live browser/download/feed
verification, and resolve any gaps or failures. A failed migration path blocks archive; after repair,
record a fresh healthy observation window rather than treating an unchecked period as success.
Archive only the GitHub repository; leave both Cloudflare retirement pages and the frozen feed online.
