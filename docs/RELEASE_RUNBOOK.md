# Release Runbook

This repo ships **two independently releasable artifacts**, and most releases touch only one:

| Artifact | What it is | Released by | When |
|---|---|---|---|
| **SDK** (`@alejoamiras/aztec-accelerator`) | npm package dApps import | `publish-testnet.yml` → `_publish-sdk.yml` | An `@aztec/*` bump, or an SDK code/feature change |
| **Accelerator** (desktop + headless) | The native-proving app/binary | `release-accelerator.yml` (tag + GitHub release + `latest.json`) | The accelerator's **own** code changes (server, tray, updater, bb download/verify) |

**Decision — SDK-only vs full accelerator release:** because the accelerator downloads `bb` at runtime, an Aztec protocol bump is almost always **SDK-only** — re-publish the SDK and you're done; installed accelerators fetch the matching `bb` on the next prove request (see [accelerator README → Version Model](../packages/accelerator/README.md#version-model--why-an-aztec-bump-doesnt-re-release-this-app)). Cut a full accelerator release **only** when the accelerator's own Rust code changed. The accelerator release is documented first below, then the [SDK publish](#releasing-the-sdk-to-npm).

## Pre-flight Checklist

- [ ] All CI checks green on `main`
- [ ] `bun run test` passes locally (lint + typecheck + unit tests)
- [ ] `cargo test --lib` passes in `packages/accelerator/src-tauri/`
- [ ] No open "P0" issues on the milestone
- [ ] Version number decided (semver: `MAJOR.MINOR.PATCH` or `MAJOR.MINOR.PATCH-rc.N`)

## Cutting a Release

B6: **publish and promote are two separate dispatches.** A `publish` run builds, gates, and publishes the
GitHub release (with its signed `latest.json` asset) but does NOT touch the live auto-updater feed. A
separate `promote-only` run flips the S3 `latest.json` feed to that version — the only step that changes
what installed clients auto-update to. `promote-only` is also the rollback lever (see Rollback Procedure).

### 1. Publish (build + gate + publish — NO feed flip)

```bash
gh workflow run release-accelerator.yml -f version=X.Y.Z            # mode=publish is the default
```

Publish pipeline: **validate → e2e-webdriver gate → build (3 Tauri + 4 headless) → smoke → tag →
sign-update-feed (stable only) → release**. A prerelease (`X.Y.Z-rc.N`) publishes as a public GitHub
prerelease (`--latest=false`, no feed). A stable publishes the release + attaches the signed `latest.json`
as a release asset, but the LIVE feed is unchanged until you promote. Failed gates never reach `release`.

### 2. Promote (flip the live feed)

After verifying the published stable (and, for a GA, soaking the RC), flip the feed:

```bash
# Rehearse first — the whole pre-flight, no write, zero prod effect:
gh workflow run release-accelerator.yml -f version=X.Y.Z -f mode=promote-only -f dry_run=true
# Organic GA: flip the feed AND open the source version-bump PR:
gh workflow run release-accelerator.yml -f version=X.Y.Z -f mode=promote-only -f bump_source=true
```

`promote` re-verifies the published release from scratch (exists, non-draft, non-prerelease stable; full
installer + `latest.json` asset set; production Ed25519 verifier over the release's OWN `latest.json`; feed
version == dispatched; every payload URL resolves) BEFORE uploading those exact bytes to S3 + invalidating
CloudFront. `verify-live-feed` then confirms the public CDN actually serves the new version.

### 2. Post-release verification

- [ ] GitHub Release page has all 6 expected assets:
  - `Aztec-Accelerator-X.Y.Z-macOS-Apple-Silicon.dmg`
  - `Aztec-Accelerator-X.Y.Z-macOS-Intel.dmg`
  - `Aztec-Accelerator-X.Y.Z-macOS-Apple-Silicon.app.tar.gz`
  - `Aztec-Accelerator-X.Y.Z-macOS-Intel.app.tar.gz`
  - `Aztec-Accelerator-X.Y.Z-Linux-x86_64.deb`
  - `Aztec-Accelerator-X.Y.Z-Linux-x86_64.AppImage`
- [ ] `latest.json` is live: `curl https://aztec-accelerator.dev/releases/latest.json`
  - Verify `version` field matches
  - Verify all `signature` fields are non-empty
  - Verify all `url` fields resolve (HTTP 200/302)
- [ ] Download a DMG, open it, verify the app launches and the tray icon appears
- [ ] Check "About" info in tray menu shows correct version
- [ ] If updating from a previous version: verify the auto-updater detects the new version
- [ ] Verify macOS notarization: `spctl --assess --verbose /Applications/Aztec\ Accelerator.app`
- [ ] Verify updater signatures are valid (non-empty in latest.json, app accepts update)

### Automated artifact checks

The release workflow already asserts all 6 expected files exist before creating the GitHub Release. The `latest.json` is generated from the `.sig` files produced by Tauri's Ed25519 signing step. If signing fails, the `.sig` files will be missing and `latest.json` will have empty signatures — the auto-updater will reject the update (signature verification is mandatory in tauri-plugin-updater).

### 3. Merge the version-bump PR

The release workflow creates a PR bumping the source version to the next RC. Merge it promptly so `main` always reflects the next development version.

## Releasing the SDK to npm

The SDK publishes via `publish-testnet.yml` (manual `workflow_dispatch`), which both publishes the SDK and deploys the playground.

```bash
# Publish the SDK (runs the e2e gate first) + deploy the playground:
gh workflow run publish-testnet.yml

# Deploy the playground ONLY — skip re-publishing the SDK to npm:
gh workflow run publish-testnet.yml -f skip_sdk_publish=true
```

- **dist-tag `testnet`, `latest:false`.** While the `@aztec/*` deps are release-candidate-labeled (`5.0.0-rc.N`), the SDK ships on the **`testnet`** dist-tag and never npm `latest` — `latest` stays on the last stable line. Consumers opt in with `npm install @alejoamiras/aztec-accelerator@testnet`.
- The reusable `_publish-sdk.yml` runs `npm publish --provenance --access public --tag <dist_tag>` — **Sigstore build provenance** is attached via `id-token: write`. npm authentication uses the `NPM_TOKEN` secret (passed into the reusable workflow); the GitHub release it cuts is always `--latest=false` (the npm dist-tag, not the GitHub "Latest" flag, governs consumer resolution).
- The same run deploys the playground (`deploy-app` job) unless the e2e gate failed. Use `skip_sdk_publish=true` to redeploy the playground after a docs/UI-only change without bumping npm.
- **7-day npm min-age** still applies to the SDK's own dependencies — never override the gate in CI.

### Pre-flight (SDK)

- [ ] `bun run --cwd packages/sdk test:lint` (typecheck) + `bun run --cwd packages/sdk test:unit` green; `bun run lint` clean (biome)
- [ ] SDK version in `packages/sdk/package.json` bumped (the rc / `testnet` line)
- [ ] `@aztec/*` deps resolve to the intended version; `bun.lock` committed (`bun install --frozen-lockfile` passes)

## Rollback Procedure

B6 is **append-only**: NEVER delete a published release or tag, and never hand-upload a feed. Rollback is a
FEED operation — you move the live `latest.json` back to a previous good version with a `promote-only`
dispatch (the same job that promoted forward). This stops NEW auto-update uptake and moves the landing
download back (landing reads the feed); already-updated clients are not downgraded.

### 1. Roll the feed back to the last good version

```bash
# Rehearse first — pre-flight only, no write:
gh workflow run release-accelerator.yml -f version=<PREV_GOOD> -f mode=promote-only -f dry_run=true
# Then roll back for real. bump_source STAYS false — a rollback must NOT bump the source version:
gh workflow run release-accelerator.yml -f version=<PREV_GOOD> -f mode=promote-only
```

`promote` re-verifies `<PREV_GOOD>`'s published release + signed feed before flipping, so you can only roll
back to a version that still has an intact, verifiable release (which append-only guarantees still exists).
`verify-live-feed` confirms the CDN serves `<PREV_GOOD>`.

### 2. Fix forward (never re-cut the bad version)

The bad `X.Y.Z` release stays on GitHub (append-only). Ship the fix as the NEXT version `X.Y.(Z+1)` through
the normal publish → promote flow. Do NOT delete the bad release/tag and do NOT re-publish `X.Y.Z` — the
publish step fails loud on a colliding tag. (Re-cutting the SAME version is a STOP-and-surface event, not an
autonomous step.)

### 3. Communicate

- Post in relevant Aztec channels that the feed was rolled back to `<PREV_GOOD>`.
- File a GitHub issue documenting what went wrong; land the fix as `X.Y.(Z+1)`.

## User Diagnostics

### Log location

| Platform | Path |
|----------|------|
| macOS | `~/Library/Application Support/aztec-accelerator/logs/` |
| Linux | `~/.local/share/aztec-accelerator/logs/` |

Logs rotate daily, keeping the last 7 files. Log level defaults to `info`; set `RUST_LOG=debug` for verbose output.

### Config location

| Platform | Path |
|----------|------|
| Both | `~/.aztec-accelerator/config.json` |

### Common issues

**"Port 59833 already in use"**: Another instance is running, or another process is using the port. Kill it with `lsof -i :59833` and restart.

**"bb binary not found"**: The bundled sidecar is missing. Re-install the app from the DMG/deb.

**"Cannot verify bb download"**: The GitHub API is unreachable or the release doesn't have a digest. The bundled bb version still works; only on-demand version downloads require verification.

**macOS: "app is damaged"**: Gatekeeper quarantine. Run: `xattr -cr /Applications/Aztec\ Accelerator.app`

**Auto-updater not working**: Check `latest.json` is accessible at `https://aztec-accelerator.dev/releases/latest.json`. Verify the `signature` fields are non-empty. Check app logs for updater errors.

### Collecting logs from users

Ask the user to:
1. Open the tray menu → "Show Logs"
2. Copy the latest log file and share it in the issue

Or manually: `cat ~/Library/Application\ Support/aztec-accelerator/logs/aztec-accelerator.log.*`

## Signing & Notarization

### Ed25519 updater signing
- Private key: `TAURI_SIGNING_PRIVATE_KEY` GitHub secret
- Public key: in `tauri.conf.json` → `plugins.updater.pubkey`
- Password: `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` GitHub secret

### Apple notarization
- Certificate: `APPLE_CERTIFICATE` GitHub secret (base64-encoded .p12)
- Signing identity: `APPLE_SIGNING_IDENTITY`
- Apple ID + app-specific password: `APPLE_ID`, `APPLE_PASSWORD`
- Team ID: `APPLE_TEAM_ID`

### Verifying notarization

```bash
spctl --assess --verbose /Applications/Aztec\ Accelerator.app
# Should output: accepted, source=Notarized Developer ID
```
