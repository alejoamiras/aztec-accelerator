# Phase 3 lessons — playground reskin + spark-orbit theater

- `PhaseQueue`/`AnimationPhase` extracted verbatim to `phase-queue.ts`; `ascii-animation.ts` + its
  test deleted (git preserves). `SparkOrbitController` keeps the exact public shape, so `main.ts`
  changed only imports/constructor names + strings.
- Dial mechanics: quadrant-owned buckets, asymptotic within-quadrant easing (τ=1.4s, cap 0.96) so
  idle stretches hold honestly; `denied`/`version-mismatch`/`fallback` switch to a dimmed
  "proving in your browser" mode and progress continues (matches the verified SDK flow); a new
  run laps forward, never rewinds. All unit-tested with an injectable reducedMotion flag.
- Tailwind v4 theming: `@theme` emits `var()`-referencing utilities, so re-assigning the same
  custom properties in a `prefers-color-scheme` block + `[data-theme]` block themes every utility.
  Tailwind's `.hidden` loses to a later `.dial-host{display:flex}` — re-assert
  `.dial-host.hidden{display:none}` (same pattern the app uses for `[hidden]`).
- Stock palette leaks (gray-700/red-400/amber-500/black-40) replaced by brand tokens incl. new
  `--color-brand-{gold,warning,danger,go,go-text}`; codex's contrast catch was real — `go`
  (#189e62) is 3.3:1 on paper, so `.log-success` wears `go-text` (#147a4c).
- `contrast-guard.test.ts` landed (accelerator/scripts, runs in test:unit): parses the REAL
  custom-property declarations from both shipped sheets, computes WCAG ratios for every text-role
  pair in both themes, threshold 4.5. Extends to landing in Phase 4.
- Playwright chromium (warm cache) rendered the og card from the vendored woff2 — no TTF needed.
- Biome gotchas: import sorting + forEach-returning-value + formatter on new files; `bunx biome
  check --write` then rerun.
- Gate: root `bun run test` ✓ (76-pass final leg; lint+typecheck+unit incl. dial suite 10, contrast
  guard 2, icon assets 6; tauri-identity untouched) · playground `test:e2e` 8 pass ✓ · playground
  `build` ✓ with `dist/og-image.png` present ✓.
