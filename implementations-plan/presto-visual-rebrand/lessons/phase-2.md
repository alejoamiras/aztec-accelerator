# Phase 2 lessons — desktop app reskin

- Token sheet landed in `frontend/style.css` only (dark twin via media query + `[data-theme]`
  override hooks; `color-scheme: light dark`; per-theme static fallbacks before every `color-mix`).
  `onboarding.css`/`renewal.css` still declare zero tokens.
- Per-OS certificate copy in `onboarding.js` (`HTTPS_WARN`) left byte-identical: the facts were
  already accurate and plain; only the surrounding row copy went Presto.
- Naming rule applied: window titles / tray tooltip / HTML titles / popup headings / verified-sites
  displayName renamed; `settings.js:139,144` + autostart/commands OS-locating strings KEPT
  (they must match the OS-visible product name). Bridging sentences (owner option b) landed in
  settings' Manage-certificate sub and the update prompt; the landing seam comes in Phase 4.
- Status voice: `ServerStatus::display_text` → "Ready" / "Fetching the prover…" /
  "Working a proof…"; same-commit companions all updated (core test seq, both doc comments,
  `main.rs:728` initial menu literal). Tray error strings (`main.rs:366,820`) kept exact-fact.
- Wizard primary label pair is now "Let's go"/"Continue" — `syncPrimaryLabel` in onboarding.js
  overwrites the HTML label, so the JS pair is the real source; 3 Playwright assertions moved in
  the same commit. Em-dash error hints reworded ("Failed. Try again") with their assertions.
- **Dual-theme evidence, headless host**: this machine has no display, so the plan's `tauri dev`
  manual pass cannot run here. Substitute: `e2e/theme.spec.ts` (new, in the mock suite) emulates
  both color schemes against the real HTML/CSS and proves light tokens, dark tokens, and
  `[data-theme]` override precedence in both directions. Remaining risk is ONLY OS→webview
  propagation of `prefers-color-scheme`, which the CSS cannot affect; it rides the same
  deferred-with-consent contingency as the WebDriver run (real build, display required). The Rust
  `window.eval` fallback design stays specced in plan.md, unimplemented unless that check fails.
- Gate: frontend:build ✓ · Playwright mock suite **74 pass** (65 legacy adapted + 6 theme + guard
  additions) ✓ · root `bun run test` ✓ (tauri-identity.test.ts UNMODIFIED, zero diff) · cargo
  src-tauri + core: see below.
