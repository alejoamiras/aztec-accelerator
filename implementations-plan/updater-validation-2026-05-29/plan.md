# Updater Validation — release-time "can N-1 actually update to this binary?" gate

**Status**: v2.1 — final codex pass: "technically sound; ship with macOS blocking day one, Linux advisory until proven." Rollout policy + negative-test refinement folded in below. Pending approval.

## Rollout policy (codex final must-fix)

- **macOS (arm64 + x86_64): release-blocking from day one** — it's the validated, lower-risk path and where the 1.0.1 bug lived. Both mac legs are in `tag.needs`.
- **Linux (AppImage): advisory first** (`continue-on-error: true`, NOT in `tag.needs`) until at least one clean `1.0.x-rc` run proves native AppImage execution + self-update on `ubuntu-latest`. Then promote to blocking in a follow-up. Rationale: native AppImage self-update is the highest flake risk; don't let it block real releases unproven.
**Date**: 2026-05-29
**Type**: Tier A (new test pipeline gating the signed release path)
**Decisions (user)**: release-time gate on the **real signed artifact**; **macOS + Linux (AppImage)**; own plan.

## Goal

Before a release is tagged/published, prove a user on the **previous stable** can auto-update to the **just-built, just-signed** build and the result **launches** (`/health` answers, reports the new version). This is the 1.0.1 failure class (macOS amfid hang after the in-place bundle swap) that fresh-install smoke does not catch.

**Success criteria**: a `update-smoke` matrix job in `release-accelerator.yml` that installs N-1, updates it to N via the prod-signed feed, and asserts relaunch + `/health.version == N`. `tag` `needs:` it → a failure aborts the release before any tag/asset/`latest.json`. Includes a **standing negative test** (a tampered/bad bundle must FAIL the gate).

## Architecture (research-confirmed by both audits)

- Updater `endpoints` (`https://aztec-accelerator.dev/releases/latest.json`) + minisign `pubkey` are baked into `tauri.conf.json`. `updater.rs` uses `app.updater()`, verifies `.sig` against the embedded pubkey. `updater:default` + `process:default` capabilities are granted, so `app.updater()`/`app.restart()` work in the shipped binary.
- The `build` job already produces prod-**signed** `.app.tar.gz`/`.AppImage` + `.sig`.
- **No signing key needed** (both audits, high confidence): the smoke serves already-signed N artifacts, validated against N-1's embedded prod pubkey (N-1 and N share it — verified on `main` and tag `accelerator-v1.0.2`). No private key, no throwaway key. The job is `contents: read` only.

## Redirection: Option A (locked)

Make N-1 fetch the feed/artifacts from a local server impersonating the prod endpoint — **no app code change, tests the exact shipped binary, works on the first release after merge.**

Both audits confirm A is feasible and beats B. Decisive evidence (opus): `tauri-plugin-updater 2.10.1 → reqwest 0.13.2 → rustls-platform-verifier 0.6.2`, and **no `webpki-roots` in the lockfile** → the updater's TLS verifies against the **OS trust store**, so a runner-installed CA is honored.

**Locked recipe** (codex's precision — a local CA + SAN leaf, not a vague self-signed cert; mirrors the app's own Safari-support cert design):
1. Generate a local **CA**; install it into trust **before launching N-1**:
   - macOS: `sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain ca.pem`
   - Linux: copy to `/usr/local/share/ca-certificates/`, `sudo update-ca-certificates`
2. Serve a **leaf cert with SAN `DNS:aztec-accelerator.dev`** (rustls-platform-verifier enforces SAN) from a local HTTPS server on **:443** (sudo; hosted runners grant passwordless sudo). Endpoint is hardcoded HTTPS:443 → 443 is mandatory, non-443/non-HTTPS needs a code change (not doing).
3. `/etc/hosts`: `127.0.0.1 aztec-accelerator.dev`.
4. Serve a synthesized `latest.json` for N pointing at the local N artifacts (+ their real `.sig`).

Option B (shipped Rust env endpoint override) rejected: ships test-path code into prod and can't test the first post-merge release against a real N-1.

## Driving the update headlessly

Pre-write `~/.aztec-accelerator/config.json` with `auto_update: true` before launching N-1 → `check_for_update` (Some(true)) → `perform_update` → `app.restart()`, no UI. The release build's poller fires ~5s after launch (then every 12h). **Assert by polling `/health` until `version == N`** within a tolerated restart gap — **never assert on PID change** (both audits).

## Job design (`release-accelerator.yml`)

`update-smoke` matrix:
- **macos-latest** (Apple Silicon / arm64)
- **macos-15-intel** (x86_64) — codex must-fix: we ship BOTH mac arches; arm64-only is a blind spot.
- **ubuntu-latest** (Linux AppImage, under `xvfb + stalonetray + dbus-x11` — reuse `_e2e-webdriver.yml`'s working tray setup).

Steps:
1. `needs: [validate, build]` (signed bundles exist; after the WS5 bundle invariant).
2. `gh release download` the latest **stable** N-1 asset for the platform.
3. `actions/download-artifact` the N updater artifacts from this run.
4. Synthesize a local `latest.json` for N (always — regardless of stable/prerelease).
5. Stand up CA-trust + hosts + HTTPS:443 local feed (per Option A recipe).
6. **macOS install mechanics (opus must-fix — the difference between a real gate and a false-green):** do NOT clone the existing `smoke` job, which launches from the **read-only DMG mount** — an in-place swap can't happen there, so it wouldn't reproduce 1.0.1. Instead: `hdiutil attach` the N-1 DMG → `ditto`/`cp -R` the `.app` into **`/Applications`** (writable; DMG-dragged install carries the realistic quarantine xattr + Gatekeeper/amfid path) → launch from `/Applications` so the updater overwrites the real bundle and amfid re-validates the swapped binary.
7. Pre-seed `auto_update: true` config; launch N-1; poll `/health` until `version == N` (bounded wait).
8. **Clean process state per run** (opus must-fix): `pkill -f aztec-accelerator` before/after; N-1 and N both bind `127.0.0.1:59833`, and a lingering process yields a broken-but-up server.
9. Gate: add `update-smoke` to `tag.needs`. Run for prereleases too (the `-rc` dry-run is the safest place to catch an updater regression before the stable cut).

## Standing negative test (opus must-fix)

A permanent assertion that the gate fails closed. **Prefer corrupting the `.sig` / serving a mismatched signature** (codex: deterministic, exercises the real verify path with minimal harness noise) over mutating bundle structure in the gate. Assert the update is **rejected** (N-1 stays at its own version; the signature check fails). Without this, the gate can silently degrade to always-green (a 404 feed read as "no update", a skipped verify). **Success must be `/health.version == N`, never merely `status: ok`** — N-1 also answers `ok` with its own version. Also rewrite `~/.aztec-accelerator/config.json` fresh per run (codex) so a stale `auto_update`/approved-origins state can't false-green the gate.

## Platform specifics / risks

- **macOS** is the crux (amfid in-place-swap). Install to `/Applications` per step 6.
- **Linux AppImage** (codex nit — the shakiest leg): the AppImage rewrites itself in place; must run from a **writable** path, executable, under xvfb. **Validate native AppImage execution on `ubuntu-latest` early** — if it needs `--appimage-extract-and-run`, that changes the very update path we claim to test; flag and decide before treating Linux as release-blocking. Acceptable to land macOS-blocking first and Linux advisory if native execution proves fragile.

## Security & Adversarial Considerations

- **No signing key in the job** — serves pre-signed artifacts, validates against the embedded prod pubkey. Strong least-privilege; `contents: read` only, no `id-token`/`contents: write`.
- **Local CA is per-run + ephemeral**, scoped to `aztec-accelerator.dev`, never persisted/exported. No prod trust impact. Generated fresh each run.
- **Never disable signature verification** — the point is to exercise the real verify path. The negative test proves verification actually rejects bad input.
- **Fail closed**: misconfigured feed/cert/download must ERROR, not silently pass. Assert positively on `version == N`.
- The smoke must not weaken the release path's permissions; it inherits minimal scope.

## Test strategy

The job is the test. Validate by: (a) green on a `1.0.3-rc.N` dry-run; (b) the standing negative leg is RED when fed a bad bundle (proving it would've caught 1.0.1). Keep WS5's static bundle invariant as the cheap first line; this is the dynamic end-to-end proof.

## Docs

Update `packages/accelerator/UPDATER_TESTING.md` (currently stale per codex — it implies the automated test needs the signing key; it does **not**). Keep the manual runbook as the human fallback, but note the automated gate now covers the common case.

## Sequencing

Own PR (`ci/updater-validation`), after the ci-speed PR (independent, but lands second to keep release-path changes isolated). Validate on a `1.0.3-rc` dry-run before relying on it to gate a stable cut.

## Open questions (final pass)

1. macOS DMG `ditto` → `/Applications` + Gatekeeper on a `gh`-downloaded notarized N-1 — any first-launch quarantine prompt that blocks headless launch? (N-1 is notarized+stapled, so Gatekeeper should pass silently; confirm.)
2. Linux: native AppImage execution on `ubuntu-latest` without extract-and-run — verify early (the make-or-break for the Linux leg).
3. Time budget on both mac arches + Linux within `timeout-minutes` (~15–20).
4. Negative test shape: separate matrix leg vs inline step — which is cleaner to keep permanently green-when-good / red-when-bad.
