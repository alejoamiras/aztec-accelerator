# Open follow-ups

Work deferred out of a plan that closed. Read this during Phase 0 recon; delete an entry the moment it resolves.
A plan must not close while it still owns an open follow-up — lift it here, or open a GitHub issue and leave only a pointer.

**Provenance note:** seeded by sweeping the 43 plans archived on 2026-09-17. Each entry was deferred in writing by its
plan and shows no evidence of closure, but none has been re-verified against today's `main` except where noted. Treat
status as *unconfirmed* and delete anything already done.

## Needs an owner decision

- **AWS root access key still not deleted** — `release-1.0.7`'s closeout leaves this as an explicit unchecked owner
  action once the three scoped OIDC pipeline roles were proven in production: nothing in the account needs root
  credentials. No later plan references doing it. Worth re-checking against the Cloudflare Workers migration, which may
  have changed what that account is still for. `archive/release-1.0.7/STATUS.md`
- **Windows builds still ship unsigned** — no Authenticode. Deferred across three plans in a row, and `v2-release-train`
  restates it as owner-deferred, do not touch. The still-active `mega-ready-audit` (2026-08-21) disagrees, calling it the
  number one mainstream-adoption blocker and recommending it as the next engagement's P0. The tension is unresolved.
  `archive/v2-release-train/brief.md`

## Deferred with concrete work attached

- **Assert trust survives an over-the-top update** — the Windows NSIS uninstall hook is `$UpdateMode`-guarded so an update
  must not delete the CurrentUser Root anchor or the certs dir. The guard shipped; the CI assertion proving it did not.
  Release-critical, because the hook is baked into each release's uninstaller and cannot be retro-fixed later.
  `archive/https-by-default-onboarding-2026-07-09/plan.md` (Phase 6)
- **Deprecate the superseded `@alejoamiras/aztec-standards` npm package** — P4 has read blocked on owner npm auth
  (`npm whoami` returned 401) since 2026-07-16, and no later plan runs `npm deprecate`. The old package still resolves as
  live and undeprecated. `archive/aztec-5.0.1-2026-07-16/plan.md` (P4)
- **Onboarding spec on the WebDriver leg** — the Playwright onboarding specs landed; the WebDriver equivalent was pushed
  to "CI iteration". It is the leg that matters, since the wizard is a release-blocking cross-OS flow.
  `archive/https-by-default-onboarding-2026-07-09/plan.md` (Phase 5)
- **Tie a release asset to its commit in the preflight** — the fixture preflight checks the release tag's config, not that
  the downloaded asset actually came from that commit. Accepted as a Low residual, documented but never coded; the fix
  named was release attestation or a recorded asset digest tied to the tag. `archive/arc-bug-hunt/log.md` (residual 5)
- **Re-add the Nulo Chrome extension to verified-sites** — the placeholder entry was removed pending a real Chrome Web
  Store ID that was never obtained. The live `verified-sites.json` ships only Nulo's two `https://` origins, with no
  `chrome-extension://` entry. `archive/verified-sites-2026-05-28/plan.md`
- **Thread `versions_to_evict` and `bb_asset_name` through `AztecVersion`** — the value object landed in PR #300, but
  these two call paths still re-parse the raw string, which is the duplication the object existed to remove.
  `archive/quality-refactor-2026-06-05/plan.md` (Phase 4)
- **Manual v5 smoke** — the full local sweep ran; the manual 5.x smoke was explicitly deferred and never recorded as done.
  `archive/aztec-5.0.0-2026-06-18/plan.md` (Phase 5)

## Decided against, revisit only on request

- **SDK phase-event discriminated union** — cut deliberately as negative ROI: a second phase type tying `durationMs` to
  "proved" across ~25 sites, fighting the playground's string-based animation model. Owner-overridable and reversible
  pre-publish. `archive/quality-refactor-2026-06-05/plan.md` (Phase 8)
- **Window-scoped capability for the onboarding window** — deferred with the bundled-content XSS risk accepted in writing
  per the S4 fallback. Recorded so the acceptance stays visible rather than buried in a closed plan.
  `archive/https-by-default-onboarding-2026-07-09/plan.md` (Phase 5)

## Standing non-goals

Declared out of scope by more than one plan, so they read as backlog rather than one-off exclusions. Listed to stop them
being re-litigated from scratch. `archive/maintenance-2026-05-27/plan.md`, `archive/release-2026-05-27/plan.md`

- npm wrapper package for the headless binary
- Code signing / attestation for the headless tarball
- Windows headless build
- `cargo audit` in CI
- Server-side `AZTEC_ACCELERATOR_PORT` support
- Bearer-token auth on `/prove`
- Re-enabling Dependabot
- Cargo workspace conversion
