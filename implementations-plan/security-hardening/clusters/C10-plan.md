# C10 / F-012 — tauri-trust-boundary — plan (deep tier) — CONSOLIDATED (main + fable + codex legs)

## Summary
The Tauri desktop frontend trust boundary is weak: `withGlobalTauri: true` (`tauri.conf.json:11`) exposes
`window.__TAURI__`; there is NO `csp`; the popups (`settings.html:59`, `authorize.html:38`,
`update-prompt.html:24`) run INLINE `<script>` blocks; `tauri-bridge.js:9` = `window.__TAURI__.core`; a
single `capabilities/default.json` grants all windows the same set; `build.rs` is bare.

## CRITICAL reframing (both legs independently verified — the load-bearing finding)
There are NO Rust caller-label checks today, AND `default.json` grants NONE of the 12 custom app commands —
yet they all work (WebDriver passes). ⇒ **Tauri v2 does NOT gate app-local commands by default**: every
window (incl. an `auth-*` popup) can already invoke EVERY command (`enable_safari_support`,
`remove_approved_origin`, `respond_update_prompt`). THAT ungated command surface IS the trust-boundary hole —
NOT closed by CSP or `withGlobalTauri`. The master's "keep the Rust caller-label checks as DiD" assumes
checks that don't exist ⇒ this plan ADDS them as PRIMARY enforcement; the per-window capability split +
`build.rs` command declaration is the Tauri-native complementary layer, and a cross-window-denial WebDriver
test is THE proof F-012 is fixed (it fails loudly only if the boundary is actually open).

## Decision ledger (reconciled across the 3 legs)
- **D1 — withGlobalTauri:false via Bun.build ESM bundles** (both legs; reject Vite/esbuild/the undocumented
  `__TAURI_INTERNALS__` shim/importmap). Source in `frontend-src/`, import `{invoke} from
  "@tauri-apps/api/core"`; three per-page entries bundled to gitignored `frontend/assets/*.js`.
- **D2 — layout: codex's IN-PLACE `frontend/assets/*.js`** (frontendDist STAYS `./frontend`) over fable's
  `dist/` (which needs HTML/CSS copy machinery). Lighter; no frontendDist change.
- **D3 — CSP (codex's tighter form):** `default-src 'self'; script-src 'self'; style-src 'self';
  img-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none';
  frame-ancestors 'none'`. connect-src EXCLUDES `'self'` (the popups never `fetch()` — deny arbitrary
  same-origin exfil); `base-uri 'none'`. Tauri auto-augments script-src (bootstrap nonce) + connect-src
  (ipc), so `'self'`/ipc work — proven by a green WebDriver IPC run + a no-`securitypolicyviolation` assert.
  NO `unsafe-inline`/`unsafe-eval`; do NOT set `dangerousDisableAssetCspModification`.
- **D4 — externalize inline scripts + ALL inline styling.** Move each inline `<script>` → `frontend-src/
  {authorize,settings,update-prompt}.js` (+ `tauri-bridge.js`→ a shared module). Replace the one markup
  `style="display:none"` (settings.html:33) with `hidden`, and the runtime `.style.display`/
  `.style.setProperty('--fill')` (settings.html:96,112,129,132) with `element.hidden` + a
  `[data-fill="N"]{--fill:N%}` CSS rule set — so `style-src 'self'` needs no `unsafe-inline`.
- **D5 — per-window capabilities + explicit list + deny-all main (codex).** Delete `default.json`; create
  `authorize.json` (`windows:["auth-*"]`: allow-get-verified-info + allow-respond-auth), `settings.json`
  (`["settings"]`: the 9 settings commands), `update-prompt.json` (`["update-prompt"]`:
  allow-respond-update-prompt), `main.json` (`["main"]`: [] deny-all, reserved). List all four in
  `app.security.capabilities`. `build.rs`: `try_build` + `AppManifest::commands(&COMMANDS)` (the 12
  commands from `main.rs:487-500`) so per-command `allow-<cmd>` perms generate + ungranted windows are
  denied; fail the build if a generated bundle is missing.
- **D6 — Rust caller-label checks (PRIMARY, added).** Add `WebviewWindow`/`Window` param + a
  `require_label` helper (centralized in `windows.rs`): respond_auth ⇒ label ==
  `auth-{sanitize_window_label(request_id)}` (strengthens SEC-06); respond_update_prompt ⇒ `update-prompt`;
  settings mutators ⇒ `settings`; get_verified_info ⇒ `auth-`+12-hex. Wrap infallible getters in
  `Result<_,String>`; Err+warn (generic message) on mismatch. Pure unit tests for the matcher + each group.
- **D7 — core:default retention (OPEN → default: RETAIN, tighten in impl):** fable retains `core:default`
  (window/event core the popup may need to close itself/emit); codex drops it. DEFAULT: retain `core:default`
  per-window unless the WebDriver run proves it unused; drop the plugin grants (`autostart:*` except where
  the settings toggle needs `autostart:allow-disable`/`allow-is-enabled`; `process:default`). Verify empirically.
- **D8 — WebDriver mode (OPEN → default: keep `tauri dev`):** codex proposes `built-debug`
  (`tauri build --debug --no-bundle`) for a real-build gate; fable keeps `tauri dev` (which already runs the
  real webview + `beforeDevCommand` builds the bundles). DEFAULT: keep `tauri dev` (lighter CI change); the
  double audit decides whether `built-debug` is worth the workflow churn. The `windows-build` job already
  does a real `tauri build`.

## Validation (CI-authoritative; GUI-less VPS ⇒ HARD RULE)
- **Static** `scripts/tauri-trust-boundary.test.ts` (`bun test scripts/`): withGlobalTauri:false; exact CSP
  directives (no unsafe-*); only the 4 named capabilities; capability perms == build.rs commands ==
  main.rs handlers (set-equality); authorize.json EXCLUDES settings commands (least-privilege drift guard);
  frontend HTML has 0 inline `<script>`/`<style>`/`on*=`/markup-`style=` + exactly one module script; no
  `window.__TAURI__` reference in frontend-src.
- **Playwright mock** (`desktop-ui`): tauri-mock → `window.__TAURI_INTERNALS__.invoke` (the bundled
  `@tauri-apps/api/core` invoke delegates to it); assert `window.__TAURI__` undefined; build bundles before
  serve; existing specs pass.
- **WebDriver** (`e2e-webdriver`, real CSP+IPC, 3 OS) `trust-boundary.spec.ts`: `window.__TAURI__` absent +
  a real settings command resolves; inline `<script>`/`<style>`/`fetch('https://…')` each blocked (a
  `securitypolicyviolation` fires) + no violation on a normal allowed invoke; `eval` throws; **cross-window
  denial** — from `settings`/`auth-*`, invoking another window's command REJECTS (passes whether the ACL or
  the Rust label denies). Existing settings/auth-flow specs stay green (positive proof).
- **Local** (VPS): `bun run frontend:build` + `bun run test:unit` (static) + `bun run lint` +
  `cargo fmt`/`clippy -D warnings`/`test` (the matcher unit tests). Tauri-GUI build + WebDriver/Playwright ⇒ CI.

## Phases (each with its gate)
- **P0 — deps:** add `@tauri-apps/api` devDep + `bun.lock`. Gate: `bun install --frozen-lockfile` + lint.
- **P1 — build + externalize (flags UNCHANGED):** frontend-src modules, `frontend:build` script,
  before(Dev/Build)Command, mock→internals, externalize scripts+styles, delete tauri-bridge.js + inline
  blocks, gitignore assets, build.rs missing-bundle guard. Gate: `frontend:build` emits 3 bundles;
  Playwright mock + static externalization test green; WebDriver green (works with withGlobalTauri still true
  — internals is always present).
- **P2 — flip:** `withGlobalTauri:false` + the strict CSP. Gate: WebDriver (IPC + module-load + CSP negatives
  incl. no-violation-on-allowed) green; static CSP test green.
- **P3 — capabilities + build.rs:** command declaration; per-label capability files + explicit list +
  deny-all main; commit regenerated `gen/schemas/*`. Gate: cargo build/clippy/test + `windows-build` real
  `tauri build` + WebDriver cross-window-denial + static capability set-equality green.
- **P4 — Rust caller-label DiD:** params + `require_label` + unit tests. Gate: cargo test (matcher) +
  WebDriver positive flows + cross-window negative reject.
- **P5 — lock:** ensure the static tests run in `lint`/`test:unit`; full `accelerator-status` green 3-OS.

## Security & Adversarial Considerations
- **Threat closed:** a compromised/hostile page in one window invoking another window's privileged command
  (auth popup → `enable_safari_support`/`remove_approved_origin`; any window → arbitrary command) +
  injected-script/eval/off-origin-exfil. Closed by (a) per-window capabilities, (b) Rust caller-label
  assertions (guaranteed — the primary layer), (c) strict CSP, validated by the cross-window-denial test.
- `withGlobalTauri:false` shrinks the JS attack surface but is NOT the enforcement boundary
  (`__TAURI_INTERNALS__` remains) — capabilities + label checks are. Plan does not over-rely on it.
- **Preserves prior findings:** F-004 (no `updater:default`/process to the update prompt — only
  respond_update_prompt), F-010 (no `autostart:allow-enable`), SEC-06/F-014 (respond_auth id/origin binding
  is STRENGTHENED by the label check). No `data:`/remote in img/connect.
- **Residual:** the webview engine + Tauri IPC remain trusted; CSP can't stop a native bug. Any new
  frontend network/img/wasm/inline-style/window-creation need requires a security review (the static CSP +
  capability set-equality tests are the change-detectors).

## Assumptions
### Facts (verified, both legs, file:line)
- `withGlobalTauri:true` (`:11`); no csp; `frontendDist:"./frontend"` (`:7`); `windows:[]` (`:10`) — NO
  static main window (labels: `settings`/`auth-<hash>`/`update-prompt` in `windows.rs`). Inline scripts
  (authorize:38-71, settings:59-176, update-prompt:24-44); tauri-bridge.js:9 = global; `main.rs:487-500`
  registers 12 commands, none read a caller label; `default.json` grants none of them yet all work;
  `build.rs` bare; `@tauri-apps/api` NOT a dep; Bun 1.3.14 toolchain; CI has desktop-ui (Playwright mock,
  no CSP) + e2e-webdriver (real webview, 3 OS) + windows-build (real `tauri build`).
### Inferences (verify in impl — highest-uncertainty first)
- `AppManifest::commands()` makes ungranted windows DENIED (not just grantable) — the crux; mitigated by
  the Rust label layer + the "rejected by either" negative test; verify vs regenerated acl-manifests.json.
- Generated perm ids = `allow-<kebab-command>`; adjust to what the first `cargo build` emits.
- Tauri auto-augments script-src (nonce) + connect-src (ipc/http://ipc.localhost) → `'self'`/ipc work.
- `@tauri-apps/api/core` invoke delegates to `window.__TAURI_INTERNALS__.invoke` (mock target).
- capability `windows:["auth-*"]` glob matches every `auth-<hash>`; verify via the WebDriver popup test.
### Asks (open reconciliation — decided by defaults above; double audit may override)
- A1 (D7): retain `core:default` per-window (vs drop) — default RETAIN, tighten if WebDriver proves unused.
- A2 (D8): `tauri dev` (vs `built-debug`) WebDriver mode — default `tauri dev`.
- A3: `freezePrototype:true` extra DiD — optional.
- A4: `style-src 'self'` vs an `unsafe-inline` fallback if WebKitGTK styling breaks — default `'self'`.

## Seeds (draft)
- `/goal`: F-012 fixed — withGlobalTauri:false + Bun ESM bundles + externalized scripts/styles + strict CSP +
  per-window capabilities + build.rs command declaration + Rust caller-label checks; static + Playwright-mock
  + WebDriver (incl. cross-window-denial + CSP-negative) gates green in CI; post-impl codex xhigh folded;
  PR into security-hardening CI green.
- `/loop 15m`: drive C10 by phase (P0 deps → P1 build+externalize → P2 CSP flip → P3 caps+build.rs → P4
  Rust label DiD → P5 lock). After each phase run the static+lint gates locally; Tauri-GUI/WebDriver in CI.
  Commit/push per phase; consult codex on the ACL-gating semantics + WebDriver mode.
