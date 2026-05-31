# Manual updater test — run before promoting an rc to stable

The auto-updater's in-place swap is the one path our automated tests cannot
fully exercise (the updater verifies an Ed25519 signature against the embedded
public key, so a CI test would need either the production signing key or a
throwaway-key build that isn't the artifact we ship). Until that is automated
(see below), run this **manual check before promoting any `X.Y.Z-rc.N` to a
stable `X.Y.Z`** — auto-update from an older stable is exactly what broke in
1.0.1.

## What automated CI already covers

- **Bundle structure invariant** (`release-accelerator.yml`, macOS `build` job):
  asserts the `.app/Contents/MacOS/` directory contains exactly
  `{aztec-accelerator, bb}`. This deterministically catches the *specific*
  1.0.1 regression (a stray `accelerator-server` binary changing the signed
  bundle's shape and breaking amfid revalidation on update).
- **WebDriver E2E** confirms a freshly-built app launches and is drivable.

What it does NOT cover: the actual N-1 → N in-place swap + relaunch on a real
macOS install.

## Manual runbook (macOS, ~5 min)

1. Pick the current latest stable on GitHub Releases — call it **N-1**.
2. On a Mac, fully remove any existing install:
   - Quit the app (tray → Quit), then `rm -rf "/Applications/Aztec Accelerator.app"`, empty Trash.
3. Download the **N-1** DMG, install it to `/Applications`, launch it. Confirm the tray icon appears and `curl -s http://127.0.0.1:59833/health` returns `"status":"ok"`.
4. Publish (or stage) release **N** so the updater feed (`latest.json`) advertises it. (For a dry run, you can point the build at a prerelease and temporarily edit your local feed — but the simplest real test is right after cutting N.)
5. In the running N-1 app, trigger the update (tray prompt → **Update Now**, or wait for the 5-min check).
6. **Confirm:** the app downloads, swaps, and **relaunches without hanging**. The tray icon returns; `curl -s http://127.0.0.1:59833/health` still answers; the version now reflects **N** (check the tray "vX.Y.Z" line, or `accelerator-server --version` for the headless build).
7. If the app hangs at launch after the swap (no tray, process stuck at 0% CPU): **do not promote.** That is the 1.0.1 failure mode — investigate the bundle shape / signature before shipping.

## Automated gate (`update-smoke`)

This is now partly automated. The `update-smoke` job in `release-accelerator.yml`
(reusable `_e2e-updater.yml` + `scripts/updater-smoke.sh` + `updater-feed-server.ts`)
runs the macOS install→update→relaunch check during every release, post-build.

**It needs no signing key.** It serves the **already prod-signed** N artifact
from a local HTTPS server impersonating `aztec-accelerator.dev` (an `/etc/hosts`
entry + a per-run local CA trusted on the runner), and N-1 verifies it against
its **embedded prod pubkey**. No private key, no throwaway key — the job is
`contents: read` only.

**What it proves — and what it doesn't.** N-1 (latest stable) and N (the build
under test) are *both* post-fix, so a green **positive** leg proves the live
signed-updater path works end-to-end (endpoint → TLS → download → signature
verify → in-place swap → bare-spawn relaunch → `/health == N`). It does **not**
by itself prove the gate would *catch* a 1.0.1-class regression — the
deterministic guard for that exact class is the **bundle-shape invariant** in
the `build` job. To give the gate some teeth, a **negative** leg serves the
**genuine signature with a tampered tarball** (one appended byte) and asserts
the update is **rejected** — proven by an actual `download/` hit in the feed log
(the app fetched the artifact) followed by `/health` *never* reporting N (the
cryptographic check over the tampered bytes failed). This exercises real
signature verification, not a malformed-blob parse error. Full detection of a
*validly-signed but bad* bundle would require an ephemeral CI signing key + a
CI-keyed N-1 (deferred).

**Robustness baked in:** the macOS build emits updater artifacts via
`tauri build --bundles app` (no `hdiutil`) and bundles the DMG in a separate
retried step, so the chronic GH-runner DMG flake can't suppress updater signal;
`update-smoke` runs per-arch independent of a sibling arch's build failure; and
the N-1 selection aborts if it resolves to the same version under test (no
no-op pass).

**Status:** macOS (Apple Silicon + Intel) positive + an arm64 negative leg,
**advisory** until validated on a `1.0.3-rc` dry-run, then promoted to
release-blocking. A follow-up adds the Linux AppImage leg. Until macOS is
blocking, run the manual steps above before promoting an rc to stable.

A `-rc` dry-run is safe for real users: prereleases skip the S3 `latest.json`
upload and are marked `--prerelease --latest=false`, so the prod updater feed
keeps pointing at the current stable — auto-update users never see the rc. The
gate's feed is also local-only.
