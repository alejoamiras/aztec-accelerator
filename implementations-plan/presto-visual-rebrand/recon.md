# Recon — presto-visual-rebrand

Base: `origin/main` @ `8ab7df9` (worktree `worktree-presto-visual-rebrand`). Seven read-only explorers
(CSS tokens, fonts/CSP, icon pipeline, string map, test coupling, animation/JS, deploy/meta/responsive).
Design source of truth: the Presto Brand Book artifact + `implementations-plan/presto-visual-rebrand/plan.md`'s
token appendix (mirrors the settled decisions).

## Reuse map

| Capability needed | Existing code found | Verdict | Notes |
|---|---|---|---|
| Design-token plumbing (app) | `src-tauri/frontend/style.css:1-13` — flat 11-var `:root`, consumed ~90% via `var()`; `onboarding.css`/`renewal.css` define zero tokens by prior fix | **reuse-as-is** (swap values, extend for light/dark) | Cleanest surface. ~6 stray literals to patch (below) |
| Design-token plumbing (landing) | `packages/landing/src/style.css:10-21` — same 9 hexes, `--accent-on` naming variant | **adapt** | ~30 hardcoded `rgba(212,255,40,*)` literals in glows/rings/keyframes won't follow a `:root` edit |
| Design-token plumbing (playground) | `packages/playground/src/style.css:3-13` — Tailwind v4 `@theme` `--color-brand-*` | **adapt** | Same literal problem (~30) plus runtime Tailwind className strings in `main.ts`/`results.ts`; stock `red-400`/`amber-500`/`gray-700` used for status |
| Theme switching (light+dark) | None on any surface — zero `prefers-color-scheme` / `color-scheme` / `[data-theme]` in product code (grep trail in §absences) | **build new** — pure-CSS `@media (prefers-color-scheme)` token overrides per surface | Justified: settled decision "app follows OS"; media-query-only needs no JS and is CSP-safe. App precedent for state-styling without inline styles: `data-fill` on speed slider (F-012 pattern) |
| Webfont loading (web) | Google Fonts `<link>` in both `index.html`s (Space Grotesk/Inter/JetBrains Mono) | **reuse pattern** (swap families to Bricolage Grotesque / Figtree / Fragment Mono) | Preconnects already present |
| Webfont loading (app) | None — system stack only; no `@font-face` anywhere in repo | **build new** — vendored woff2 + `@font-face` | CSP `default-src 'self'` (no `font-src` directive → falls back to `'self'`) allows same-origin fonts **without any CSP edit**. Files MUST live in tracked `src-tauri/frontend/fonts/` — `frontend/assets/` is gitignored AND `rm -rf`'d by `scripts/build-frontend.ts:77-79`. Relative `url("fonts/x.woff2")` per the app's bare-relative-path convention. First vendored binaries in repo → add OFL attribution file |
| Icon/tray generation | None — icons were one-off exports (git history: single bulk add; ImageMagick one-off noted in windows-release lessons); no rasterizer dep anywhere | **build new** small generator (`packages/accelerator/scripts/generate-icons.ts`) | Constraints verified from real bytes: tray = 44×44 RGBA, **pure (0,0,0) RGB, alpha-only shading** (template purity); `.icns` has FULL 11-chunk retina ladder (16→1024), `.ico` 7 sizes 256→16 — regeneration must preserve complete ladders (a missing `.ico` is the historical failure mode). Numeric frame order 1..24 = rotation order |
| Tray animation pipeline | `src-tauri/src/tray.rs:12-37` — 26 literal `include_bytes!`; 50ms × 24 frames = 1.2s/lap; `icon_as_template(true)` re-set with `set_icon` in one main-thread closure (anti-flash) | **reuse-as-is** (assets swap only) | Prior quality audit explicitly decided to KEEP the 24 literal lines (no macro indirection) — do not re-litigate. `icon_as_template` is macOS-only (verified in tray-icon 0.23.1 source); Win/Linux render the black PNG as-is (pre-existing) |
| Proving-animation phase machinery | `packages/playground/src/ascii-animation.ts`: `PhaseQueue` (zero DOM coupling, `onChange` callback), `AnimationPhase` union, elapsed-timer block, `AsciiController` public shape `start(mode)/pushPhase/stop` driven from `main.ts:114,159,88-94,140,185` | **reuse-as-is**: `PhaseQueue` + `AnimationPhase` (+ export `MIN_DISPLAY_MS`); **replace**: frame generators + controller render loop with an SVG `SparkOrbitController` keeping the same public shape | E2E only asserts `#progress` visibility (`playground/e2e/fullstack.helpers.ts`) → renderer swap is e2e-safe. `ascii-animation.test.ts` asserts `<pre>.textContent` → replaced by a new test suite |
| Phase→dial-label mapping | None — 14 phases (incl. `denied`/`version-mismatch`/`fallback` terminal states); closest analog `stepToPhase` (`results.ts:191-193`) maps step names, one level removed | **build new** (pure function + tests) | Genuine new design decision, spelled out in plan.md |
| Hero entrance animation | Already pure CSS: `landing/src/style.css:798-827` fade-up/scale-in staggered delays | **reuse-as-is** (retime/restyle) | Task's "staggered reveal" is already built |
| Race bar (landing) | None — zero `requestAnimationFrame`/canvas in landing | **build new** (small JS + CSS) | Needs its own reduced-motion guard (see below) |
| Reduced-motion | Landing CSS blanket rule `style.css:930-936` only; playground none; **zero `matchMedia` in repo** | **build new**: playground CSS rule + JS `matchMedia` guard for JS-driven loops (spark orbit, race bar) | |
| Accelerator-detected state (landing) | `main.ts:103-134` health probe `127.0.0.1:59833/59834 /health`, DOM contract `.hero-sub.detected` + `.accel-dot` | **reuse-as-is** (restyle + new copy) | Network contract untouched |
| App popup JS conventions | `frontend-src/bridge.js`: `wireToggle`/`wireButton`/`showErrorHint`; hint anchor selector list `.row, .speed-section, .popup-container, .wiz .cta, .r .cta` | **reuse-as-is** | New/renamed markup must keep controls inside one of those containers or extend the list |
| Speed control | Native `<input type=range>` (`settings.html:79`, `settings.js:12-41,245-255`, CSS `style.css:290-373`); Rust `set_speed` + `Speed` enum | **adapt** (HTML/JS/CSS only) | Rust command + enum unchanged; keep `invoke("set_speed", {speed: level.value})` verbatim |
| og:image + meta | None (no `og:image`/`twitter:image` anywhere; twitter:card is `summary`); no `public/` dirs exist | **build new**: `packages/{landing,playground}/public/og-image.png` + meta tags; upgrade to `summary_large_image` | CloudFront function passes any extension path through (`infra/tofu/cloudfront.tf:61-66`) → stable URL `https://<domain>/og-image.png`. Landing ships on main-merge; playground only on manual `publish-testnet.yml` dispatch |
| Mobile nav | None — `.nav-links{display:none}` below 768px (`landing/src/style.css:674-677`); zero hamburger prior art repo-wide | **build new** (landing-only, small) | Follow existing `main.ts` enhancement style + co-locate media query |
| String rename map | Full RENAME/KEEP/AMBIGUOUS tables produced (below) | n/a | `scripts/tauri-identity.test.ts` pins `productName`/`identifier`/`publisher`/`copyright` — untouched scope means this file is the **drift guard** proving we didn't leak into operational identity |

## Load-bearing facts (by area)

### Tokens & colors
- App tokens (`frontend/style.css:1-13`): `--bg #0b0a06, --surface #131008, --surface-raised #1a1610, --border #2a2518, --border-accent (dead), --text #f0e8d0, --text-muted #9a9080, --accent #d4ff28, --accent-dim #b8dd1e, --accent-on-dark #0e1400, --danger #e5484d`.
- Naming drift for the same value `#0e1400`: `--accent-on-dark` (app) vs `--accent-on` (landing) vs `--color-brand-accent-on` (playground).
- Stray literals in app CSS: `style.css:333,338,352` shadows; `338/342` accent-rgba glows; `480 #fff`; `529 #16a34a` verified-check; `544 #4a90d9` dead fallback; **`559 var(--warning, #c98a2d)` — `--warning` never defined, the fallback ALWAYS renders**; `onboarding.css:16 #d9a441` warn amber.
- ~80+ hardcoded `rgba(212,255,40,α)` accent literals across landing (~30) + playground (~30) + app (2) — the #1 "half-done rebrand" risk. Playground also re-encodes neutrals as rgba (`rgba(19,16,8,*)` etc.). Landing re-encodes `--bg` at `:72`.
- Four unrelated ambers (`#d9a441`, `#c98a2d`, `#ffb347`, Tailwind `amber-500`); three danger mechanisms (`--danger` / none / Tailwind `red-400`).
- TEE-Rex teal family (`#4edea3` + rgba) — settled decision: that callout is hidden anyway.
- `theme-color` meta `#d4ff28` + favicon data-URIs in both `index.html`s duplicate the accent outside CSS.
- Playground stock classes in TS strings: `results.ts:178`, `main.ts:209,219,259-260` (`text-amber-500/80`, `text-red-400/80`).
- Playground already uses `oklch(from var(--color-red-400) l c h / 40%)` (`style.css:131-132`) — the derivation technique to standardize on.

### Fonts & CSP
- App CSP verbatim (`tauri.conf.json:13`): `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'` — no `font-src` → same-origin fonts allowed as-is.
- `build-frontend.ts` bundles ONLY the 5 JS entrypoints into gitignored `frontend/assets/` (wiped each run); CSS/HTML ship as tracked static files. CI invokes via `.github/actions/setup-accelerator/action.yml:126-129`.
- No woff/ttf/otf anywhere in repo; no vendoring/attribution precedent.

### Icons & tray
- Load-bearing files (31): `bundle.icon` 5 (`32x32.png,128x128.png,icon.png,icon.icns,icon.ico`) + `tray-idle.png` + `tray-proving-{1..24}.png` via `include_bytes!` (`tray.rs:12-37`). `256x256.png`/`512x512.png` exist but are NOT in `bundle.icon` (pre-existing gap, leave as-is). SVGs are design sources only.
- Current proving frames: 24-wedge diamond ring with a rotating 24-entry opacity ramp (comet effect) + fixed bolt; wedge coords are clean fractions → originally programmatic, script not preserved.
- `.desktop` `Icon={{icon}}` templated by tauri-bundler; NSIS takes installer icon from `icon.ico` automatically; no workflow copies/validates icons; **no test asserts icon correctness** (add one).

### Strings (rename map — condensed; full grep trails in the string-recon section below)
RENAME (user-visible, no operational coupling):
- `windows.rs:108,128,257` window titles; `tray.rs:138` tooltip.
- App HTML: `onboarding.html:6,20`, `settings.html:6,26`, `renewal.html:15`, `authorize.html:13,17`, `update-prompt.html:6,12`; `frontend-src/authorize.js:54`.
- Landing: `index.html:6,15,20,49,150,155,166,189` (footer: subject only; add "or the Aztec Foundation"); `src/main.ts:130`.
- Playground: `index.html:6,15,20,43,76,147`; `src/main.ts:37,40,92`; `ascii-animation.ts:139,144,153,159` (superseded by renderer swap).
- `verified-sites.json:19-20` displayName/description ("Presto Playground").
KEEP (operational; frozen): `tauri.conf.json` productName/identifier/publisher/copyright/homepage/updater endpoint; `CA_COMMON_NAME` + trust/* + `nsis/hooks.nsi:388-411`; NSS prefix `aztec-accelerator-ca-`; Cargo crate/bin names (`AztecAccelerator`); `APP_NAME` (autostart:54, crash_recovery:235), `TASK_NAME`, `SYSTEMD_NAME`, `PRODUCT_DIR_NAME`, `.aztec-accelerator` data dir; npm names; repo slug; domains; release artifact names; CI-internal names; `.desktop` `Name={{name}}` (templated from productName).
AMBIGUOUS → plan resolves each: OS-locating copy (`settings.js:139,144`, `autostart.rs:1921`, `commands.rs:669`) — user must find the app by its OS name → KEEP name inside these strings; narrative-only lock messages (`autostart.rs:725`, `uninstall.rs:151`) → reword to "Presto"-neutral phrasing is optional, KEEP for zero-risk; `tray.rs:83` "· Aztec X" = network version, KEEP; `main.rs:883` panic, KEEP; systemd/Task descriptions, KEEP; `tauri.conf copyright`, KEEP (test-pinned); `diagnostics.ts:218` filename → rename cosmetic, allowed; `aztec.ts:199-200` prose KEEP class name; demo token "Accelerator/ACEL" KEEP (content).
Dev-only: tray status item + Versions submenu; "Show Logs" is in BOTH builds (not dev-only).

### Tests (blast radius)
- Zero color/snapshot/icon assertions anywhere (no .snap, no toMatchSnapshot, no screenshots, no insta).
- Playwright mock suite = `packages/accelerator/e2e` (65 tests, `test:e2e:ui`): guaranteed break only `settings.spec.ts:262-264` (full product name in repair copy — stays passing under our KEEP resolution); ~18 copy-string assertions break only if that copy is reworded (buttons "Start"/"Continue"/"Update Now", speed labels, empty-state, error hints).
- WebDriver = `packages/accelerator/e2e-webdriver` (17 tests, `test:e2e:webdriver`): `helpers.ts:19` + `trust-boundary.spec.ts:22` + `smoke.spec.ts:19` pin `"Aztec Accelerator Settings"` — must move in lockstep with `windows.rs:108`; `auth-flow.spec.ts:189` pins "Authorize Site" (unchanged title). No tray/icon assertions.
- Rust: `core/src/server.rs:80-85` `display_text()` ("Status: Proving...") pinned by `core/src/server/tests.rs:597` — feeds tray tooltip/status; reword ⇒ update that test. `verified_sites.rs:141-145` pins the playground DOMAIN (unchanged). Window titles untested in Rust.
- TS: `ascii-animation.test.ts` (13) replaced wholesale with spark-orbit suite; `tauri-identity.test.ts` (14) must stay GREEN untouched (drift guard). `release-contract`, `trust-boundary` size-drift guard: agnostic.
- Run commands: root `bun run test` = lint+typecheck+TS unit (NO Rust, NO Playwright/WebDriver). Rust: `cargo test` in `src-tauri` + `cargo test --manifest-path ../core/Cargo.toml`. Playwright mock: `bun run --cwd packages/accelerator test:e2e:ui`. Playground mocked e2e: `bun run --cwd packages/playground test:e2e`. WebDriver: build with `--features webdriver` then `bun run --cwd packages/accelerator test:e2e:webdriver`.

### Deploy / CI
- Branch push without PR fires ZERO CI (only `deploy-landing.yml`, cache-warmers trigger on push, all restricted to `main`). PR would fire `landing.yml` / `app.yml` / `accelerator.yml` per paths-filter.
- Landing deploys on main-merge (S3 sync of `dist/`, byte-for-byte); playground ships only via manual `publish-testnet.yml`.

### Animation/JS details
- `stepToPhase` gap: WASM-mode mint/transfer/balance steps emit no phase → animation freezes on last phase (pre-existing; dial should tolerate idle phases gracefully).
- `handleProverPhase` (`main.ts:88-94`) special-cases `fallback` outside the renderer — error-state visuals need the same hook.
- Hint/markup constraint: keep controls inside bridge.js's anchor selectors.

## Dedup / collision risks for the plan
1. Literal-alpha accent copies (~80+) must be converted to token derivations (`color-mix` / Tailwind `/α` / `oklch from`) or they silently stay lime.
2. Same-value-different-name token triplication: use one Presto token sheet applied three times with per-surface naming kept (visual-only scope ⇒ no shared package; recorded as rejected alternative).
3. Phantom `--warning` fallback and dead `--border-accent`: fix/drop while in the file.
4. WebDriver title constants must change in the same commit as `windows.rs` titles.
5. Frame filenames/order (1..24, non-padded) and template purity are enforced by the new generator + validation test.
6. Favicon/theme-color/meta duplicates of the accent live in HTML, not CSS.

## Absence search trails (condensed)
- No shared token package / cross-package CSS import: workspaces list (4 pkgs), `find -iname "*token*"`, sdk tree has zero CSS, only `@import "tailwindcss"` exists.
- No `prefers-color-scheme`/`color-scheme`/`matchMedia` in product code: repo-wide grep; hits only under `implementations-plan/**`/`audit/**` report HTML.
- No `@font-face`, no `*.woff*`/`*.ttf`/`*.otf`, no fontsource/webfont deps: repo-wide find/grep.
- No icon generation tooling: scripts dirs enumerated; grep `tauri icon|rsvg|imagemagick|resvg|sharp|magick` (only OS build-deps + a lessons-file prose mention); `build.rs` read in full; git history shows bulk adds.
- No snapshot tests: `find *.snap`, grep `toMatchSnapshot|toHaveScreenshot|insta::`.
- No hamburger/mobile-nav prior art: grep `hamburger|menu-toggle|mobile-nav|nav-toggle|burger` → zero.
- No spark/orbit/race prior art, no rAF/canvas in landing/playground: grep → zero.
- No "presto" anywhere in repo yet: grep → zero.
