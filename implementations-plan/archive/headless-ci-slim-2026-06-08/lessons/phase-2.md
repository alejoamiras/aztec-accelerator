# Phase 2 — composite boolean inputs

Added `install-tauri-system-deps` + `run-prebuild` to `setup-accelerator/action.yml` (both `default: "true"` →
zero change for every existing caller). Gates use **explicit STRING comparison** (final-codex condition — GHA
inputs are strings, so bare truthiness is always true):
- host/target assert: `if: inputs.rust-target != '' && inputs.run-prebuild != 'false'` (the assert guards
  host-selected sidecar copying = the prebuild; irrelevant to pure headless builds).
- system deps: split into "desktop" (`install-tauri-system-deps != 'false'` → full WebKit/GTK list) +
  "headless-only" (`== 'false'` → `libssl-dev` only).
- prebuild ("Copy bb sidecar"): `if: inputs.run-prebuild != 'false'`.
Validation: `bun run lint:actions` clean; defaults `"true"` → behavior-preserving.
