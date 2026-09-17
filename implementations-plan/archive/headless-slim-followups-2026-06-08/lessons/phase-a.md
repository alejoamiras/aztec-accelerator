# Part A — drop GUI libs from _e2e.yml

Base: `feat/e2e-drop-gui-libs` off `1acc951` (#329 merged).

## Done
- `_e2e.yml` "Install system dependencies (accelerator)" (:45-49): dropped `libwebkit2gtk-4.1-dev` /
  `libappindicator3-dev` / `librsvg2-dev` / `patchelf` / `libgtk-3-dev` → `sudo apt-get install -y libssl-dev`
  only. The leg builds ONLY `accelerator-server` (`cargo build` in packages/accelerator/server) — no Tauri GUI →
  no GTK consumer (both auditors + final codex confirmed). Kept `libssl-dev` (reqwest / bb download), the prebuild
  (bb for the SDK e2e via `BB_BINARY_PATH`), and the `AZTEC_BB_VERSION` hook (Phase 3b).
- Validation: `bun run lint:actions` clean. Self-proves on the PR gate (the accelerator E2E runs with
  `build_accelerator: true` → builds + runs the headless server + proves).
