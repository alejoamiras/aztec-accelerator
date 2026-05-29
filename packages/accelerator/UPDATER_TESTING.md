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

## Future automation (own plan)

An automated updater-path E2E would need to run **post-build** (after the
signed bundle exists — the current release E2E gate runs before signing) and
either (a) as a privileged release smoke using the real artifacts + feed, or
(b) with a test-only, Rust-only updater endpoint/pubkey override signed by a
throwaway key (never exposed to the frontend or HTTP surface). Tracked as a
separate effort; not blocking releases today given the bundle invariant above.
