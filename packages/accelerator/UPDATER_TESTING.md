# Updater testing

The release pipeline exercises the real N-1 → N updater path before publishing an accelerator release. All release-time updater smokes consume artifacts and feed fragments signed by the isolated `sign-updater-artifacts` job; no smoke job can read the production updater private key.

## Signing boundary

Desktop build jobs generate a throwaway updater key because Tauri requires one while producing updater bundles. Those signatures are excluded from the uploaded build artifacts.

After all four desktop builds finish, the `release-signing` environment exposes the production key to one short job. Before the key is available, that job checks out the intended commit, installs pinned tooling, builds the locked Rust verifier, and downloads the exact build outputs. The current key-scoped step performs only:

- sign the four updater payloads;
- verify every payload signature against the public key embedded in `tauri.conf.json`;
- assemble and verify `latest.json`;
- assemble and verify one-platform `smoke-latest.json` feeds.

It never builds, installs, launches, or smoke-tests an application. Downstream jobs receive only signed artifacts and feeds.

This is secret scoping, not an independent security sandbox: repository code in the signing job is still trusted. In the solo-maintainer setup, protect `main`, restrict the environment to `main`, and inspect signing-workflow changes before release. Add an independent environment reviewer or external signing service only if protection against a compromised maintainer or malicious code already merged to `main` becomes part of the threat model.

The throwaway build keys therefore test whether a new binary can be produced. The production key is tested by signature verification and by the N-1 clients accepting the signed N payloads in the updater smokes.

## Release-blocking automated coverage

`release-accelerator.yml` blocks draft creation unless all of these pass:

- **macOS Apple Silicon and Intel, positive:** install the current stable N-1 DMG, serve the production-signed N payload from a local TLS endpoint impersonating the configured production host, then require the app to update, relaunch, and report N from `/health`.
- **macOS Apple Silicon, negative:** append a byte to the genuine payload while retaining its signature and require N-1 to reject it.
- **Linux x86_64, positive:** run the N-1 AppImage natively under FUSE/Xvfb, update the file in place, require its checksum to change, then require the relaunched app to report N.
- **Windows x86_64, positive and negative:** install the pinned real N-1 NSIS fixture, require a production-signed N update to apply, and separately require a tampered payload to be rejected.
- **Bundle/notarization checks:** enforce the macOS bundle shape and verify both DMGs' code signatures and stapled notarization tickets.

The local feed is never public and never writes the production KV feed. A prerelease publish is also safe for installed users: it is a GitHub prerelease without `latest.json`, and publishing never flips the live feed.

## What remains manual

For a release that changes Windows trust, certificate installation, onboarding, or the HTTPS listener, complete the real-Windows composed-proof procedure in the [accelerator README](README.md#windows-composed-proof--manual-pre-ga-check). GitHub-hosted Windows runners cannot approve the interactive root-CA consent dialog.

For a high-risk macOS updater change, a manual installed-app check is still useful as a final sanity check:

1. Install the current stable N-1 DMG in `/Applications` and confirm `/health` reports N-1.
2. Publish N without promoting it.
3. Serve or temporarily select N through a controlled local updater test setup; do not hand-edit the production feed.
4. Trigger the update and confirm the app relaunches, the tray returns, and `/health` reports N.
5. If it hangs or fails to relaunch, do not promote the release.

The automated gates are authoritative for ordinary releases; this manual check is not a substitute for a failed CI gate.
