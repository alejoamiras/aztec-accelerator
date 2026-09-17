# Open follow-ups

Work deferred out of a plan that closed. Read this during Phase 0 recon; delete an entry the moment it resolves.
A plan must not close while it still owns an open follow-up — lift it here, or open a GitHub issue and leave only a pointer.

**Provenance note:** this file was seeded by sweeping the 28 plans archived on 2026-09-17. Each entry was deferred in
writing by its plan; none has been re-verified against today's `main`. Treat status as *unconfirmed* until checked, and
delete anything already done.

## Deferred with concrete work attached

- **Assert trust survives an over-the-top update** — the Windows NSIS uninstall hook is `$UpdateMode`-guarded so an update
  must not delete the CurrentUser Root anchor or the certs dir. The guard shipped; the CI assertion proving it did not.
  Called out as release-critical because the hook is baked into each release's uninstaller and cannot be retro-fixed by a
  later one. `archive/https-by-default-onboarding-2026-07-09/plan.md` (Phase 6)
- **Onboarding spec on the WebDriver leg** — the Playwright onboarding specs landed; the WebDriver equivalent was pushed to
  "CI iteration". The wizard is a release-blocking cross-OS flow, so it is the leg that matters most.
  `archive/https-by-default-onboarding-2026-07-09/plan.md` (Phase 5)
- **Thread `versions_to_evict` and `bb_asset_name` through `AztecVersion`** — the value object landed in PR #300 but these
  two call paths still re-parse the raw string, which is the duplication the object existed to remove.
  `archive/quality-refactor-2026-06-05/plan.md` (Phase 4)
- **Manual v5 smoke** — the full local sweep ran, the manual 5.x smoke was explicitly deferred and never recorded as done.
  `archive/aztec-5.0.0-2026-06-18/plan.md` (Phase 5)

## Decided against, revisit only on request

- **SDK phase-event discriminated union** — cut deliberately as negative ROI: a second phase type tying `durationMs` to
  "proved" across ~25 sites, fighting the playground's string-based animation model. Owner-overridable and reversible
  pre-publish; ships as a follow-up if asked. `archive/quality-refactor-2026-06-05/plan.md` (Phase 8)
- **Window-scoped capability for the onboarding window** — deferred with the bundled-content XSS risk accepted in writing
  per the S4 fallback. Recorded here so the acceptance stays visible rather than buried in a closed plan.
  `archive/https-by-default-onboarding-2026-07-09/plan.md` (Phase 5)

## Standing non-goals

Declared out of scope by more than one plan, so they read as backlog rather than one-off exclusions. Listed to stop them
being re-litigated from scratch each time. `archive/maintenance-2026-05-27/plan.md`, `archive/release-2026-05-27/plan.md`

- npm wrapper package for the headless binary
- Code signing / attestation for the headless tarball
- Windows headless build
- `cargo audit` in CI
- Server-side `AZTEC_ACCELERATOR_PORT` support
- Bearer-token auth on `/prove`
- Re-enabling Dependabot
- Cargo workspace conversion
