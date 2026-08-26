# Phase 4 lessons — landing rebuild

- Full Presto landing per the Brand Book: trimmed banner, presto✦ wordmark nav, hero with retimed
  CSS stagger + floating cloud/bolt visual, the live race card (race.ts: IntersectionObserver
  start, rAF loop, reduced-motion renders final state), dev callout, FOUR feature tiles
  (Encrypted · HTTPS on by default added), how-it-works, the "Why presto?" card, TEE-Rex kept but
  `hidden` with a re-enable comment, footer disclaiming Aztec Labs AND the Aztec Foundation,
  bridging seam #1 in the hero note ("installs as 'Aztec Accelerator' until our full rename").
- Mobile nav built new (recon: zero prior art): toggle button with aria-expanded/aria-controls,
  drawer, closes on link click and Escape (focus returns to the toggle).
- The cursor-follow ambient glow was removed (period marker; JS-driven so reduced-motion never
  covered it). Scroll reveals, OS-detect download, feed-tag resolution, and the health-probe
  contract (.hero-sub.detected / .accel-dot) all preserved; the detected line now builds its DOM
  with createElement (no innerHTML) and says "Presto is running on this machine."
- Landing got its FIRST typecheck: minimal tsconfig (types: vite/client + bun for feed.test.ts)
  wired into `test`. Gate caught a real gap the package always had.
- Contrast guard extended to the landing sheet (three sources, both themes) — all pairs clear 4.5.
- Gate: root `bun run test` ✓ · `bun run --cwd packages/landing test` ✓ (typecheck + 4 tests) ·
  landing build ✓ with `dist/og-image.png` ✓.
