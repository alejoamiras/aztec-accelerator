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
