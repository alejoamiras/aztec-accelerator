# Recon: B6 + B4 — release pipeline & end-product validation

Agent: sonnet Explore, 2026-08-16, tree @ 0c351bc. App 1.0.8-rc.1 (tauri.conf.json:4); latest stable 1.0.7.

## release-accelerator.yml job map (1169 lines; dispatch-only, main-only :40-45; concurrency
release-accelerator; top-level contents:read)

validate(:24-66, semver + is_prerelease from `-`) → release-auth-preflight(:73-89, OIDC probe only)
→ e2e-webdriver(:91-105, 3-OS matrix) + gate-watchdog(:115-127, cost-control cancel) → build(:129-285,
4-way matrix, updater Ed25519 via createUpdaterArtifacts:v1Compatible tauri.conf:45, macOS notarize +
DMG retry) + build-headless(:287-377, incl linux-arm64) → smoke(:379-462, macOS arm64 DMG mount +
codesign/stapler + launch + /health loop :425-446) + smoke-intel(:470-499, verify-only) +
update-smoke×4(:506-605, via _e2e-updater{,-linux,-windows}.yml) → tag(:607-652, HEAD==sha assert)
→ sign-update-feed(:663-775, stable-only; assembles+envelope-signs latest.json, verifies with the
PRODUCTION Rust verifier :764; isolated so signing key and AWS creds never share a job) →
**release(:777-998: contents:write + id-token:write)**: flatten/assert 16 files :832-856; gh release
delete-then-recreate if exists :895-899; gh release create --verify-tag :959-979; THEN SAME JOB
:981-998 assumes AWS role, `aws s3 cp latest.json s3://$BUCKET/landing/releases/latest.json` +
CloudFront invalidation — **the exact B6 coupling** → verify-live-feed(:1006-1070, stable-only,
post-hoc canary polling public CDN + production verifier) → bump-source(:1072-1168, permissions:{},
GH-App token, next-RC PR).

**SPLIT POINT**: publish = :792-979; promote = :981-998 as new job needing only validate outputs +
signed-update-feed artifact + AWS secrets; `id-token:write + contents:read` — LESS privilege than
today's combined job. verify-live-feed stays post-promote; add release-asset check post-publish.

## Feed + channels

- Storage: S3 landing/releases/latest.json behind CloudFront (invalidation :994-998; viewer-request
  rewrite /releases→/landing/releases). Public: https://aztec-accelerator.dev/releases/latest.json.
  Artifacts themselves = GitHub release assets.
- tauri.conf.json:24-32 updater endpoints + minisign pubkey; windows installMode quiet.
- Version compare: REAL semver both layers (updater.rs:9,100,180,318): Layer A verify_manifest
  (:194-206, version↔artifact binding), Layer B candidate_allowed monotonic floor
  (updater_state.rs:203; tests pin 1.0.8-rc.10 > 1.0.7). No lexical compare. Floor state in
  ~/.aztec-accelerator/updater-state.json (deliberately outside config.json).
- **NO channel mechanism**: everything feed-touching is gated is_prerelease==false (:667,:863,:870,
  :982,:1009,:1075). RCs get GitHub prerelease (--prerelease --latest=false :964-971), NEVER touch
  latest.json → fleet can never auto-update to an RC. `gh release list --exclude-pre-releases` is the
  canonical "current fleet version" query.

## Runbook (docs/RELEASE_RUNBOOK.md, 172 ln)

- :28-36 pipeline summary STALE (missing ~half the jobs); CLAUDE.md:13,16 same staleness.
- :91-96 advises gh release delete + tag delete; :98-110 manual latest.json restore via curl+s3 cp —
  no previous latest.json is stored ANYWHERE (not artifact, not S3-versioned). B6 rewrite targets.
- ZERO trust/Keychain/certutil mention — yet trust_{macos,windows}.rs:1-9 defer to "the manual
  pre-release runbook" which THEREFORE DOES NOT EXIST (inputs/03 P2 corroborates).
  PLATFORM_SUPPORT.md:25-34 has the trust-store table (reference, not checklist).

## E2E inventory (for B4)

- Playwright = mocked frontend only (serve -l 3456, tauri-mock). Playground PW = SDK consumer.
- WebDriverIO = REAL app (raw cargo binary --features webdriver, :4445 + :59833 HTTP) — real
  IPC/ACL/CSP but NEVER the installed package; no signing/install-path/registry.
- **No E2E anywhere drives a proof over HTTPS** (trust-boundary PROVE_URL is http; headless-server
  e2e is http; headless stays TLS-free by design). B4's HTTPS leg is genuinely new.
- **Real installs exist in updater-smoke scripts on all 3 OSes** (reusable for installed-desktop
  E2E!): macOS hdiutil attach + ditto to /Applications + xattr -dr quarantine
  (updater-smoke.sh:142-157); Linux AppImage → ~/Applications chmod+run (updater-smoke-linux.sh:
  171-179); Windows silent NSIS /S → %LOCALAPPDATA% with bounded wait (updater-smoke-windows.ps1:
  223-234). All launch installed binary + poll /health; all pre-seed ~/.aztec-accelerator/config.json
  (auto_update:true) — same seam seeds origins/https/speed for upgrade tests.
- Nothing drives the tray (no accessibility automation). Ports hardcoded (59833/4445/3456/8080/443)
  — fine on ephemeral GH runners.
- Self-hosted-hygiene precedents in smoke scripts: run-unique CA CNs keyed GITHUB_RUN_ID+ATTEMPT;
  scoped pkill with documented rationale (linux.sh:79-84).

## Trust-install reality (B4 residual candidate)

- App's own trust targets: macOS LOGIN keychain (trust/macos.rs:16-19, interactive dialog); Windows
  CurrentUser\Root (trust/windows.rs:23, dialog; CI attempts FROZE — empirically documented in
  updater-smoke-windows.ps1:156-164 which uses LocalMachine\Root instead); Linux NSS certutil —
  already fully CI-covered (accelerator.yml:163,179 cert-trust job runs --ignored tests).
- CI smoke DOES manipulate runner trust but DIFFERENT stores for a TLS-impersonation CA:
  macOS System keychain via sudo security add-trusted-cert -d (updater-smoke.sh:113); Windows
  Import-Certificate LocalMachine\Root (ps1:156-164). Note: LocalMachine trust would NOT flip the
  app's own is_ca_trusted() (queries CurrentUser only).
- Non-interactive USER-store install: macOS plausible via fresh login keychain + unlock +
  set-key-partition-list or authorizationdb write trust-settings.admin (NO precedent in repo);
  Windows: no known non-interactive CurrentUser\Root path per repo's own empirical note → honest
  residual, document with codex concurrence per brief.

## config.rs / migration (B4 prerequisite)

- CONFIG_VERSION=1 (:47) written, **never read** (doc :41-46 says so). load_from :127-135 fail-open
  → default() on ANY parse error. No migration fn.
- safari_support DELIBERATELY ignored (tests :303-313, :327-349 pin the drop; framed as owner-agreed
  clean-install for HTTPS-by-default). B4 changes this: honor config_version + alias legacy fields.
- State locations: config ~/.aztec-accelerator/config.json (atomic tmp-rename, 0600/DACL;
  approved_origins INSIDE it); certs ~/.aztec-accelerator/certs/ (keyless CA); updater floor
  updater-state.json; Windows update-txn markers same dir; autostart = OS-native (LaunchAgent /
  HKCU Run + schtasks / systemd user); trust = OS stores; logs = data-local dirs (Windows path
  undocumented).
- Upgrade test must SEED: approved_origins ≥1, https_enabled:true (or legacy safari_support:true),
  speed≠default, auto_update, autostart enabled, CA trusted — and ASSERT all survive. Today's
  smokes assert only launch+/health. NO 1.0.7 profile fixture exists; smoke scripts write minimal
  synthetic config — **mac/linux scripts still write `safari_support` while Windows writes
  `https_enabled`** (drift to fix while touching).

## Obtaining 1.0.7 in CI

- Dynamic latest-stable pattern (mac/linux _e2e-updater*.yml): breaks for pinned-1.0.7 once 2.0.0
  ships (guard fails loud, but can't test 1.0.7→2.0.0 post-release).
- **Pinned + 4-point preflight pattern (_e2e-updater-windows.yml:21-158, n1-version default "1.0.7")
  is THE reusable precedent**: gh release download accelerator-v1.0.7; asserts asset unique, semver
  order via Bun.semver, historical pubkey matches HEAD (gh api contents?ref=tag), historical feed
  endpoint matches. Already handles pre-rename binary name (N1BinaryName, ps1:56-59).
- GitHub Releases IS the fixture store; contents:read suffices.

## Collisions / conventions

- promote-latest.yml NAME TAKEN (SDK npm). Accelerator promote → job inside release-accelerator.yml
  (inherits concurrency) or _-prefixed reusable; don't confuse the two in UI.
- Workflow naming: user-facing plain, workflow_call `_`-prefixed. Composite actions:
  setup-accelerator (heavy; has slimming flags), setup-aztec, start-services, playwright-cache.
  Promote job needs NONE (aws/curl/gh only, mirror release-auth-preflight shape).
- _e2e-updater-windows.yml pulls full setup-accelerator apparently just for bunx tauri signer —
  mac/linux siblings use light setup-bun + install; reconcile rather than propagate.
- actionlint.yaml registers only macos-15-intel; new runner labels must be added or lint:actions
  fails.
- Least-privilege near-total: per-job permissions everywhere; bump-source permissions:{} + GH-App
  token; secrets passed explicitly per-call, never inherit. Match it.
- Runbook/CLAUDE.md staleness must be fixed together or they re-diverge.
