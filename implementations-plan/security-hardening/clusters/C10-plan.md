# C10 / F-012 — tauri-trust-boundary — plan (deep tier) — MAIN-AGENT leg (of 3 parallel plans)

## Summary
The Tauri desktop app's frontend trust boundary is weak: `withGlobalTauri: true` (`tauri.conf.json:11`)
exposes `window.__TAURI__` to every window; there is NO `csp`; the popups (`settings.html:59`,
`authorize.html:38`, `update-prompt.html:24`) run INLINE `<script>` blocks (incompatible with a strict CSP);
`tauri-bridge.js:9` pulls `invoke` off the global; a SINGLE `capabilities/default.json` grants the same
permission set to all windows (no per-label split). A compromised renderer (XSS in a popup, a malicious
recognized-name, etc.) can reach the full IPC surface.

Fix (master F-012): externalize inline scripts/styles; `withGlobalTauri: false` + bundle `@tauri-apps/api`
imports; strict CSP with only the documented Tauri IPC connect sources; split capabilities by window label;
keep the Rust-side caller-label command checks as defense-in-depth.

## Key complexity (drives the deep tier)
- **The frontend is BUILDLESS** — plain HTML files loaded directly by the webview; `tauri-bridge.js` uses
  the `window.__TAURI__` global. `withGlobalTauri: false` removes that global, so each window's JS must
  IMPORT from `@tauri-apps/api` — which requires either (a) a small bundling/copy step for the src-tauri
  frontend (esbuild/bun build of the api + the per-window scripts into ES modules the webview loads), or
  (b) Tauri's ESM `@tauri-apps/api` served as assets. This is a build-system change, not just config.
- **CSP + externalization interact**: a strict `script-src 'self'` (no `unsafe-inline`) REQUIRES every
  inline `<script>` moved to a `.js` file first; and `connect-src` must enumerate exactly the IPC origin(s)
  (`ipc:` / `http://ipc.localhost` on Windows) the app uses — too tight breaks IPC, too loose is useless.
- **Per-window capabilities**: authorize / settings / update-prompt / main each need only their own
  commands. Splitting `default.json` into per-label capability files + `build.rs` command declaration.

## Design (main-leg draft — to be reconciled with codex + fable legs)
1. **Frontend build step**: add a minimal `bun build`/esbuild step (in the accelerator prebuild or a new
   `build:frontend`) that (a) externalizes each popup's inline `<script>` into `authorize.js` / `settings.js`
   / `update-prompt.js`, (b) bundles `@tauri-apps/api` imports (replacing `window.__TAURI__`), emitting ES
   modules the webview loads via `<script type="module" src="...">`. `tauri-bridge.js` → import `invoke`
   from `@tauri-apps/api/core`.
2. **`withGlobalTauri: false`** in tauri.conf.json.
3. **Strict CSP** in tauri.conf.json `security.csp`: `default-src 'self'`; `script-src 'self'`;
   `style-src 'self'` (+ externalize any inline styles); `connect-src 'self' ipc: http://ipc.localhost`;
   `img-src 'self' data:` as needed; `object-src 'none'`; `base-uri 'none'`; `frame-ancestors 'none'`.
4. **Per-window capabilities**: split `default.json` into `authorize.json` / `settings.json` /
   `update-prompt.json` / `main.json`, each scoped to its window label + only its commands (e.g. authorize:
   respond_auth + get_verified_info; settings: the config/toggle commands minus autostart:allow-enable per
   C8). Declare custom commands in `build.rs`. Keep the Rust-side caller-label checks (defense-in-depth).

## Validation gate (per phase)
- CSP/config: `cargo build` the app (CI, GUI) + a WebDriver E2E that each popup still loads + IPC works +
  the CSP header is present + no inline-script CSP violation. `bun run lint:actions` if workflows touched.
- Frontend build: `bun run` the new build step + the Playwright mock UI tests (CI) still green.
- Capabilities: `cargo build` (capability schema regen committed) + WebDriver flow per window.
- Local caveat (HARD RULE): Tauri-GUI build + WebDriver/Playwright run in CI (this VPS: no Tauri GUI run +
  Playwright unsupported on the host OS). Local = `cargo fmt`/`clippy`/build-check where possible + lint.

## Security & Adversarial Considerations
- **Threat:** a compromised renderer (XSS via a malicious origin/recognized-name, a supply-chain'd asset)
  reaching the full IPC command surface + exfiltrating via arbitrary connect. Closed by: strict CSP
  (no inline script, tight connect-src), no global Tauri handle, per-window least-capability, Rust caller
  checks.
- **Residual:** the webview engine + Tauri IPC itself remain trusted; CSP can't stop a native-side bug. The
  recognized-name/badge path must stay `textContent` (no innerHTML) — verify no regression.

## Assumptions
### Facts (verified)
- `withGlobalTauri: true` (`tauri.conf.json:11`); no `csp` key; inline `<script>` in settings/authorize/
  update-prompt HTML; `tauri-bridge.js:9` = `window.__TAURI__.core`; single `capabilities/default.json`
  (no per-window split); `build.rs` = bare `tauri_build::build()`.
### Inferences (verify in impl)
- The buildless frontend needs a bundling step for `withGlobalTauri:false` (the parallel legs must confirm
  the lightest correct approach — bun build vs Tauri ESM assets).
- The exact `connect-src` IPC origin differs per platform (`ipc:` scheme vs `http://ipc.localhost`).
### Asks (surface in consolidation)
- A1: add a frontend build step vs a buildless alternative — reconcile across the 3 legs.
- A2: CSP strictness (exact directives) — reconcile.
- A3: capability split granularity (per-window vs per-command).
