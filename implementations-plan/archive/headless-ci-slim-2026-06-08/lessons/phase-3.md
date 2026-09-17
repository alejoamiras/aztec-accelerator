# Phase 3 — slim the bb-less legs + hooks

- **Smoke:** composite `install-tauri-system-deps: "false"` + `run-prebuild: "false"`. Replaced the
  `cat AZTEC_VERSION` hook (its file is gone post-slim) with a version-only resolve:
  `AZTEC_BB_VERSION="$(bun "$GITHUB_WORKSPACE/packages/accelerator/scripts/bb-version.ts")"` — invoked by
  **ABSOLUTE path + direct script** (NOT `bun run prebuild:version`, which echoes a `$ …` line that
  command-substitution would capture — verified the direct call yields `[4.2.0]` clean). Added a `.version`
  shape-guard. Did NOT assert `bb_available` (legitimately `false` now — no bb copied; codex finding).
- **Release-Smoke:** composite both inputs `"false"` (build + package only; no /health, no bb).
- **e2e (`_e2e.yml`):** set `AZTEC_BB_VERSION` from `packages/accelerator/src-tauri/AZTEC_VERSION` via
  **`$GITHUB_ENV`** (persists to the later launch step — codex: a plain `export` in an earlier step wouldn't).
  This is the `/prove` fast-path fix (codex finding #1); the e2e SETUP is untouched.
- Validation: `bun run lint:actions` clean; the direct-script capture is `[4.2.0]`.
