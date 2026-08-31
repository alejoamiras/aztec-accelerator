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

- **Publishes to `testnet`, never to `latest`.** This release path always uses the `testnet` dist-tag; `_publish-sdk.yml` accepts only `testnet` / `nightlies` (guard step; `nightlies` is available solely through that workflow's own direct dispatch), so moving npm `latest` is structurally impossible from any publish and belongs to `promote-latest.yml` (below). A publish therefore never changes what a bare `npm install` resolves — but it does become eligible immediately for any consumer whose range matches (`^X.Y.Z`) on a fresh or unlocked install, since semver ranges ignore dist-tags. Existing lockfiles keep their resolution until updated.
- The reusable `_publish-sdk.yml` runs `npm publish --provenance --access public --tag <dist_tag>` — **Sigstore build provenance** is attached via `id-token: write`. npm authentication uses the `NPM_TOKEN` secret (passed into the reusable workflow); the GitHub release it cuts is always `--latest=false` (the npm dist-tag, not the GitHub "Latest" flag, governs consumer resolution).
- The same run deploys the playground (`deploy-app` job) unless the e2e gate failed. Use `skip_sdk_publish=true` to redeploy the playground after a docs/UI-only change without bumping npm.
- **7-day npm min-age** applies when a lockfile is regenerated locally, with one standing, scoped exception: the first-party `@aztec/*` packages are listed — by exact name, one per line — in `bunfig.toml`'s `minimumReleaseAgeExcludes` (an Aztec bump PR is human-reviewed and CI-gated, and the gate would otherwise stall every bump for a week). A bump that pulls in a NEW `@aztec/*` package needs that exact name added; never extend the list beyond first-party `@aztec/*` names, and never bypass the gate ad hoc. The publish job itself installs `--frozen-lockfile`, so min-age is not a publish-time control.

### Pre-flight (SDK)

- [ ] `bun run --cwd packages/sdk test:lint` (typecheck) + `bun run --cwd packages/sdk test:unit` green; `bun run lint` clean (biome)
- [ ] `@aztec/*` deps resolve to the intended version; `bun.lock` committed (`bun install --frozen-lockfile` passes). `packages/sdk/package.json`'s own `version` is a `0.0.0` placeholder, never hand-bumped.
- [ ] Know the DERIVED publish version before dispatching. `scripts/get-sdk-publish-version.ts` takes the pinned `@aztec/stdlib` version as its base and returns it unchanged only when that exact string is unpublished; if the base is already on npm it appends a revision — `<base>-revision.N` for a stable base, `<base>.N` for a prerelease one (both keep the result valid semver). Run `bun scripts/get-sdk-publish-version.ts <base>` to see which. The two checks below apply to that DERIVED version, not the base.
- [ ] `gh secret list | grep NPM_TOKEN` — if the timestamp is older than ~85 days, rotate BEFORE dispatching. Granular npm tokens expire on a 90-day lifetime and give no warning; the publish simply 404s. This has cost two releases (2026-05-27, 2026-08-26) and is the cheapest check on this list.
- [ ] `npm view @alejoamiras/aztec-accelerator versions dist-tags --json` — confirms the derivation above, and that no other npm mutation is queued or in flight (`gh run list` for `publish-testnet.yml`, `_publish-sdk.yml`, `promote-latest.yml`; they share a non-cancelling concurrency group).
- [ ] `git ls-remote origin 'refs/tags/@alejoamiras/aztec-accelerator@<derived-version>'` empty — the workflow's tag guard fires only AFTER npm publish, so a squatted tag would strand a published version without its release.
- [ ] Consumer-fidelity proof at the dispatch commit — `sdk.yml`'s `tarball-consumer` job green at that SHA. It packs the REWRITTEN manifest; a bare `npm pack` does not (it ships `0.0.0` with source `exports`) and proves nothing about the published shape. To reproduce locally, mirror what CI does, from `packages/sdk` (the rewrite script's manifest argument defaults to a relative path):

```bash
# Subshell + `set -e`: safe to paste interactively — a dirty manifest aborts before any
# mutation, and the manifest is restored when the subshell exits, not when you close the terminal.
( set -euo pipefail
  cd packages/sdk
  git diff --quiet HEAD -- package.json        # refuse to start on a dirty manifest
  trap 'git checkout HEAD -- package.json; rm -f "${TARBALL:-}"' EXIT
  bun run build
  bun ../../scripts/prepare-sdk-publish.ts <derived-version>
  TARBALL="$PWD/$(npm pack --silent)"
  bash ../../scripts/sdk-tarball-consumer.sh "$TARBALL"
)
```

## Promoting the SDK to npm `latest`

`promote-latest.yml` is the ONLY sanctioned path that moves `latest`. It never publishes: it validates the version string, verifies the version already exists on npm, then moves the tag.

```bash
# Make an already-published version the default install:
gh workflow run promote-latest.yml -f version=<NEW_VERSION>

# Roll `latest` back to a previously published version (same workflow — it is the rollback lever):
gh workflow run promote-latest.yml -f version=<PREV_GOOD>
```

- **Precondition**: verify the publish actually landed — `testnet` moved, provenance attestation attached, git tag and GitHub release present — plus whatever acceptance evidence the release was gated on. Record what was checked; the workflow enforces none of it.
- **What rollback does and does not fix.** Moving the tag back changes what NEW bare/`@latest` installs resolve. It does not downgrade anyone already installed, and it does not help range consumers (`^X.Y.Z`) — the published version stays eligible for their fresh or unlocked installs regardless of dist-tags, and a `X.Y.Z-revision.N` fix-forward is a semver *prerelease* their ranges will not match. Fix-forward for those consumers means the next real version.
- **Append-only.** Never delete or re-publish a version to "undo" it; a colliding tag makes the publish step fail loud, and re-cutting the same version is a STOP-and-surface event.

### Verifying a promote (two stale reads that fake a failure)

**The registry read-back lags the write.** After a successful `npm dist-tag add`, `npm view` can
still report the OLD tag for a while — on 2026-08-26 the promote workflow's own `npm view`, running
0.4 s after its own successful `+latest: …@5.2.0`, still printed the previous version, and a local
`npm view` a minute later agreed. Confirm with a read that bypasses caches instead:

```bash
curl -fsSL -H 'Cache-Control: no-cache' 'https://registry.npmjs.org/@alejoamiras%2faztec-accelerator' \
  | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s)["dist-tags"]))'
```

Never roll back on a stale read-back — that would undo a correct promote.

**A local packument cache resolves the OLD `latest`.** A post-promote "fresh install" check on a
machine that queried the package recently silently installs the PREVIOUS release and then fails on
whatever the new version added — a convincing fake regression (it happened here: 5.0.1 installed
minutes after 5.2.0 was promoted). Always revalidate:

```bash
cd "$(mktemp -d)" && npm init -y >/dev/null
npm install @alejoamiras/aztec-accelerator --prefer-online --no-audit --no-fund
node -p 'JSON.parse(require("fs").readFileSync("node_modules/@alejoamiras/aztec-accelerator/package.json","utf8")).version'
node -e 'import("@alejoamiras/aztec-accelerator").then(m => console.log(typeof m.AcceleratorProver, m.ACCELERATOR_API_VERSION))'
```

Read the installed manifest from `node_modules/…` as above: the published `exports` map exposes
only `"."`, so `require("@alejoamiras/aztec-accelerator/package.json")` correctly throws
`ERR_PACKAGE_PATH_NOT_EXPORTED` — a verification script that reaches for an unexported subpath
manufactures failures against a healthy package.

### After any FAILED publish or promote run

Read the registry BEFORE classifying the failure — an npm command can fail after the registry accepted the change (a lost response, a failed trailing step):

```bash
npm view @alejoamiras/aztec-accelerator versions dist-tags --json
```

**Failed publish.** Version absent + auth-shaped error (E401/E403/E404) → credential problem. npm answers **404, not 403**, for an unauthorized write to a scoped package, so "could not be found" here means permission, never a missing package — do not go looking for a naming or registry-config bug. Check the secret's AGE first: `gh secret list` prints last-updated timestamps (metadata only, no values). Both occurrences of this failure (2026-05-27, 2026-08-26) were `NPM_TOKEN` reaching the end of a 90-day granular-token lifetime — on the second, the secret was 91 days old and the last successful publish had been at day 83, which settled the diagnosis in one read-only call. Rotate the secret, never blind-retry. Version present but a later leg failed → do NOT redispatch the publish (it would mint a revision suffix — `-revision.N` or `.N` per the derivation rule above); repair only the failed leg, and treat a missing tag/release as STOP-and-surface.

**Failed promote.** The tag move can succeed and the run still go red on a trailing step. Judge by the *mutation's own output*, not by a read-back: a landed move prints `+latest: <pkg>@<version>` in the `Point npm latest at the version` step. Printed → the mutation landed, do NOT re-dispatch (investigate the trailing failure only). Not printed → the move did not land; safe to re-dispatch once the cause is understood. A read showing a THIRD version → STOP and surface before any further mutation. Note that a read showing the OLD version proves nothing on its own — see the lag below.

## Rollback Procedure (accelerator app)

For the SDK on npm, rollback is the `promote-latest.yml` dispatch documented above — this section covers the
desktop app's signed update feed only.

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
