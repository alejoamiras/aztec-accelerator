# Phase 1 lessons — toolchain + app brand assets

- devDeps installed clean (all past the 7-day min-age gate): `@resvg/resvg-wasm@2.6.2`,
  `@fontsource/figtree@5.3.0`, `@fontsource/fragment-mono@5.3.0`,
  `@fontsource/bricolage-grotesque@5.3.0`.
- **og renderer pivot**: fontsource ships woff/woff2 only; resvg's fontdb needs ttf/otf, so the
  planned resvg og-card rendering can't set text without vendoring TTFs from a second source.
  Pivot: `--target og-*` renders an HTML template via Playwright chromium (already a project dev
  tool with a warmed browser cache) using the SAME vendored woff2 the app ships — better brand
  fidelity, no new deps, no second font source. Consequence: og PNG bytes are not
  cross-machine-reproducible (antialiasing varies); the sha256 manifest asserts committed-file
  integrity, and og regeneration re-records the manifest. Tray/app icons remain resvg
  (text-free, deterministic).
- Tray SVG masters are emitted by the generator itself from one geometry constant (bolt path +
  orbit radius), then rasterized — a single source of truth instead of 25 hand-kept files.
- PNG IDAT is a zlib stream: `Bun.inflateSync` (raw deflate) fails with "invalid stored block
  lengths"; use `node:zlib` `inflateSync`.
- `tauri icon` emits `icon.png` at 512px; the repo convention (and Linux `{{icon}}` source) is
  1024px — the generator overwrites it with the 1024 master render.
- This host's non-interactive shell has no cargo on PATH: prefix `PATH="$HOME/.cargo/bin:$PATH"`
  for every cargo-touching command (root `bun run test` includes `lint:rust`).
- Fresh worktree needs the bb sidecar before any cargo build: `bun scripts/copy-bb.ts` (the
  `prebuild` script) stages `src-tauri/binaries/bb-<triple>` from node_modules.
- Never trust `cargo test | tail` exit codes (zsh, no pipefail): run unpiped with output
  redirected to a file, echo `$?`.
- Gate: frontend:build ✓ · root `bun run test` ✓ · accelerator test:unit 64 pass (incl. 6
  icon-asset checks) ✓ · `cargo test` exit 0, 131 passed ✓ · commit `ccf1415`.
