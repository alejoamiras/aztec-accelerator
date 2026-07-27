# C4 / F-004 Implementation Plan

## Decision

Implement both layers without forking `tauri-plugin-updater`:

1. Verify an embedded, canonically serialized minisign manifest immediately after `Updater::check()` returns and before the update is logged as accepted, stored, prompted, downloaded, or installed.
2. Persist a one-way version floor in a dedicated atomic owner-only state file. A release build raises it only after its own `/health` endpoint repeatedly reports the running version.

Use the existing updater key for both artifact and manifest signatures. A separate manifest key adds rotation and secret-management complexity without creating a two-key threshold: a compromised manifest key could still authorize any old publicly available artifact/signature pair.

## Verified code constraints

- `tauri-plugin-updater` is `2.10.1`: `packages/accelerator/src-tauri/Cargo.lock:4792-4817`; `minisign-verify` is `0.2.5`: `Cargo.lock:2509-2512`.
- `Updater::check()` compares unsigned `release.version` before creating `Update`; the plugin has a version comparator but no feed-signature hook.
- `Update` publicly exposes `version`, `date`, `download_url`, `signature`, and parsed `raw_json`; `download()` verifies artifact bytes and `install()` installs them.
- `raw_json` is not the original HTTP bytes. It is a cloned `serde_json::Value`; whitespace, object order, and duplicate-key history are already lost. “Exact match” must therefore mean strict semantic equality with the parsed value.
- Artifact verification happens only after the entire response is buffered. `download()`’s callback cannot abort: current SEC-03 residual remains.
- The pinned key is `packages/accelerator/src-tauri/tauri.conf.json:14-21`.
- The private key currently enters every desktop build at `.github/workflows/release-accelerator.yml:154-165`.
- Feed creation and AWS/GitHub publishing currently share the privileged `release` job at `release-accelerator.yml:566-824`.
- `config::save_to` provides the existing temp-write/0600 pattern at `packages/accelerator/core/src/config.rs:135-176`, but malformed config silently defaults at `:89-107`; therefore the security floor must not be an `AcceleratorConfig` field.
- Startup re-arms crash recovery before spawning HTTP at `packages/accelerator/src-tauri/src/main.rs:471-477,532-538`. `.setup()` returning does not prove that the server bound.
- `version_policy.rs` governs Aztec/bb cache versions, not application SemVer. Do not reuse its custom sorting.
- Tauri documents the existing updater key/static-feed format and private-key environment variables in the [official updater documentation](https://v2.tauri.app/plugin/updater/).

The active checkout is `chore/security-audit-2026-07-09`, while the requested worktree branch `sechard/updater-rollback` is at `a6cd29b`. Relevant application code is equivalent, but the target branch contains C3’s SHA-pinned Actions; implementation must switch to the target branch and preserve those pins.

## Signed feed contract

Keep the plugin-compatible top-level fields and add one ignored envelope:

```json
{
  "version": "1.0.7",
  "notes": "Aztec Accelerator 1.0.7",
  "pub_date": "2026-07-14T12:00:00Z",
  "platforms": {
    "linux-x86_64": {
      "url": "https://github.com/.../artifact.AppImage",
      "size": 12345678,
      "signature": "<artifact .sig contents>"
    }
  },
  "manifest_envelope": {
    "manifest": {
      "schema": "aztec-accelerator-update-manifest-v1",
      "version": "1.0.7",
      "pub_date": "2026-07-14T12:00:00Z",
      "platforms": {
        "linux-x86_64": {
          "url": "https://github.com/.../artifact.AppImage",
          "size": 12345678,
          "signature": "<artifact .sig contents>"
        }
      }
    },
    "signature": "<manifest .sig contents>"
  }
}
```

Rules:

- `manifest`, platform entries, envelope, and top-level feed are typed with `#[serde(deny_unknown_fields)]`.
- `size` is mandatory `u64`; floats, negative numbers, strings, or overflow fail.
- `version` must parse as canonical SemVer and serialize back byte-identically; reject leading `v` and noncanonical forms even though the plugin accepts them.
- `pub_date` is mandatory RFC 3339 and must match top-level exactly.
- Platform keys use `BTreeMap`, giving lexical ordering.
- Canonical bytes are compact UTF-8 JSON in this exact struct field order:
  `schema`, `version`, `pub_date`, `platforms`; platform fields `url`, `size`, `signature`; append exactly one LF.
- A single Rust canonicalizer is used by both the release job and application. Do not reproduce canonicalization in `jq`, PowerShell, or JavaScript.
- Top-level `version`, `pub_date`, and the entire `platforms` value must equal the signed manifest semantically. `notes` remains informational and unsigned.
- Exactly one signed platform entry must match `Update.download_url` and `Update.signature`; reject zero or ambiguous matches.
- Require `Update.version == manifest.version`.

Embed the envelope instead of publishing `latest.json.sig`. A sidecar would require a second fetch, introduce feed/signature mix-and-match timing, and cannot be obtained through the plugin. The embedded unknown field survives in `Update.raw_json`.

## Minisign verification

Add direct dependencies in `packages/accelerator/src-tauri/Cargo.toml`:

```toml
minisign-verify = "=0.2.5"
semver = "1"
```

Use the same transformations as the plugin, except reject legacy non-prehashed signatures:

```rust
let public_text = String::from_utf8(STANDARD.decode(pinned_pubkey.trim())?)?;
let public_key = minisign_verify::PublicKey::decode(&public_text)?;

let signature_text =
    String::from_utf8(STANDARD.decode(envelope.signature.trim())?)?;
let signature = minisign_verify::Signature::decode(&signature_text)?;

public_key.verify(&canonical_manifest_bytes, &signature, false)?;
```

`false` requires the prehashed signature format generated by the current Tauri 2 signer. The signing-job verification gate proves compatibility with the real secret before publication.

Avoid duplicating the pinned public key:

- Extend `packages/accelerator/src-tauri/build.rs` to parse `tauri.conf.json`, extract `plugins.updater.pubkey`, validate it, and emit `cargo:rustc-env=AZTEC_UPDATER_PUBKEY=...`.
- Add `cargo:rerun-if-changed=tauri.conf.json`.
- Production verification uses `env!("AZTEC_UPDATER_PUBKEY")`; tests pass a fixture key explicitly.

The plugin comparator is not the verification seam: it receives `RemoteRelease`, not `raw_json` or the envelope, and is invoked before `Update` exists. Verification belongs immediately after `updater.check().await`.

Introduce a private `VerifiedUpdate` wrapper holding the plugin `Update`, parsed SemVer, selected signed size, and canonical-manifest digest. Only the verifier can construct it; `perform_update` accepts only `VerifiedUpdate`.

## Phase 0 — Contracts and characterization

Files:

- Add `packages/accelerator/src-tauri/src/update_manifest.rs`.
- Add `packages/accelerator/src-tauri/examples/update-manifest.rs` as the shared release canonicalize/assemble/verify tool. Do not add `src/bin/*`; `Cargo.toml:8-16` explains why extra binaries can corrupt macOS bundle shape.
- Export the module from `src-tauri/src/lib.rs`.
- Add fixture public key, canonical manifest, and signature under `src-tauri/tests/fixtures/updater/`.

Tests first:

- Golden canonical bytes, including exact LF.
- Platform insertion order does not change bytes.
- Mutating version, date, URL, size, artifact signature, schema, or platform key fails.
- Missing/unknown fields, floats, duplicate selected URLs, and malformed base64 fail.
- Top-level/envelope mismatch fails even when the envelope signature is valid.
- Known fixture signature verifies; every one-byte canonical mutation fails.
- SemVer matrix: `rc.2 < rc.10 < stable`, next-patch RC greater than prior stable, equal/build-only versions rejected.

Gate:

```bash
cd packages/accelerator/src-tauri
cargo fmt --check
cargo test update_manifest -- --nocapture
cargo clippy --all-targets -- -D warnings
cargo run --example update-manifest -- verify-feed \
  tests/fixtures/updater/latest.json \
  tests/fixtures/updater/public.key
```

## Phase 1 — Runtime acceptance gate

Modify `packages/accelerator/src-tauri/src/updater.rs:14-57`:

1. Load floor state before network access; corruption/I/O failure disables this update check.
2. Call plugin `check()`.
3. Convert public `Update` fields into a pure `CandidateView`.
4. Parse and verify the embedded envelope.
5. Require strict top-level/envelope and `Update` equality.
6. Require mandatory signed size `<= MAX_UPDATE_BYTES`.
7. Require candidate `> max(current_version, persisted_floor)`.
8. Only then log “Update available,” auto-install, return it for prompting, or store it.

Replace `advertised_update_size()` and its optional behavior at `updater.rs:64-122`. Size now comes only from `VerifiedUpdate`; missing size is rejection, not a warning.

Re-read the floor and repeat the monotonic comparison in `perform_update` before download. This closes a stale-prompt/racing-instance window.

Keep `Update::download()` and `Update::install()` unchanged so the plugin remains the artifact authenticity authority.

Gate:

```bash
cd packages/accelerator/src-tauri
cargo test updater -- --nocapture
cargo test update_manifest -- --nocapture
cargo clippy --all-targets -- -D warnings
```

## Phase 2 — Monotonic floor and successful-launch commit

Add `packages/accelerator/src-tauri/src/updater_state.rs`.

Storage:

```text
~/.aztec-accelerator/updater-state.json
{"schema":1,"floor":"1.0.7"}
```

Do not store it in `config.json`: malformed config currently becomes defaults, which would silently erase the floor.

Atomic persistence:

- Extract/reuse an `atomic_write_private()` helper based on `config.rs:140-176`.
- Resolve a real home directory; no `"."` fallback for security state.
- Create/force the parent directory to `0700` on Unix.
- Create a random same-directory `NamedTempFile`, which starts owner-only; write JSON, `sync_all()`, and `persist()` over the destination.
- Sync the parent directory on Unix after replacement.
- Windows uses the profile ACL plus `tempfile`’s replace-existing `MoveFileExW` implementation.
- Serialize all updates through a process mutex.
- `NotFound` means first run. Invalid JSON/schema/SemVer, permission failure, or other I/O errors are distinct corrupt/unavailable states and disable updates.
- Never rename aside, reset, or overwrite a corrupt floor automatically.

Successful launch:

- Extend the existing health probe to require `status=="ok"`, `api_version==1`, and `.version == env!("CARGO_PKG_VERSION")`.
- In release builds only, start a tracker after crash recovery is armed and HTTP startup is spawned.
- Require three consecutive successful self-probes over a short grace period before calling `commit_successful_launch(current)`.
- Commit `floor = max(old_floor, current)`; never lower it.
- Do not write the candidate during download or install. A crash before the new build becomes healthy leaves the old floor intact; crash recovery can relaunch and retry the health commit.

No pending-install marker is necessary. Every higher build that demonstrably runs—including updater and manual upgrades—may raise the floor; downloads and installs cannot. This also handles rollout from legacy builds: the first protected release bootstraps the missing floor after becoming healthy.

Crash-recovery ordering remains:

```text
old build verifies/downloads
→ Windows disarms recovery
→ install
→ old build re-arms before restart
→ new setup re-arms if enabled
→ new /health reports its version repeatedly
→ floor advances
```

No direct behavioral change is needed in `crash_recovery.rs`; it does not observe application health.

Downgrades:

- No updater candidate equal to or below the historical floor is accepted.
- A manually installed older binary may run, but a protected binary will never lower the file and will only accept a future version above the old floor.
- No production override/reset UI. Recovery from genuine state corruption is an explicit support action.

Gate:

```bash
cd packages/accelerator/src-tauri
cargo test updater_state -- --nocapture
cargo test updater -- --nocapture
cargo test

cargo test --manifest-path ../core/Cargo.toml
cargo clippy --all-targets -- -D warnings
cargo clippy --manifest-path ../core/Cargo.toml --all-targets -- -D warnings
```

Unix tests assert directory `0700`, file `0600`, missing-state bootstrap, monotonic writes, corrupt-state preservation, and failed-write preservation of the old file. Windows CI must exercise repeated replace-existing writes.

## Phase 3 — Isolated release signing and artifact handoff

Add `sign-update-feed` after `validate` and `build` in `.github/workflows/release-accelerator.yml`.

Permissions:

```yaml
permissions:
  contents: read
```

No `contents: write`, `actions: write`, `id-token`, AWS credentials, GitHub App token, `gh release`, or S3/CloudFront commands. `upload-artifact` is the intended handoff and does not grant repository write authority.

Steps:

1. Checkout the exact release ref using the target branch’s SHA-pinned action.
2. Download all four desktop build artifacts.
3. Compute final release URLs, exact artifact sizes, and read artifact `.sig` contents.
4. Use the Rust example to create the typed manifest and canonical byte file.
5. Sign the arbitrary canonical file with:

```bash
TAURI_SIGNING_PRIVATE_KEY=... \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD=... \
bunx tauri signer sign manifest.canonical.json
```

Use environment variables only; do not pass `-k`/`-p`, echo secrets, or enable shell tracing. Tauri CLI 2.10.1 supports arbitrary `<FILE>` signing and writes the corresponding `.sig`.

6. Use the Rust tool to assemble `latest.json`.
7. Verify the finished feed cryptographically against the pubkey extracted from committed `tauri.conf.json`.
8. Emit SHA-256 as a job output and upload `latest.json` as immutable Actions artifact `accelerator-update-feed`.

Generate the signed feed for RC runs too, because updater smokes need it; only stable runs publish it.

Modify `release`:

- Add `sign-update-feed` to `needs`.
- Delete inline latest generation at `release-accelerator.yml:641-709`.
- Download `accelerator-update-feed`, verify its SHA-256 job output, and copy it unchanged into `release-files/`.
- The privileged release job only uploads those signed bytes to GitHub/S3.
- Never reserialize or re-sign after handoff.
- Add the signing job to `tag.needs` so a tag cannot be created without a valid feed.

Modify `verify-live-feed`:

- Compare public feed SHA-256 with the signing-job output.
- Retain version/platform/URL reachability checks.
- Validate top-level/envelope equality with `jq`; the exact hash proves the live bytes are the already cryptographically verified artifact.

The same key is intentional. Existing artifact builds still need `TAURI_SIGNING_PRIVATE_KEY`; centralizing all artifact signing would require redesigning Tauri’s updater-artifact generation and is outside F-004.

Gate:

```bash
bun install --frozen-lockfile
bun run lint:actions
actionlint .github/workflows/release-accelerator.yml \
  .github/workflows/_e2e-updater.yml \
  .github/workflows/_e2e-updater-linux.yml \
  .github/workflows/_e2e-updater-windows.yml

shellcheck packages/accelerator/scripts/updater-smoke.sh \
  packages/accelerator/scripts/updater-smoke-linux.sh
```

Local throwaway-key rehearsal:

```bash
cd packages/accelerator
tmp="$(mktemp -d)"
bunx tauri signer generate --ci -p "" -w "$tmp/test.key"
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$tmp/test.key")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
cargo run --manifest-path src-tauri/Cargo.toml \
  --example update-manifest -- canonicalize "$tmp/manifest.json" "$tmp/canonical.json"
bunx tauri signer sign "$tmp/canonical.json"
cargo run --manifest-path src-tauri/Cargo.toml \
  --example update-manifest -- verify-feed "$tmp/latest.json" "$tmp/test.key.pub"
```

## Phase 4 — CI and updater E2E

### PR-safe Linux gate

Add an `updater-rollback-linux` job to `.github/workflows/accelerator.yml` and include it in `accelerator-status`.

Using only a generated fixture key:

1. Build release-profile synthetic N-1 and N AppImages from the changed code.
2. Patch the fixture public key into the temporary build config.
3. Generate artifact and manifest signatures with the fixture private key.
4. Exercise:

   - Valid signed N: updates, relaunches, `/health==N`, floor becomes N.
   - Feed-writer attack: alter only top-level version to `9.9.9` while serving N’s valid signed artifact; expect manifest mismatch, security log, and no artifact request.
   - Signed candidate `<= floor`: expect no download.
   - Corrupt floor: app remains healthy on N-1, updater is disabled, file remains untouched.
   - Artifact bytes tampered with valid artifact signature: artifact is downloaded but plugin rejects it.

Use separate temporary HOME/app paths per scenario.

### Release smokes

Update the existing macOS/Linux/Windows scripts to consume the exact signed `latest.json` artifact rather than synthesizing one.

The signed production URLs point to `github.com`. For local E2E:

- Generate the local CA leaf with SANs for both `aztec-accelerator.dev` and the artifact URL host.
- Map both hosts to `127.0.0.1` only after all real GitHub downloads finish.
- Rename/stage the artifact to the exact basename in the signed URL.
- The existing host-agnostic feed server can serve both paths.
- Do not create a second prod-key-signed test manifest with alternate URLs; such a reusable signed feed would be an unnecessary capability.

Assertions:

- Existing real N-1 macOS/Linux positive paths remain, proving legacy clients ignore the new envelope and update successfully.
- After N becomes healthy, assert `updater-state.json.floor == N`, no lower value, and `0600` on Unix.
- Windows synthetic N-1 must be built unsigned or with a matching fixture key; remove the current “throwaway private key with committed prod pubkey” mismatch.
- Preserve existing artifact-tamper negative legs.
- Add the F-004 top-level-high-version/old-artifact negative at least on Linux PR CI and Windows release CI.
- macOS CI remains authoritative for notarized in-place `.app` replacement; Windows CI for NSIS restart, Task Scheduler arm/disarm/re-arm, ACL/atomic replacement; Linux CI for AppImage/FUSE in-place replacement.

## Local versus CI coverage

Locally on a GUI-less Linux VPS:

- Canonical byte format and signature fixtures.
- Every manifest mutation and exact-match rejection.
- SemVer/prerelease ordering.
- Floor load/bootstrap/corruption/monotonicity.
- Unix atomicity and modes.
- Pure candidate gate and signed-size cap.
- Rust formatting, tests, Clippy, actionlint, shellcheck.
- The Rust manifest CLI’s generate/sign/verify round trip with a throwaway key.

CI-only or authoritative:

- Real Tauri `Update.raw_json` behavior against an HTTPS feed.
- Actual plugin artifact download verification.
- AppImage/FUSE replacement and restart.
- macOS code signing, notarization, `amfid`, and both architectures.
- Windows NSIS mutation, WebView/tray launch, Task Scheduler sequencing, and atomic replace-existing behavior.
- Production-key signing job, artifact handoff, GitHub/S3 publication, and CDN byte equality.

## Security & Adversarial Analysis

- High unsigned version + old valid artifact: top-level fields cannot match the old signed envelope; rejected before download.
- Replayed old fully signed feed: rejected by `candidate <= max(current,floor)`.
- Modified URL/signature/size/date/version: either envelope signature or exact-match check fails.
- Signature replacement: minisign verification fails.
- Malformed/unknown/ambiguous JSON: strict typed parsing fails closed.
- Stale manual prompt: floor is re-read immediately before download.
- Crash after install but before health: floor remains old; recovery relaunch may later commit.
- Floor truncation/corruption: updater disabled; file is neither reset nor overwritten.
- Floor deletion: treated as first-run bootstrap after healthy launch. Protection is against feed/publisher compromise, not a same-user local attacker who can replace application state or binaries.
- `pub_date` is audit metadata, not freshness authority; anti-replay comes from SemVer and the floor.
- Manifest signing supplements, never replaces, artifact-byte minisign verification.
- Signed mandatory size closes SEC-03’s feed-writer lie. It still does not cap bytes actually streamed if the signed artifact host serves a huge/chunked response; plugin v2.10.1 buffers before verification and exposes no aborting callback. Keep #345/residual documented.
- Same-key domain separation comes from the fixed signed `schema` and typed JSON structure. Separate-key use does not create two-party security because old artifact signatures are public.
- Key rotation remains constrained by Tauri’s single pinned artifact key. Rotate with an old-key-signed bridge release that embeds the new key, retain the bridge feed long enough for adoption, then sign later releases with the new key. Do not switch both keys in one release or silently accept multiple arbitrary keys.

## Assumptions

### Facts

- Production auto-updates are one-way; no legitimate updater downgrade exists.
- Stable feeds currently contain four platform entries and artifact URLs on GitHub.
- The current signing secret already produces artifact signatures matching the pinned public key.
- Existing release smokes prove artifact authenticity and restart behavior but do not exercise F-004 because real N-1 binaries predate manifest verification.

### Inferences

- Three consecutive version-specific `/health` responses are the closest in-repo definition of “successful launch” and match current release gates.
- A higher manually installed healthy build should raise the floor; distinguishing updater versus manual upgrade adds state without strengthening rollback resistance.
- Strict unknown-field rejection is acceptable for a security envelope; future feed-schema extensions require an explicit schema revision.

### Asks

- Non-blocking operational confirmation: the `TAURI_SIGNING_PRIVATE_KEY` and password secrets are available to the new read-only signing job, not restricted to the build matrix environment.
- Assign an owner/runbook for corrupt-floor recovery and staged updater-key rotation before the first protected stable release.