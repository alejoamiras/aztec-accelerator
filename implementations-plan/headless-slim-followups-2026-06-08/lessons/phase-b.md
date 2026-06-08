# Part B — slim release build-headless + dead-patch/comment cleanup

Base: `feat/release-build-headless-slim` off `1acc951` → PR #331 (code; rc owner-gated).

## Done (4 edits to release-accelerator.yml `build-headless`)
- **(a)** Added `install-tauri-system-deps: "false"` + `run-prebuild: "false"` to the composite `with:` → drops
  WebKit/GTK + the bb-copy prebuild + (via the Phase-3b gating) the host-assert.
- **(b)** Deleted the dead `sed` on `src-tauri/Cargo.toml` + its `.bak` rm — build-headless compiles server→core,
  NOT src-tauri (verified: `server/Cargo.lock` has no `aztec-accelerator` stanza; the DESKTOP `build` job patches
  src-tauri separately on its own runner — release:114-123).
- **(c)** Fixed the version-patch comment → only the SERVER crate is patched; it's the single source for
  `--version` + `/health.version` (injected `app_version`).
- **(d)** Fixed the `--locked` comment → `--locked` stays off because patching `server/Cargo.toml` stales the
  server lock's own `accelerator-server` stanza (NOT the gone src-tauri patch) — else a future reader re-adds
  `--locked` + breaks the release build.

## Validation
- `bun run lint:actions` clean.
- **rc-gated (owner-dispatched) — SURFACED, not dispatched.** RC-green proves the slimmed build-headless
  compiled + version-asserted + packaged + got into the prerelease — NOT extracted runtime/`/health` (the
  updater-smokes consume DESKTOP artifacts, not the headless tarballs; final codex). It's a REAL prerelease
  publish (public rc tag + GH prerelease; prod feed untouched) — fresh rc.N each run. Release = owner-only.
