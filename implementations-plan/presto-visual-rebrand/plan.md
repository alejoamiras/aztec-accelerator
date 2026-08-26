# Plan — presto-visual-rebrand (v2, post dual-audit)

**Objective**: apply the settled Presto design system (Brand Book artifact
https://claude.ai/code/artifact/abd18e7a-6010-439a-b881-237e211be711; deck rollout map
https://claude.ai/code/artifact/4d9c1533-7d06-4041-a186-a8c087e5397d) to every user-visible surface —
landing, playground, the desktop app's five webviews, icons/tray, fonts, light+dark themes, and the
"Presto" name in UI copy — with **zero operational identity changes**.

**Hard exclusions** (future project, not planned here): repo rename, domain/URL migration + redirects,
npm package rename, `tauri.conf.json` `productName`/`identifier`/`publisher`/`copyright`/homepage,
updater endpoints, CA common name + NSS nickname prefix, Cargo crate/bin names, autostart/task/systemd
names, install/data dir names, release artifact names. `scripts/tauri-identity.test.ts` must pass
**unmodified**; because it does NOT pin everything frozen (audit finding), Phase 5 adds a committed
frozen-identity sweep that does.

**Tier**: `mid` (0 HIGH rubric dimensions; multi-package + Rust asset coupling + large test surface
keeps it above `light`). **Delivery mode**: branch preview only — no PR until the owner flips.
**eli5_mode**: Artifact. ELI5 source: `implementations-plan/presto-visual-rebrand/eli5.html`.
ELI5 Artifact URL: https://claude.ai/code/artifact/ea7920bd-794d-4707-b33c-708207e4ea33

---

## Design inputs (settled palette/type/motion; copy anchors corrected by audit)

- **Palette (light)**: paper `#FFFAF1`, card `#FFFFFF`, wash `#FFF3DD`, ink `#241B33`, dim `#6E6580`,
  line `#ECE3D4`, cobalt `#3B4FE0` (all action), cobalt-deep `#2F40C4`, silk `#F04E42`
  (celebration/danger family — **never text-sized on paper**; danger text `#B02D23`), gold `#FFC53D`
  (+ gold-text `#7A5A00`), go-green `#189E62`.
- **Palette (dark twin, designed not inverted)**: paper `#191226`, card `#221A33`, wash `#2A2140`,
  ink `#F1EBE0`, dim `#9D93B0`, line `#372C4E`, cobalt `#8B99FF` (on-cobalt `#141026`),
  silk `#FF6A5C`, gold `#FFD066`, go `#3FCE8C`. Text-safe status tokens: `--go-text` `#147A4C`
  (light) / `#3FCE8C` (dark) — `go` itself is dots/LEDs only, never text on paper (3.31:1). Every
  surface declares `color-scheme: light dark` and ships a dark `theme-color` meta twin (web).
- **Type**: Bricolage Grotesque (display), Figtree (UI/body), Fragment Mono (origins/numbers).
  Web: Google Fonts links (existing posture). App: vendored **fontsource-built woff2, unmodified**
  (no self-subsetting; sidesteps OFL Reserved-Font-Name), static weights, latin + latin-ext.
- **Radius scale** 10/16/999; one soft shadow; sparks (✦) only at success; sober consent surfaces.
- **Proving theater**: spark-orbit dial; tray = spark orbiting the bolt, icon-only; status text in
  tooltip only.
- **Copy anchors**, with one audit correction: the Brand Book's single-line cert note ("Your system
  will ask you to approve it once") is **factually wrong on Linux/Windows** and is NOT adopted.
  The app keeps per-OS variants (reworded to Presto tone, facts preserved — macOS: password prompt
  once; Windows: installed into browsers on Start, no OS prompt; Linux: no separate prompt).
  Follow-up: correct the Brand Book artifact after approval. Everything else per Book: wizard
  "Hi! Three quick choices.", authorize "Can this site use Presto?", banner "Early days · Use at
  your own risk", footer "…not affiliated with Aztec Labs or the Aztec Foundation.", no em dashes.

## Naming rule (resolves recon's AMBIGUOUS table)

UI copy says **Presto**. A string keeps "Aztec Accelerator" iff it directs the user to an OS surface
where the frozen `productName` (or CA CN) is what they'll see:
- KEEP name: `frontend-src/settings.js:139,144`, `src/autostart.rs:1921,725`, `src/commands.rs:669`,
  `src/uninstall.rs:151`, `nsis/hooks.nsi:406`, `main.rs:883`, systemd/Task descriptions,
  `tray.rs:83` version line ("Aztec" = network version). Also KEEP exact-fact status strings fed to
  the tray from `main.rs:366` ("Error: port 59833 in use"/"Error: server failed") and `main.rs:820`
  ("Warning: bb not found").
- RENAME: recon Table 1 (window titles, tray tooltip "Presto", app HTML/JS copy, landing,
  playground, `verified-sites.json` displayName → "Presto Playground" + description). Landing
  footer: "Built for the Aztec ecosystem by Alejo Amiras. Presto is not affiliated with Aztec Labs
  or the Aztec Foundation."
- Status voice (`core/src/server.rs:80-85` `display_text()`): reworded to Presto tone with the same
  facts. Same-commit companions (audit): `core/src/server/tests.rs:597` assertion, the
  byte-identity doc comments at `server.rs:66` and `tests.rs:528-529`, and the initial dev-menu
  literal `"Status: Idle"` at `src-tauri/src/main.rs:728`.
- `playground/src/diagnostics.ts:218` → `presto-diagnostics-*.json`. `AcceleratorProver` class name
  and demo token "Accelerator"/ACEL: KEEP (SDK API / content).

**RESOLVED ASK (owner, 2026-08-26) — name-mismatch bridging copy: option (b), three seams.** One
plain sentence at: the landing download section ("Installs as 'Aztec Accelerator' until our full
rename"), the HTTPS row / Manage-certificate disclosure (naming the CA by its CN "Aztec
Accelerator Local CA"), and the update prompt. Trivially removable at the operational rename.

---

## Architecture & Implementation

### Proposed architecture
Four per-surface reskins sharing one written token sheet (above), plus a small asset toolchain. No
shared token package (visual-only scope; ledger). Each surface keeps its token *mechanism* — flat
`:root` vars (app: **defined in `style.css` only**, never re-introduced into
`onboarding.css`/`renewal.css`; landing likewise), Tailwind v4 `@theme` (playground) — with Presto
values, a `@media (prefers-color-scheme: dark)` token-override block (pure CSS, CSP-safe),
`color-scheme: light dark`, and **all hardcoded accent/neutral alpha literals converted to token
derivations** (`color-mix(in srgb, var(--accent) N%, transparent)`; playground: Tailwind `/α` or its
existing `oklch(from var(...))` pattern). Compatibility floor (audit-corrected): the app's
`minimumSystemVersion` is **macOS 10.15**, whose WebKit predates `color-mix` (Safari 16.2+) — so in
app CSS every `color-mix`/`oklch(from …)` usage is PRECEDED by a static literal fallback
declaration of the same computed color — **per theme**: the dark token/override blocks carry their
own dark literals before their derivations, so a Catalina-era engine gets correct colors in BOTH
themes rather than light literals bleeding into dark. Static (non-variable) woff2 weights in the
app; web targets evergreen browsers.

Asset toolchain (`packages/accelerator/scripts/generate-brand-assets.ts`, target-scoped):
- `--target app-icons`: wraps **`bunx tauri icon`** (Tauri CLI is already a devDep) on the 1024px
  master, run into a STAGING dir, then normalizes: copy the 5 `bundle.icon` files
  (`32x32.png`, `128x128.png`, `icon.png`, `icon.icns`, `icon.ico`) under their existing names,
  discard the CLI's extra outputs (`128x128@2x.png`, `Square*Logo*`, store icons), and render
  `256x256.png`/`512x512.png` via resvg (the CLI does not emit them; the repo tracks them) so the
  on-disk file set is byte-for-byte name-identical to today's. NO hand-written container code.
- `--target tray`: renders `tray-idle` + 24 spark-orbit frames (spark at `(N−1)×15°`) at 44×44 via
  **`@resvg/resvg-wasm`** (pure-WASM sibling of resvg-js: no per-platform native blobs), with
  explicit `initWasm()` on the package's local `index_bg.wasm` (initialization is mandatory),
  `loadSystemFonts: false`, and every font buffer passed explicitly (determinism); post-pass forces
  RGB to pure black (template purity).
- `--target og-landing` / `--target og-playground`: renders the 1200×630 cards into each package's
  `public/` — invoked from those surfaces' phases, not from a global asset phase.
- SVG masters REPLACE the existing ones in place (`src-tauri/icons/icon.svg`, `tray-*.svg`) — no new
  `icon-sources/` dir, no stale old-brand SVGs left for the sweep to trip on.
- Validation `packages/accelerator/scripts/icon-assets.test.ts` (runs inside `test:unit`): asserts
  the spec **derived from the real regenerated bytes** — tray: 25 files, 44×44, RGBA, zero
  non-black RGB; icns/ico: parse containers and assert full ladders as produced by `tauri icon`
  (chunk/entry enumeration recorded in the test from actual output, not assumed "PNG payloads" —
  the committed originals use ARGB ic04/ic05 and BMP ico entries, so payload formats are asserted
  from what the CLI emits). Plus a sha256 manifest test covering vendored woff2 AND generated
  binaries (fonts get the same integrity control as icons).

### Key interfaces / types
- `SparkOrbitController` keeps `AsciiController`'s public shape (`constructor(host, elapsedEl?)`,
  `start(mode)`, `pushPhase(p: AnimationPhase)`, `stop()`); `PhaseQueue`/`AnimationPhase` extracted
  to `packages/playground/src/phase-queue.ts` and reused unchanged (export `MIN_DISPLAY_MS`).
  `phaseToDial(p): {quadrant: 0|1|2|3, mode?: "fallback"}` (pure, unit-tested): 0 witness =
  detect/downloading/serialize/app:simulate · 1 sync = transmit · 2 prove = proving/app:prove ·
  3 ta-da = proved/receive/app:confirm. **`denied`/`version-mismatch`/`fallback` are NOT terminal**
  (verified: SDK `#fallbackToWasm` emits `fallback` → local prover → `receive`): they switch the
  dial to "in-browser proving" treatment (dim track, label swap, log line via the existing
  `handleProverPhase` hook) and progress CONTINUES. **Idle-phase tolerance** (recon's `stepToPhase`
  gap): when no phase event arrives mid-lap, the dial holds position with the spark breathing and
  the elapsed timer running — never spins on fake progress.
- Speed control: **restyle the existing native `<input type="range">`** using its own
  `data-fill`/track/thumb machinery (recon's "adapt" verdict; both e2e suites drive `#speed` as an
  HTMLInputElement — that mechanic is preserved). No custom ARIA control.
- Reduced motion: one `matchMedia("(prefers-reduced-motion: reduce)")` guard used by the dial and
  the landing race loop; behavior is specified (dial renders final state, race renders final
  numbers) and unit-tested via an injected flag.

### Consent-surface invariants (immutable acceptance criteria, Phase 2)
Copy/styling changes may never weaken, and their tests may never be deleted or loosened:
server-sourced origin binding; raw origin displayed untruncated, selectable, bidi-isolated
(`unicodeBidi`/`userSelect` assertions stay); recognition badge never replaces the raw origin;
permanence disclosure present; Allow/Deny reachable and keyboard-focusable at 400×300; update
prompt binds action to the displayed version; deny-by-default semantics untouched.

### Data & control flow (critical path: a proof in the playground)
SDK `onPhase` → `handleProverPhase` → `SparkOrbitController.pushPhase` → `PhaseQueue` (1s pacing)
→ `phaseToDial` → SVG attribute writes (accumulating rotation, label `.on`, bolt pop on ta-da) +
elapsed text; fallback family flips the dial mode and keeps flowing.

### File-level change map
| Area | Files | Change |
|---|---|---|
| App CSS | `src-tauri/frontend/style.css` (tokens live HERE only), `onboarding.css`, `renewal.css` | Presto tokens + dark block + `color-scheme`; `@font-face`; literal fixes (`:333-352,480,529,544,559`, `onboarding.css:16`); slider restyle; drop dead `--border-accent`, define real `--warning` |
| App HTML/JS | `frontend/*.html`, `frontend-src/*.js` | Copy per Book (cert note stays per-OS, Presto-toned); emoji → inline SVG; cert note into HTTPS row; bridging copy per owner Ask; controls stay inside bridge.js hint-anchor containers |
| App Rust | `windows.rs:108,128,257`, `tray.rs:138`, `main.rs:728`, `core/src/server.rs:66,80-85` + `core/src/server/tests.rs:528-529,597` | Title/tooltip/status strings + their doc comments/tests, same commits |
| Icons | `src-tauri/icons/*` (30 load-bearing: 5 bundle + 25 tray include_bytes, same names; SVG masters replaced in place), `scripts/generate-brand-assets.ts`, `scripts/icon-assets.test.ts` | `tauri icon` ladder + resvg-wasm tray frames; sha256 manifest; `bundle.icon` list untouched |
| Fonts | `src-tauri/frontend/fonts/*.woff2` + `fonts/LICENSES.md` (per-family OFL, upstream package@version, sha256); web `index.html` font links | Vendor app fonts (fontsource-built, unmodified); swap web families |
| Playground | `src/style.css`, `index.html`, `main.ts`, `ui.ts`, `results.ts`, `aztec.ts` (log strings), `diagnostics.ts:218`, NEW `spark-orbit.ts`/`phase-queue.ts` (+tests), DELETE `ascii-animation.ts` (+test), `public/og-image.png` | Reskin + renderer swap + strings + reduced-motion + meta/favicon/`color-scheme` |
| Landing | `index.html`, `src/style.css`, `src/main.ts`, NEW `src/race.ts`, NEW minimal tsconfig + `typecheck` script (audit: landing currently ships untypechecked), `public/og-image.png` | Full Presto landing per Book; TEE-Rex hidden; footer + Foundation; mobile nav (keyboard/aria correct); og/meta/favicon; dark block |
| Tests | `accelerator/e2e/*.spec.ts` copy updates; `e2e-webdriver/{helpers.ts:19, trust-boundary.spec.ts:22, smoke.spec.ts:19}` lockstep; `core/src/server/tests.rs`; NEW `spark-orbit.test.ts`, `icon-assets.test.ts`, `scripts/brand-sweep.test.ts` | Same-commit as the strings they pin |
| Untouched (guards) | `scripts/tauri-identity.test.ts`, `tauri.conf.json`, `Cargo.toml`, certs/trust/autostart/NSIS identity constants, all CI workflows | Green, unmodified |

### Algorithms / non-obvious mechanics
- **Brand sweep as a committed test** (`scripts/brand-sweep.test.ts`, runs in root `test:scripts`):
  (a) retired-value scan with word-boundary/quoted patterns per surface (`d4ff28`,
  `rgba(212, 255, 40`, `b8dd1e`, `"Space Grotesk"`, `"Inter"` as a quoted font-family — NOT bare
  `Inter`, which matches `IntersectionObserver`/"Interactive"); (b) frozen-identity fixture: the
  KEEP table as data — asserts each frozen string still present byte-identical at its site AND that
  "Presto" appears nowhere in frozen files (tauri.conf.json, Cargo.toml, certs/trust/NSIS,
  workflows). The fixture enumerates EVERY hard exclusion, not just the UI-adjacent KEEP rows
  (audit): productName/identifier/publisher/copyright, updater endpoint URL, CA CN + NSS prefix,
  autostart/task/systemd names, install/data dir names, npm package names, domains, release
  artifact name patterns.
- **Contrast guard**: a unit test that PARSES the actual custom-property declarations out of the
  three shipped CSS sources (no duplicated constants — audit) and computes WCAG ratios for every
  text-role/background pair in both themes (ink, dim, danger-text, gold-text, go-text, on-cobalt),
  failing under 4.5:1. `silk` and `go` are excluded from text roles by construction (they fail on
  paper; their `-text` twins carry text).
- Tray frames: numeric 1..24 order = rotation order; generator refuses to emit if any frame
  contains non-black RGB after flattening.

### Trade-offs & alternatives not taken
- No shared token package (refactor creep; literal→derivation conversion captures the maintenance
  win in-file). Media-query-only theming; `data-theme` toggle deferred (app follows OS). If the
  Phase 2 check finds a webview NOT propagating `prefers-color-scheme`, the fallback is
  **Rust-side** (audit-corrected — the frontend capability files intentionally grant no core
  window/event API, and stay untouched): the initial theme is applied from
  `on_page_load(PageLoadEvent::Finished)` (an eval fired right after `build()` can target the
  transient pre-navigation document and be lost), re-applied on every page load/reload, and
  `WindowEvent::ThemeChanged` drives subsequent changes — each via `window.eval()` setting
  `document.documentElement.dataset.theme`. No capability grants, no inline styles, CSP
  unaffected; the CSS token blocks honor `[data-theme]` overrides alongside the media query (same
  triple-block pattern as the Brand Book).
- `tauri icon` over hand-written icns/ico writers (audit: committed containers use ARGB/BMP
  payloads a naive PNG-container writer would silently get wrong; the CLI is already pinned in the
  lockfile). resvg-wasm over resvg-js napi (no native blobs) over sharp (bigger native dep).
- Restyle native slider over custom ARIA control (audit + recon: free native semantics; both e2e
  suites drive it as an input).
- Delete ASCII theater (git preserves; dev-mode easter egg stays future work).
- **Competing outline B** (recolor-first): rejected — see ledger.

---

## Phases

Common prerequisite used below: `FB = bun run --cwd packages/accelerator frontend:build` (build.rs
verifies bundle hashes; required before any `cargo` gate after JS/lockfile changes).

### Phase 1 — Toolchain + app brand assets ✓ (gate green 2026-08-26)
Pin devDeps (`@resvg/resvg-wasm`, fontsource families — exact versions; `bun install`, commit
`bun.lock`; min-age gate applies). Replace SVG masters in place; build `generate-brand-assets.ts`
(`app-icons` via `tauri icon`, `tray` via resvg-wasm); regenerate the 30 load-bearing icon files
under existing names; vendor woff2 + `fonts/LICENSES.md`; write `icon-assets.test.ts` (spec derived
from real regenerated bytes) + sha256 manifest.
**Gate**: `bun install` (lockfile committed) → `FB` → `bun run test` → `bun run --cwd
packages/accelerator test:unit` → `cargo test` (in `src-tauri`). Pass: all exit 0; asset test green.
Layers: lint/typecheck/unit (TS+Rust).

### Phase 2 — Desktop app reskin ✓ (gate green 2026-08-26; dual-theme via theme.spec.ts, OS-propagation deferred with the WebDriver contingency)
Tokens (style.css only) + dark block + `color-scheme` + `@font-face` + literal fixes; five windows'
markup/copy per Book with per-OS cert facts; emoji → SVG; slider restyle; bridging copy per owner
Ask; `windows.rs`/`tray.rs`/`main.rs:728` strings; `display_text()` + doc comments + test;
`verified-sites.json`. Playwright mock specs and the three WebDriver title constants updated in the
SAME commits as their strings. Consent invariants (above) checked against the updated specs.
**Gate**: `FB` → `bun run test` → `cargo test` (src-tauri) → `cargo test --manifest-path
../core/Cargo.toml` → `bun run --cwd packages/accelerator test:e2e:ui`. Pass: all exit 0; Playwright
mock suite green; `tauri-identity.test.ts` green UNMODIFIED. Layers: lint/typecheck/unit (TS+Rust) +
mocked UI e2e. In-phase: `bunx tauri dev` manual pass over all five windows, OS light AND dark
(screenshots into lessons/; if dark does not propagate, implement the documented
`onThemeChanged`→`data-theme` fallback IN THIS PHASE — light-only is not an outcome).

### Phase 3 — Playground reskin + spark-orbit theater ✓ (gate green 2026-08-26)
`phase-queue.ts` extraction; `spark-orbit.ts` + tests (phaseToDial table incl. fallback-continues
and idle-hold; reduced-motion via injected flag); delete `ascii-animation.ts` + test; rewire
`main.ts`/`index.html`; `@theme` Presto + dark + `color-scheme` + literal conversion (incl. TS
className strings; brand danger/warning tokens replacing stock red-400/amber-500/gray-700 where
brand-visible); header lockup; services/banner/log/results copy; fonts link; favicon/theme-color
(light+dark)/meta; `--target og-playground`; the contrast-guard test lands here covering the TWO
converted sources (app + playground) and is extended to the third in Phase 4 when landing's tokens
exist — its per-phase contract matches what is actually converted.
**Gate**: `bun run test` → `bun run --cwd packages/playground test:e2e` → `bun run --cwd
packages/playground build`. Pass: exit 0; spark-orbit suite green; `#progress` contract intact;
`dist/` contains `og-image.png`. Layers: lint/typecheck/unit + mocked e2e + build. In-phase: dev
server manual pass, light + dark.

### Phase 4 — Landing rebuild ✓ (gate green 2026-08-26)
Full Presto landing per Book (banner trim; hero + retimed CSS stagger + `race.ts`; 4 feature tiles;
"Why presto?"; TEE-Rex hidden with re-enable comment; footer + Foundation; detected-state restyle on
the existing probe contract; bridging sentence per owner Ask); mobile nav (button, `aria-expanded`,
Escape/focus handling); og/meta/favicon/theme-color (light+dark); dark block + `color-scheme`;
NEW landing tsconfig + `typecheck` wired into its `test` script; `--target og-landing`; extend the
contrast guard to landing's tokens (completing its three-source contract).
**Gate**: `bun run test` → `bun run --cwd packages/landing test` (now includes typecheck) → `bun run
--cwd packages/landing build`. Pass: exit 0; `dist/og-image.png` exists. Layers:
lint/typecheck/unit + build. In-phase: dev server manual pass at 1280px/375px, light + dark,
keyboard-only nav check.

### Phase 5 — Integration sweep + full E2E ✓ (gate green 2026-08-26; WebDriver leg deferred-with-consent per contingency — no display/Xvfb on this host, launcher exit 2 recorded in lessons/phase-5.md)
`scripts/brand-sweep.test.ts` (committed; retired-values + full frozen-identity fixture). Full
suites + WebDriver on this Linux host (validates the title lockstep for real). Because
`wdio.conf.ts` expects an ALREADY-RUNNING app (audit), the WebDriver leg is a committed launcher
`packages/accelerator/scripts/run-webdriver-local.sh` modeled on `_e2e-webdriver.yml`'s steps:
**display preflight first** — if `$DISPLAY`/`$WAYLAND_DISPLAY` is absent, start an OWNED
Xvfb + dbus session + stalonetray stack exactly as CI does, and if those binaries are missing
(verified: this host currently has neither a display nor Xvfb), stop and trigger the contingency
below; then build with `--features webdriver`; pre-check ports 4445/59833 are free (fail fast with
the owning pid — this host may run a real accelerator); launch the binary detached in its OWN
process group with logs captured into `lessons/`; wait on readiness (`:4445` webdriver +
`:59833/health`); run `bun run --cwd packages/accelerator test:e2e:webdriver`; `trap`-guaranteed
teardown kills only the owned pgids (app AND the display stack it started).
**Gate**: `FB` → `bun run test` (includes lint + the new sweep via `test:scripts`) → `bun run --cwd
packages/accelerator test:e2e:ui` → `packages/accelerator/scripts/run-webdriver-local.sh`. Pass:
all exit 0; WebDriver suite green. Layers: all except live-network e2e (unaffected by a reskin;
runs in CI at PR-flip).
**Contingency** (explicitly includes the missing-display case): if the local webdriver run is
blocked by system deps — no display and no Xvfb/stalonetray installed (the current state of this
host), or build deps — the implementer does NOT install system packages autonomously. Record in
lessons/, offer the owner the one-line fix (`sudo apt install xvfb stalonetray dbus-x11`, then
rerun the script) in the wrap-up report, and absent that, the suite becomes the first CI check at
PR-flip ("deferred-with-consent", never silent).

---

## Competing outline B (alternative not taken — recolor-first, rebuild-later)
B1 cross-surface token swap + literals + fonts (instant whole-product recolor); B2 strings sweep;
B3 structural rebuilds (dial, race, feature grid, nav, slider, emoji→SVG); B4 assets + E2E.
Pros: earliest whole-product color preview; smallest early diff. Cons: three touch-passes per
surface; repeated Playwright/WebDriver churn; interim Presto-color-on-old-copy states; interleaved
commits ruin a later per-surface PR split. Both auditors concurred with A; fable suggested an
optional P3/P4 swap (landing first as cheapest de-risk) — rejected: the playground carries the
riskiest new code (renderer swap), and earlier means more soak time before Phase 5.

## Decision ledger
| Decision | Source | Status |
|---|---|---|
| Outline A over B | draft + both audits concur | adopted |
| Naming rule + KEEP list; exact-fact tray error strings classified KEEP | draft + codex/fable | adopted |
| Per-OS cert copy retained (Book's single line rejected as factually false); Book correction queued | codex H + fable H-1 | adopted |
| Bridging copy at name-mismatch seams: option (b), three seams | codex Ask-High + fable M-1 | **resolved by owner at gate (2026-08-26)** |
| `denied`/`version-mismatch`/`fallback` continue (not terminal); idle-hold specified | codex High (verified in SDK) + recon gap | adopted |
| `tauri icon` for app ladder; resvg-**wasm** for tray/og; masters replaced in place; spec derived from real bytes; sha256 manifest incl. fonts | codex M + fable C-1/C-3/M-2/M-3/L-1 | adopted |
| Slider: restyle native input (no custom ARIA control) | codex M + fable C-2 (recon "adapt") | adopted |
| Gates rewritten: `FB` prerequisite, real commands, landing typecheck added, redundant lint dropped, magic-number pass criteria removed | codex High + fable C-4 | adopted |
| Committed brand-sweep + frozen-identity fixture (identity test alone insufficient); word-boundary patterns | codex High/M + fable C-5 | adopted |
| Consent invariants enumerated as immutable criteria | codex High | adopted |
| Contrast guard test; silk never text-sized on paper | codex M | adopted |
| `color-scheme: light dark` + dark theme-color | codex M + fable C-6 | adopted |
| Dark-mode propagation resolved IN Phase 2 with concrete `onThemeChanged` fallback (light-only not an outcome) | codex M | adopted |
| Fonts: fontsource-built unmodified, full latin(+ext), exact pins, per-family licenses | codex M + fable M-3 | adopted |
| Tokens defined in app `style.css` only | fable C-7 | adopted |
| Facts corrected: 25 include_bytes / 30 load-bearing / icns ARGB+BMP reality / push-trigger nuance / breakage-set scope | fable F3 + codex Facts | adopted |
| Tray on Windows/Linux keeps today's single black template asset (macOS-only auto-invert); a per-OS variant is future work, out of visual-parity scope | recon + final codex | adopted (explicit decision, was previously implicit) |
| Phase 5 WebDriver gate = committed launcher script (build, port pre-check, owned pgid, readiness, trap teardown) | final codex blocker | adopted |
| Theme fallback moved Rust-side (`window.eval` on ThemeChanged; capability files untouched) | final codex blocker | adopted |
| `tauri icon` staging + filename normalization (CLI emits `128x128@2x`, not 256/512 — those two rendered via resvg) | final codex | adopted |
| resvg-wasm explicit `initWasm` + font buffers | final codex | adopted |
| `color-mix` static-fallback declarations (macOS 10.15 floor, verified `minimumSystemVersion`) | final codex | adopted |
| Contrast guard parses real CSS tokens; `--go-text` added (go 3.31:1 on paper is non-text) | final codex | adopted |
| Frozen-identity fixture enumerates ALL hard exclusions | final codex | adopted |
| P3/P4 order swap (landing first) | fable optional | rejected — riskiest code earlier |
| Landing Google Fonts → self-host | fable L-2 | rejected for this plan — existing posture, separate decision |
| Old ASCII theater kept as dev easter egg | Brand Book earmark | rejected here — future work, git preserves |
| Unresolved disputes | — | none; one OPEN ASK above |

## Security & Adversarial Considerations
- **Consent surfaces**: copy/styling only; the enumerated invariants are acceptance criteria and
  their tests may not be weakened. Per-OS cert facts preserved. Anti-spoof assertions
  (`unicodeBidi`/`userSelect`) stay.
- **Name-mismatch grooming risk** (Presto UI → "Aztec Accelerator" OS prompts): mitigated by the
  bridging-copy Ask; whichever option the owner picks is recorded here.
- **CSP**: unchanged; vendored fonts are same-origin under `default-src 'self'`. No new external
  loads in the app.
- **Supply chain**: new devDeps exact-pinned behind `minimumReleaseAge=604800` + committed
  `bun.lock` (CI `--frozen-lockfile`); resvg-wasm avoids per-platform native blobs; vendored woff2
  are fontsource-built binaries with per-file sha256 + upstream package@version in
  `fonts/LICENSES.md`, integrity-tested. Generated binaries covered by the same sha256 manifest;
  regeneration determinism pinned (`loadSystemFonts:false`, explicit fonts); a resvg version bump
  legitimately regenerates bytes and the manifest together — never by weakening the test.
- **Input validation**: no new trust-boundary inputs; new DOM via `createElement`/`textContent`
  (the one legacy static-`innerHTML` in `landing/main.ts:128-131` is not a license — new code, and
  its restyle, use node building).
- **Least privilege / CI**: no workflow changes; the only push-triggered workflows are main-only
  except `_e2e-crash-recovery-windows.yml`, whose two path filters this plan never touches.
- **/harden**: not warranted by this plan; the standing pre-release `/harden security`
  recommendation is unaffected.

## Assumptions
**Facts** (verified; corrected per audits):
1. App CSP allows same-origin fonts without edits (`tauri.conf.json:13`, no `font-src`).
2. `frontend/assets/` is gitignored and wiped every build (`build-frontend.ts:77-79`) — fonts live
   at tracked `frontend/fonts/`.
3. Tray coupling: **25** `include_bytes!` paths (`tray.rs:12-37`); frames 44×44 RGBA pure-black;
   committed `icon.icns` = 10 image chunks + `info`, with **ARGB** ic04/ic05; `icon.ico` = 7 **BMP**
   entries — regeneration asserts the spec of the NEW artifacts as actually emitted.
4. No test asserts a color, icon byte, or snapshot; breakage set = enumerated string assertions PLUS
   interaction/layout mechanics (slider drive in both e2e suites — moot under restyle; window-size
   fit tests unaffected by copy).
5. Push-triggered CI: main-only except `_e2e-crash-recovery-windows.yml` (two paths outside this
   plan) — the planned changes fire zero CI on branch push.
6. `PhaseQueue` has zero DOM coupling; playground e2e asserts only `#progress` visibility.
7. WebDriver pins "Aztec Accelerator Settings" in exactly three places (helpers.ts:19,
   trust-boundary.spec.ts:22, smoke.spec.ts:19); it also drives `#speed` as an input
   (`e2e-webdriver/settings.spec.ts:48-54`) — preserved by the restyle decision.
8. `bun run test` = lint + typecheck + TS unit only; Rust and Playwright/WebDriver commands are as
   written in the gates; root `test` already includes `lint`.
9. SDK fallback flow: `denied`/`version-mismatch` → `fallback` → local prover → `receive`
   (`accelerator-prover.ts:607-673`) — dial treats them as mode switch, not failure.
**Inferences** (attackable):
1. `@resvg/resvg-wasm` (exact pin) clears the min-age gate and renders the masters faithfully —
   verified at Phase 1; fallback `@resvg/resvg-js`, same gate.
2. Webviews propagate `prefers-color-scheme` — verified in Phase 2; if not, the Rust-side
   `window.eval` fallback (no capability changes) makes the phase outcome dual-theme either way.
   Companion inference: `@resvg/resvg-wasm` initializes from its local `index_bg.wasm` under Bun —
   verified at Phase 1; fallback `@resvg/resvg-js`, same min-age gate.
3. Vendored latin+latin-ext static woff2 across three families ≲ ~600KB — negligible vs the app
   bundle; measured at Phase 1.
4. WebDriver runs on this host (CI does it on ubuntu runners); Phase 5 contingency covers failure.
5. `tauri icon` output feeds the existing `bundle.icon` list without tauri.conf changes (files kept
   under the same names) — verified at Phase 1 by `cargo test` + a bundle dry-run if needed.
**Asks**: none open. The bridging-copy decision was presented at the approval gate and resolved by
the owner: option (b), three seams (Naming rule, RESOLVED ASK).

## Delivery
Single branch `worktree-presto-visual-rebrand`; conventional commits with `P1`..`P5` markers; push
for checkpointing only — **no PR is opened by this plan**. At owner flip: (a) one `gh pr create`,
or (b) three stacked PRs (assets+app / playground / landing) cut from the commit ranges via
`gh stack`. The flip is the owner's explicit call and is when CI first runs.

## Post-implementation (self-contained — the implementing session executes THIS section)
1. After Phase 5's gate passes: `/code-review low --fix` on the full diff from the plan baseline
   (`git merge-base origin/main HEAD`). Owner amendment 2026-08-26: the plan's original `max` was
   killed as too token-expensive, and `medium` was killed too (it still fans out finder agents in
   this build) — `low` is the executed level, chosen by the owner from the ladder
   (low/medium/high/max/ultra). Skim the applied fixes, commit them SEPARATELY
   (`chore(review): apply code-review fixes`).
2. Codex audit: `/codex xhigh` with (a) the net diff, (b) a summary of the code-review commits,
   (c) this plan.md + Decision ledger, (d) the adversarial/security ask ("what could go wrong? what
   would an attacker target? what are we trusting that we shouldn't?"), and (e) BOTH rules verbatim:
   - No-over-engineering: "Report bugs and small, targeted improvements only. Do not propose
     speculative abstractions, extra configuration surface, new layers, or rewrites — the smallest
     change that fixes each real problem. If code works and is clear, leave it alone."
   - Comment-quality: "Audit the comments for value per character. Flag any comment that narrates
     what the code visibly does, restates its line, references implementation plans / phases /
     reviews, or spends a paragraph where a sentence works — and flag places where a non-obvious
     invariant or constraint deserves a comment it doesn't have. Comments are permanent context
     every future reader, human or LLM, pays to re-read: they must be few, dense, and exact."
3. Iterative fix loop: verify codex's factual claims against the repo, apply accepted fixes, commit,
   log the round in `lessons/`, RESUME the same codex session with the fix diff for re-review.
   Repeat until a round yields no new material findings; after 3 still-churning rounds, stop and
   surface. Rejected nitpicks are not churn.
4. **Delivery — STOP before PRs.** Wrap-up report: what shipped, contentious decisions with ELI5
   context, preview instructions (`bun run --cwd packages/landing dev`, `bun run --cwd
   packages/playground dev`, `bunx tauri dev` in `packages/accelerator`, OS light+dark), and the two
   flip options. Open PRs ONLY when the owner says so; then `gh pr create` (or `gh stack` per
   ranges) + `gh pr checks --watch`, and update `implementations-plan/index.md`.

## Audit verdicts (inline)
- **Codex round 1** (session `01a03b08-e71e-71c3-8e7d-9305f91d884d`): **reject** — blocking:
  cert-consent copy accuracy, terminal-phase model, incomplete frozen-identity guard,
  non-executable gates. All four addressed in v2 (see ledger); full transcript in
  `audit-codex.md`.
- **Fable round 1**: **conditional approve** — conditions: icns/ico spec correction (→ `tauri
  icon`), per-OS cert copy, slider restyle, old-SVG-masters fate. All four adopted in v2; full
  transcript in `audit-fable.md`.
- **Final fresh-context codex, round 1** (session `01a03b18-d7d5-7350-9cbb-482a053becea`):
  **reject** — blocking: Phase 5 WebDriver gate did not launch/supervise the app; dark-theme
  fallback not executable under the capability model. Plus five non-blocking corrections (tauri
  icon output names, resvg-wasm init, color-mix vs macOS 10.15, contrast-guard scope, tray ledger
  omission). ALL adopted in v2.1 (ledger rows above).
- **Final fresh-context codex, round 2** (same session, v2.1 delta): **reject** — launcher lacked
  display provisioning for this headless host (verified: no DISPLAY, no Xvfb); eval-after-build
  page-load race; contrast-guard phasing; theme-specific static fallbacks. ALL adopted in v2.2.
- **Final fresh-context codex, round 3** (same session, v2.2 delta): **approve** — "all four
  round-2 findings are genuinely resolved… no new security, gate-ordering, reuse, or executability
  blocker." Full transcripts in `audit-codex.md`.

## Seeds (FINAL — approved 2026-08-26: scope unchanged, bridging copy = option b)

/goal All five phases marked ✓ in implementations-plan/presto-visual-rebrand/plan.md (the phase
headers in the file), each ✓ backed by its phase's validation gate as defined in plan.md reported
passing in the transcript (P1: bun install committed + frontend:build + bun run test + accelerator
test:unit + cargo test; P2: frontend:build + bun run test + both cargo test runs + test:e2e:ui with
tauri-identity.test.ts unmodified + dual-theme manual pass recorded in lessons; P3: bun run test +
playground test:e2e + playground build; P4: bun run test + landing test (with typecheck) + landing
build; P5: frontend:build + bun run test + test:e2e:ui + test:e2e:webdriver or its
deferred-with-consent record); for each phase
LESSONS_FILE=implementations-plan/presto-visual-rebrand/lessons/phase-N.md printed in the
transcript; /code-review max --fix complete with fixes committed separately; the codex fix loop
converged (a resumed codex pass reporting no new material findings, quoted in the transcript); NO
pull request opened (pushes to worktree-presto-visual-rebrand only); the wrap-up report with
preview instructions and PR flip options delivered in the transcript; bun run test exits 0 in the
transcript.

/loop 15m Drive implementations-plan/presto-visual-rebrand forward. Never idle waiting for my
input. Each firing: (1) read plan.md + lessons/ (authoritative), git status, git log --oneline -5;
no PRs exist by design — do NOT open one. (2) No task in hand? Pick the next pending step from
plan.md and start it; after each meaningful edit run bun run lint + the touched package's unit
tests; commit per surface with P-N markers; push the branch. (3) Stuck or facing a decision? Call
/codex xhigh with full context, decide together, act, log the consult in lessons/phase-N.md; hard
limits stay hard (no PR, no merge, no publish, no scope beyond plan.md). (4) Same step
failed 5 times? Stop retrying, reassess with codex. (5) Phase gate green (exact commands in
plan.md)? Paste the result, mark ✓ in plan.md, print LESSONS_FILE=..., advance. (6) All phases ✓?
Execute plan.md's Post-implementation section (code-review → codex loop → STOP before PRs → wrap-up
report with preview instructions), then surface and stop.
