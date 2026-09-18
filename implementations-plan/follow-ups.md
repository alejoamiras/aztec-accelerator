# Open follow-ups

**Read the framing first: this product is retired.** Final native `3.1.0` shipped, the legacy updater feed is frozen,
`Release Accelerator` / `Release SDK` / `Deploy Release Feed` are all disabled, and landing and playground serve
migration pages only. See [`docs/PRESTO_RETIREMENT.md`](../docs/PRESTO_RETIREMENT.md). Development continues in Presto.

So most of what follows cannot be acted on here: it gates releases that will not happen again. Only the first two
sections are live. The rest is kept so archiving does not silently drop it, and should be deleted when the repo is
archived.

Seeded by sweeping all 50 archived plans, 2026-09-17 and 2026-09-18. Status is unconfirmed against today's `main`
except where a code-level check is noted.

## Live — survives retirement

These are account and credential hygiene. They outlast the product.

- **AWS root access key still not deleted** — `release-1.0.7`'s closeout left this as an unchecked owner action once the
  three scoped OIDC pipeline roles were proven in production: nothing in the account needs root credentials. The case is
  stronger now, since the AWS stack is gone entirely (`infra/` holds only a branch-protection ruleset) and hosting moved
  to Cloudflare Workers. Root keys can only be removed by signing in as the account root.
  `archive/release-1.0.7/STATUS.md`
- **Deprecate the superseded `@alejoamiras/aztec-standards` npm package** — P4 has read blocked on owner npm auth
  (`npm whoami` returned 401) since 2026-07-16, and no later plan ran `npm deprecate`. The old package still resolves as
  live and undeprecated, so consumers can still install it by accident. `archive/aztec-5.0.1-2026-07-16/plan.md`

## Carried to Presto

The ingress and SDK code was forked into the successor, so these travel with it rather than dying here. Neither has been
checked against the Presto repo.

- **Untimed HTTP client fallback and unbudgeted in-request download** — `release_metadata.rs` ends
  `.build().unwrap_or_else(|_| reqwest::Client::new())`, silently dropping its 300s/30s deadlines if the builder ever
  fails, and `/prove` still awaits an attacker-chosen version download inside the request path with no per-request
  budget. Closed here as documented, not fixed. `archive/independent-hardening/lessons/phase-1.md`
- **Node and Bun consumers still default to plaintext** — the browser HTTPS-only default closed the port-squat
  witness-capture window for dApps, but server runtimes keep the HTTP compatibility path, so the window persists there
  unless Presto narrows it. `archive/independent-hardening/findings.md`

## Dead-lettered by retirement

Real work, verified still open in code, that nothing will act on because the product is frozen. Kept for the record.

**Windows Authenticode signing** was the standing tension in this file: deferred by the owner in `v2-release-train`,
while `mega-ready-audit` called it the number one mainstream-adoption blocker. Retirement resolves it. No signed build
will ship. `archive/v2-release-train/brief.md`

- **Tray build failure takes down the whole app** — `main.rs` propagates a tray error out of `.setup()` with `?`, so a
  missing libayatana means no accelerator at all rather than an accelerator without a tray. The fix threads an
  `Option<TrayIcon>` through five startup call sites. `archive/mega-ready-audit/readiness.md`
- **No hint when Firefox has enterprise roots disabled** — nothing detects `security.enterprise_roots.enabled` being
  off, so the user sees TLS warnings and the SDK drops to HTTP with no explanation. `archive/mega-ready-audit/readiness.md`
- **Two accepted same-user hardening nits** — the `secure_create_dir` create-then-open swap window, and the Unix
  `config.json.tmp` path keeping a pre-planted file's looser mode. Both deliberately left as churn over risk; recorded so
  the acceptance stays visible. `archive/mega-ready-audit/readiness.md`
- **Assert trust survives an over-the-top update** — the Windows NSIS uninstall hook is `$UpdateMode`-guarded; the guard
  shipped, the CI assertion proving it did not. `archive/https-by-default-onboarding-2026-07-09/plan.md`
- **Onboarding spec never ran on the WebDriver leg** — the Playwright specs landed, the WebDriver equivalent was pushed
  to "CI iteration". `archive/https-by-default-onboarding-2026-07-09/plan.md`
- **Playwright consent UI is tested on ubuntu only** — the desktop-ui job runs solely on `ubuntu-latest`, leaving the
  most security-sensitive UI untested against Windows webview quirks. `archive/mega-ready-audit/test-plan.md`
- **Router and auth wiring have no direct tests** — server assembly and the per-port host guard are exercised only
  through HTTP-level tests, and `server/auth.rs` has no inline tests at all. `archive/mega-ready-audit/test-plan.md`
- **Crash-recovery CI does not watch its own module** — the gate fires only when its `.ps1` or workflow changes, so an
  edit to `crash_recovery.rs` ships untested by it. `archive/mega-ready-audit/test-plan.md`
- **Release assets are never tied to their commit** — the fixture preflight checks the release tag's config, not that
  the downloaded asset came from that commit. Accepted as a Low residual; the named fix was release attestation or a
  recorded asset digest. `archive/arc-bug-hunt/log.md`
- **Nulo Chrome extension never re-added to verified-sites** — the placeholder was removed pending a Chrome Web Store ID
  that was never obtained. `archive/verified-sites-2026-05-28/plan.md`
- **`versions_to_evict` and `bb_asset_name` still re-parse the raw string** — the `AztecVersion` value object landed in
  PR #300 without these two call paths, which is the duplication it existed to remove.
  `archive/quality-refactor-2026-06-05/plan.md`
- **Manual v5 smoke never recorded as run** — the full local sweep ran; the manual smoke was deferred.
  `archive/aztec-5.0.0-2026-06-18/plan.md`

## Decided against

- **SDK phase-event discriminated union** — cut as negative ROI: a second phase type tying `durationMs` to "proved"
  across ~25 sites, fighting the playground's string-based animation model. `archive/quality-refactor-2026-06-05/plan.md`
- **Window-scoped capability for the onboarding window** — deferred with the bundled-content XSS risk accepted in
  writing per the S4 fallback. `archive/https-by-default-onboarding-2026-07-09/plan.md`
- **`F-09` withdrawn-release replay** — every known fix reintroduces the `F-04` permanent-lockout shape; needs an
  upstream revocation story that does not exist. `archive/mega-ready-audit/readiness.md`

## Standing non-goals

Declared out of scope by more than one plan. `archive/maintenance-2026-05-27/plan.md`, `archive/release-2026-05-27/plan.md`

- npm wrapper package for the headless binary
- Code signing / attestation for the headless tarball
- Windows headless build
- `cargo audit` in CI
- Server-side `AZTEC_ACCELERATOR_PORT` support
- Bearer-token auth on `/prove`
- Re-enabling Dependabot
- Cargo workspace conversion
