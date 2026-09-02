# Release runbook

This repository ships two independently versioned artifacts:

| Artifact | Release entry point | Use it when |
|---|---|---|
| SDK (`@alejoamiras/aztec-accelerator`) | `release-sdk.yml` | The SDK or pinned `@aztec/*` dependencies changed |
| Desktop + headless accelerator | `release-accelerator.yml` | Native server, desktop UI, updater, trust, or bb download logic changed |

An Aztec protocol bump is normally SDK-only. Installed accelerators download and verify the matching `bb` version at runtime; do not cut a native-app release merely to track an `@aztec/*` bump.

## One-time production configuration

Keep the release setup small: neither GitHub environment requires reviewers, but both restrict deployment branches to `main`. For this solo-maintainer repository, environments scope secrets and OIDC claims to release jobs; they are not independent approval boundaries. A commit already trusted on `main` can change a workflow that consumes an environment secret. Add a reviewer or external signing service only if that stronger threat model becomes necessary.

### `release-signing` GitHub environment

Store this required environment secret:

- `TAURI_SIGNING_PRIVATE_KEY`

`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is optional. Omit it when the production key is passwordless; the workflow passes an empty value and the signer supports that configuration. Set it only when the stored private key was generated with that exact password.

The production updater key and its optional password must not also exist as repository-level secrets. `release-accelerator.yml` exposes these values only to the dedicated signing step, whose commands are intentionally narrow and use the installed, lockfile-pinned Tauri CLI directly. Apple signing/notarization credentials remain separate because they are required by the macOS build jobs; this change does not isolate those build-time credentials.

#### Accelerator 3 updater-key rotation

Accelerator 3 rotates the production updater key to public key ID `456E5A3DB518F598`. The matching
passwordless private key is stored in the Personal-vault item
`Aztec Accelerator Updater Signing Key v3 (passwordless)` and in the `release-signing` environment only.
The older `Tauri Signing Key` item is retained as historical evidence and must not be used.

In that 1Password Login item, `username` records the public key and `password` contains the private-key
payload. “Passwordless” means the updater private key has an empty encryption passphrase; it does **not** mean
the 1Password `password` field is empty. GitHub's `TAURI_SIGNING_PRIVATE_KEY` must be populated from that
`password` field, while `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` remains unset.

This is an intentionally breaking updater migration: 1.x and 2.x binaries pin the old public key and cannot
authenticate any 3.x feed. The 3.0.0 changelog and generated GitHub release notes must therefore tell those
users to quit the accelerator and manually install 3.0.0 over the existing application. A prior uninstall is
normally unnecessary and risks removing integration state; install-over preserves configuration and cached bb
versions. After the manual reinstall, 3.x-to-3.x automatic updates work normally.

Before publishing any 3.0.0 candidate, require the isolated signer job to prove that all updater payloads and
the generated feed verify against the public key committed in `tauri.conf.json`. Never work around a mismatch
with an HTTP feed, unsigned payload, alternate public key, or hand-edited release artifact.

### `npm-publish` GitHub environment and npm trusted publisher

The environment has no npm secret. Configure the package's [npm GitHub Actions trusted publisher](https://docs.npmjs.com/trusted-publishers/) exactly as follows:

| Field | Value |
|---|---|
| Organization or user | `alejoamiras` |
| Repository | `aztec-accelerator` |
| Workflow filename | `release-sdk.yml` |
| Environment | `npm-publish` |
| Allowed action | `npm publish` only |

`_publish-sdk.yml` is intentionally `workflow_call`-only. npm validates the calling workflow name for reusable workflows, so the trusted-publisher filename is `release-sdk.yml`; both caller and called workflow grant `id-token: write`.

For the first OIDC canary, leave the existing `NPM_TOKEN` stored but unused. Confirm the workflow contains no token reference:

```bash
rg 'NPM_TOKEN|NODE_AUTH_TOKEN' .github/workflows/release-sdk.yml .github/workflows/_publish-sdk.yml
```

The command must return no matches. After one successful OIDC publish and provenance verification, delete the obsolete automation token/secret. Do not add a token fallback: a fallback would hide a broken trusted-publisher binding.

## Preflight for every release

- Work from a clean `main` checkout at the intended commit.
- Require all CI checks to be green.
- Confirm no release workflow is queued or running.
- Run the repository checks:

```bash
bun install --frozen-lockfile
bun run test
bun run lint:actions
bun run audit:dependencies
bun run --cwd packages/accelerator frontend:build
cargo test --locked --manifest-path packages/accelerator/core/Cargo.toml
cargo test --locked --manifest-path packages/accelerator/server/Cargo.toml
cargo test --locked --manifest-path packages/accelerator/src-tauri/Cargo.toml
```

`audit:dependencies` combines `bun audit` with `cargo audit` over all three Rust lockfiles. Both release workflows run the same audit as a publication gate. New npm high/critical findings and every RustSec vulnerability block. npm moderate/low findings and RustSec informational warnings are reported. A blocking finding may be accepted only in `scripts/dependency-audit-allowlist.json` with an exact package/advisory pair, rationale, upgrade path, and future expiry. Never extend an expired exception just to make a release green.

## Releasing the accelerator

Publishing and promotion are separate, serialized events. Publish creates a tested GitHub release but never changes the live updater feed. Promotion verifies an already-published stable release and moves the feed; the same operation is the rollback lever.

### 1. Publish

```bash
gh workflow run release-accelerator.yml --ref main -f version=X.Y.Z
# or prerelease:
gh workflow run release-accelerator.yml --ref main -f version=X.Y.Z-rc.N
```

The first release after an intentional updater-key rotation is the only exception to the ordinary N-1 update
smoke. It must be `rc.1` of exactly the next major and requires an explicit bootstrap flag:

```bash
gh workflow run release-accelerator.yml --ref main \
  -f version=X.0.0-rc.1 -f updater_key_rotation_bootstrap=true
```

The resolver fails closed unless there is no complete lower release under the new key. In bootstrap mode, the
macOS gate installs the newest complete old-key release and requires it to reject the authentic new-key
manifest with `SignatureInvalid` while remaining healthy at N-1. This proves the manual-reinstall boundary;
it does not pretend that an impossible cross-key automatic update succeeded. Publish `X.0.0-rc.2` normally
afterward. The resolver then selects RC1 as a same-key baseline and restores every macOS, Linux, and Windows
positive/tamper updater smoke. Never use the bootstrap flag for RC2, GA, a retry under an existing same-key
baseline, or an ordinary release.

The release path is:

```text
validate/main-only + AWS preflight + 3-OS WebDriver
  → 4 desktop builds + 4 headless builds
  → isolated production updater signing
  → resolve the greatest lower complete same-key updater baseline
  → notarization, launch, updater, and tamper-rejection smokes
  → draft release
  → packaged E2E against the draft's own assets
  → tag the dispatched commit
  → re-check asset digests and publish the draft
```

Desktop builds use fresh throwaway updater keys so a new binary can be produced without exposing the production key. Their temporary signatures are excluded. The `release-signing` job then signs the four exact updater payloads with the production key and verifies every payload and feed against the embedded public key. That job does not build, install, launch, or smoke-test applications. Smokes consume only the pre-signed artifacts. The updater baseline may be a prerelease: this is intentional, so RC2 exercises RC1 under the rotated key and GA exercises the newest same-key RC instead of falling back to an older incompatible key.

A prerelease is public with `--latest=false` and omits `latest.json`. A stable release includes `latest.json`, but publishing still does not write S3 or alter what installed clients receive.

### Expected assets

Every release has 16 binary assets:

- two macOS DMGs and two macOS updater `.app.tar.gz` files;
- Linux `.deb` and `.AppImage` files;
- Windows first-install `.exe` and updater `.nsis.zip` files;
- four headless server `.tar.gz` files, each with a `.sha256` sidecar.

A stable release has a seventeenth asset: signed `latest.json` containing exactly `darwin-aarch64`, `darwin-x86_64`, `linux-x86_64`, and `windows-x86_64`.

The Windows installer is deliberately not Authenticode-signed. SmartScreen therefore shows **Unknown publisher** on first install; users select **More info → Run anyway**. This is an accepted distribution limitation, not a release blocker. The updater payload is independently Ed25519-signed and verified before application.

### 2. Verify the published release

- Confirm the workflow finalized the draft and the Git tag resolves to the dispatched commit.
- Confirm the exact asset count: 16 for a prerelease, 17 for stable.
- Confirm macOS signature and notarization jobs passed.
- Confirm macOS, Linux, and Windows updater smokes passed, including negative tamper controls.
- For a stable, download its `latest.json` asset and confirm its version, four platform entries, non-empty signatures, sizes, and exact release URLs.
- For Windows trust/certificate/HTTPS changes, complete the manual Windows composed-proof check in the [accelerator README](../packages/accelerator/README.md#windows-composed-proof--manual-pre-ga-check).

Do not promote a release that needs unexplained retries or manual asset replacement. Published releases and tags are append-only; fix forward with a new version.

### 3. Promote the live updater feed

Rehearse the exact validation without writing production:

```bash
gh workflow run release-accelerator.yml --ref main \
  -f version=X.Y.Z -f mode=promote-only -f dry_run=true
```

For an ordinary GA, flip the feed and open the source-version bump PR:

```bash
gh workflow run release-accelerator.yml --ref main \
  -f version=X.Y.Z -f mode=promote-only -f bump_source=true
```

Promotion independently requires a published, non-draft, non-prerelease release with the exact 17 assets. It verifies the release's own signed manifest, exact platform URLs, and reachable payloads before uploading those same manifest bytes. The S3 write is authoritative; CloudFront invalidation is best effort, and a separate job polls the public feed until it serves the requested version and passes cryptographic verification.

Merge the source-version bump PR after an organic GA. Never request `bump_source` for a rollback.

### Accelerator rollback

Move the live feed back to an intact previous stable; do not delete or rebuild any release:

```bash
gh workflow run release-accelerator.yml --ref main \
  -f version=<PREVIOUS_GOOD> -f mode=promote-only -f dry_run=true

gh workflow run release-accelerator.yml --ref main \
  -f version=<PREVIOUS_GOOD> -f mode=promote-only
```

This stops new updater uptake and moves landing-page downloads back. It does not downgrade clients that already updated. Fix forward under the next version.

## Releasing the SDK candidate

`release-sdk.yml` has one manual entry point and three modes:

```bash
# Default: publish SDK candidate, then deploy the testnet playground
gh workflow run release-sdk.yml --ref main -f mode=sdk-and-playground

# Publish only
gh workflow run release-sdk.yml --ref main -f mode=sdk-only

# Deploy playground only; no npm mutation
gh workflow run release-sdk.yml --ref main -f mode=playground-only
```

`testnet` is the npm candidate dist-tag used by the public testnet playground. It is not an npm network or a lesser form of the package. There is no separate `mainnet` publish path today: accepted candidates are deliberately promoted from `testnet` to npm's default `latest` tag. The old npm nightly publish path is retired; the historical `nightlies` dist-tag is left untouched.

### Candidate version and gates

The SDK package's checked-in version remains `0.0.0`. The workflow derives a version from the pinned `@aztec/stdlib` version. If the base already exists, it chooses `<base>-revision.N` for a stable base or appends `.N` to a prerelease base.

Preview the derived version:

```bash
AZTEC_VERSION=$(node -p "require('./packages/sdk/package.json').dependencies['@aztec/stdlib']")
bun scripts/get-sdk-publish-version.ts "$AZTEC_VERSION"
```

Before dispatching, verify the derived npm version, matching Git tag, and GitHub release are all absent. The workflow repeats these checks, builds and rewrites the package manifest, packs one exact tarball, runs the consumer test against that tarball, and publishes those bytes with OIDC to `testnet`.

After npm accepts the package, the workflow requires all of the following before creating the tag/release:

- the exact version is readable from npm;
- `testnet` points to it;
- npm's current verifier cryptographically validates registry signatures and the SLSA attestation for the exact installed package;
- the verified attestation subject digest matches npm's exact tarball integrity, and its source dependency identifies `alejoamiras/aztec-accelerator`, `.github/workflows/release-sdk.yml`, `refs/heads/main`, and the dispatched commit.

It then tags the commit, creates a non-latest GitHub release, and verifies a fresh registry install. npm publication is irreversible, so if npm accepted the package but a later step failed, do not redispatch blindly; inspect and repair only the missing record.

### First OIDC canary

For the first run after enabling trusted publishing:

1. Double-check the npm trusted-publisher fields and `npm publish` allowed action.
2. Keep the old token stored but ensure it is not referenced by either workflow.
3. Dispatch `sdk-only` from `main`.
4. Confirm the publish step reports trusted publishing/OIDC, `testnet` moved, provenance passes, the Git tag/release exist, and a clean install succeeds.
5. Remove the old automation token from GitHub and revoke it on npm.

An authentication failure before npm contains the derived version is safe to retry after fixing the trusted-publisher configuration. npm does not validate the configuration when it is saved, so filename, owner, repository, environment, runner type, and `id-token: write` are the first things to inspect.

## Promoting the SDK to `latest`

Promotion is intentionally local and interactive so proof of presence/2FA stays with the maintainer:

```bash
npm login
bun run sdk:promote -- <VERSION> --dry-run
bun run sdk:promote -- <VERSION>
```

The script refuses to mutate npm unless:

- no `release-sdk.yml` run is queued or active;
- the version exists and `testnet` points to it;
- provenance matches this repository, `release-sdk.yml`, `main`, and a concrete commit;
- `npm audit signatures` cryptographically verifies the exact package's SLSA attestation;
- the remote Git tag resolves to that provenance commit;
- the matching GitHub release exists.

The non-dry run prints the evidence, asks for `y`, then immediately rechecks active workflows and uncached `latest`/`testnet` state before executing `npm dist-tag add` and reading back `latest`. npm has no compare-and-swap operation for dist-tags, so this deliberately small residual race is accepted for the solo-maintainer workflow; do not run two promotion commands concurrently. This needs a locally authenticated npm identity with package write access and the account's 2FA policy. npm access tokens do not have a promotion-only permission; the safety boundary is the interactive script's checks, dry run, confirmation, and absence of a CI token.

Moving a dist-tag changes new bare/`@latest` installs only. It does not remove the candidate or change consumers already pinned by a lockfile or semver range. Rollback is explicit, must target a lower version, and does not require the old version to remain on `testnet`:

```bash
bun run sdk:promote -- <PREVIOUS_GOOD> --rollback --dry-run
bun run sdk:promote -- <PREVIOUS_GOOD> --rollback
```

Rollback accepts provenance from the current `release-sdk.yml` identity or the retired, exact `publish-testnet.yml` identity so pre-migration releases remain usable. Ordinary forward promotion accepts only `release-sdk.yml` provenance and still requires `testnet` to identify the candidate.

Never delete or re-publish an npm version. Fix forward under a new derived revision/version.

## Failure classification

Always read external state before retrying:

```bash
npm view @alejoamiras/aztec-accelerator versions dist-tags --json
gh run list --workflow release-sdk.yml --limit 20
```

- **Version absent:** no npm publish landed. Fix the root cause, then redispatch.
- **Version present, `testnet` missing/wrong:** stop and inspect the publish output and registry state; do not mint another revision automatically.
- **Version and `testnet` correct, tag/release missing:** publication landed. Do not redispatch; repair the missing Git/GitHub record only after matching it to provenance.
- **Promotion command printed the successful `+latest` mutation but read-back failed:** do not immediately reverse it. Inspect the uncached registry state first; registry reads can lag writes.
- **Unexpected third version/tag:** stop all mutation and investigate.

For the partial-publish case, first run both verifiers and record the provenance commit printed by the first command:

```bash
bun scripts/sdk-release-verification.ts <VERSION>
bun scripts/verify-sdk-package-signatures.ts <VERSION>
```

Then confirm the exact tag and release are absent, fetch the printed commit from `origin`, and inspect it before creating anything. Only when the package digest, source dependency, workflow, branch, and commit all match may you repair the append-only records:

```bash
TAG="@alejoamiras/aztec-accelerator@<VERSION>"
COMMIT="<PROVENANCE_COMMIT>"
git fetch origin main
git show --stat "$COMMIT"
git ls-remote --exit-code origin "refs/tags/$TAG" && exit 1 || true
gh release view "$TAG" && exit 1 || true
git tag "$TAG" "$COMMIT"
git push origin "refs/tags/$TAG"
gh release create "$TAG" --title "$TAG" --notes "Recovered the release record for the provenance-verified npm package." --latest=false packages/sdk/MIGRATION.md
```

Those last three commands mutate public state. Run them only as a deliberate repair after the read-only checks; never use them to replace an existing tag or release.

## User diagnostics

| Platform | Logs |
|---|---|
| macOS | `~/Library/Application Support/aztec-accelerator/logs/` |
| Linux | `~/.local/share/aztec-accelerator/logs/` |
| Windows | `%LOCALAPPDATA%\\aztec-accelerator\\logs\\` |

Configuration is stored in `~/.aztec-accelerator/config.json` on macOS/Linux and the equivalent user profile location on Windows.

- **Port 59833 in use:** another accelerator instance or local process owns the HTTP listener. Inspect it before terminating anything.
- **bb unavailable:** inspect the health payload and logs; versioned proof requests should trigger a verified on-demand download.
- **bb verification failed:** preserve the logs. Runtime downloads fail closed when the upstream digest is missing or mismatched.
- **Updater failure:** inspect the published release's `latest.json`, exact platform URL, payload size/signature, and application logs. Never work around it by hand-editing the live feed.
