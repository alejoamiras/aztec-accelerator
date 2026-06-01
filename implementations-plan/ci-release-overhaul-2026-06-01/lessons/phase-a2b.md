# Phase A2b — Linux AppImage updater-smoke (advisory)

PR #250. Branch `ci/phase-a2b-updater-smoke-linux`. Validated via 1.0.3-rc dry-runs.

## What landed
- `packages/accelerator/scripts/updater-smoke-linux.sh` — Linux sibling of the macOS `updater-smoke.sh`.
- `.github/workflows/_e2e-updater-linux.yml` — reusable workflow (FUSE + Xvfb + dbus + stalonetray).
- `release-accelerator.yml` — `update-smoke-linux` advisory job (absent from `tag.needs`).

## Resolved unknowns (researched locally, before any CI cycle)

### 1. TLS trust — the determinative blocker, GREEN
The whole local-CA impersonation hinges on the Linux updater reading the **system trust store**.
- `reqwest = "0.12"` with no explicit TLS feature → default-tls (native-tls → OpenSSL → `/etc/ssl/certs`).
- Cargo.lock has **no `webpki-roots`** in the tree, but **does** have `rustls-native-certs`. So even if tauri-plugin-updater pulls the rustls path, roots come from the OS store, **not** bundled Mozilla roots.
- ⇒ Either path reads the system store. `update-ca-certificates` makes the local CA trusted. Conclusion: the macOS keychain-trust approach ports directly. (high confidence)

### 2. Config dir — IDENTICAL to macOS
`config.rs:75` → `dirs::home_dir().join(".aztec-accelerator").join("config.json")` on **both** OSes (not XDG). So preseeding `auto_update:true` at `$HOME/.aztec-accelerator/config.json` works unchanged.

### 3. Updater trigger
`updater.rs::check_for_update()` runs in a background loop (main.rs); `auto_update==true` ⇒ `perform_update()` auto-applies. The N-1 AppImage is a **release** build (updater compiled IN — unlike `--features webdriver` builds, which compile the check OUT), so the check is live.

### 4. Single minisign pubkey, platform-agnostic
`tauri.conf.json` has one `pubkey` for all platforms ⇒ serving the already-signed N `.AppImage` needs **no signing key** (same secretless property as macOS).

## Implementation gotchas

### actionlint: `continue-on-error` is forbidden on a `uses:` caller job
```
when a reusable workflow is called with "uses", "continue-on-error" is not available.
```
So "advisory" can't be expressed on the caller. Fix: push the semantics **inside** the reusable workflow — `continue-on-error: true` on the smoke **step** + a follow-up "Advisory result" step that downgrades a failure to `::warning::` + `$GITHUB_STEP_SUMMARY`. Net: the job stays green (production releases never red on a Linux-updater fault), the signal is surfaced (warning annotation + summary), and the smoke **step** log carries the real pass/fail. To make it blocking later: delete `continue-on-error` + add the job to `tag.needs`.

### FUSE vs extract-and-run — must be native
AppImage self-update replaces the file at `$APPIMAGE` in place. `--appimage-extract-and-run` does **not** set `$APPIMAGE` the same way ⇒ would test the wrong path. So install `libfuse2` (`libfuse2t64` on 24.04 / ubuntu-latest, with a `|| libfuse2` fallback) and run **natively** — the AppImage runtime then sets `$APPIMAGE` itself (don't override it manually; a forced value risks a path mismatch vs the runtime's canonicalization).

### Don't touch the blocking macOS path
macOS `update-smoke` is in `tag.needs` and proven green (rc.8). Linux is a **separate** script + reusable workflow + job, not a parameterization of `_e2e-updater.yml`, specifically so an advisory experiment can't regress the blocking gate. Refactor to share only after Linux is itself proven.

## Hardening (plan security items, folded into the merge candidate)
- **In-place-swap proof**: assert the on-disk AppImage sha256 **changed** from N-1 (and ideally equals the served N). A version flip with an unchanged file = a non-in-place mechanism worth knowing about.
- **Run-unique CA** (`updater-smoke-local-CA-$GITHUB_RUN_ID-$ATTEMPT`): cleanup removes only this run's anchor — a fixed name could clobber a concurrent/leftover entry on a non-ephemeral self-hosted runner. (The macOS script still uses a fixed name — retro-harden as a follow-up.)

## Open question this leg EXISTS to answer
Does Tauri's `v1Compatible` Linux updater apply a **raw `.AppImage`** (what the shipped `latest.json` points at) in place? The artifact-format spike confirmed the shipped feed is self-consistent (raw `.AppImage` + `.AppImage.sig`), but whether the updater *applies* it is empirical. The advisory leg surfaces it on a real runner; if red, the script log distinguishes a **FUSE/harness** failure from a genuine **updater rejection**. A red here may mean Linux auto-update is broken in production — which would reframe this from a gate into a bug-fix + gate.
