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
- [ ] Review changes, pass affected tests and required CI, and merge the exact reviewed candidate.
- [ ] Publish final native `3.1.0` using the existing updater key and real `3.0.0` baseline.
- [ ] Promote and verify `3.1.0`, then freeze the legacy feed and disable future publication.
- [ ] Deploy and verify retirement pages and update original repository metadata.
- [ ] Deprecate all legacy SDK versions with the successor package name; never unpublish.
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
  Mock UI tests do not substitute for native/release gates; no production legacy deployment is recorded yet.

## Current handoff

Implementation and local validation are complete; changes are staged on
`worktree-legacy-presto-retirement`. The commit hooks passed, but commit signing failed with
`1Password: agent returned an error`. No commit or PR was created and no production state changed.
Resume by unlocking 1Password and retrying the signed commit; do not rotate keys or disable signing.
The subsequent Windows/other hosted CI and release gates are still required.
