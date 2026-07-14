# C10 / F-012 — tauri-trust-boundary — plan (deep tier) — v2 (double audit folded: codex + fable)

## Summary
The Tauri desktop frontend trust boundary is weak: `withGlobalTauri: true` (`tauri.conf.json:11`) exposes
`window.__TAURI__`; there is NO `csp`; the popups (`settings.html`, `authorize.html`, `update-prompt.html`)
run INLINE `<script>` blocks; `tauri-bridge.js:9` = `window.__TAURI__.core`; `build.rs` declares no commands.
And — the load-bearing hole — the 12 custom app commands (`main.rs:487-500`) are invokable from EVERY window.

## CRITICAL reframing — SOURCE-PROVEN (both audit legs + my own read; tauri 2.11.0, the locked version)
The invoke gate `tauri/src/webview/mod.rs:1819-1848` enforces the ACL only when
`plugin_command || has_app_acl_manifest`. `has_app_manifest()`==`has_app_acl` (`ipc/authority.rs:132`) is
FALSE today — `gen/schemas/acl-manifests.json` has an EMPTY app manifest (top keys: autostart/core*/process/
updater only; no app key). So app-local commands skip the ACL → all 12 run from every window; that is exactly
why `default.json` granting none of them still "works". Declaring `AppManifest::commands(&COMMANDS)` in
`build.rs` sets `has_app_acl=true` → thereafter an app command whose calling window has no granting capability
→ `resolve_access`==None (`authority.rs:439-471`, per-window glob) → REJECTED BEFORE dispatch (so a denial
provably cannot run the side effect). **⇒ D5's capability layer is genuine default-deny enforcement, not
theater.** D6 (Rust caller-label) is real belt-and-suspenders + binds request_id↔window-label, which the ACL
cannot express. `withGlobalTauri:false` shrinks JS surface but is NOT the boundary (`__TAURI_INTERNALS__`
remains) — capabilities + label checks are.

**`has_app_acl` is a GLOBAL, ALL-OR-NOTHING switch:** the instant the manifest is declared, ALL 12 commands
become ACL-gated for ALL windows at once. If `COMMANDS` in build.rs omits a command that `generate_handler!`
registers, that command is default-denied from EVERY window (no `allow-X` exists) → silent breakage. ⇒ the
static set-equality test (build.rs `COMMANDS` == `main.rs` handlers == union of capability grants) is a HARD
GATE, not cleanliness; and P3 MUST run the full POSITIVE WebDriver suite (settings/auth/update), which goes red
if any window can't invoke a command it needs.

## Decision ledger (reconciled across the 3 legs + double audit)
- **D1 — withGlobalTauri:false via Bun.build ESM bundles.** Source in `frontend-src/`, `import {invoke} from
  "@tauri-apps/api/core"`; three per-page entries bundled to gitignored `frontend/assets/*.js`. (Reject
  Vite/esbuild/importmap.) Confirmed: `@tauri-apps/api/core` invoke delegates to `__TAURI_INTERNALS__.invoke`,
  present even with withGlobalTauri:false → the flag flip breaks only `window.__TAURI__`, not IPC.
- **D2 — IN-PLACE `frontend/assets/*.js`** (frontendDist STAYS `./frontend`).
- **D3 — CSP (folded).** `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self';
  connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; frame-ancestors 'none';
  form-action 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'`.
  - SOURCE-VERIFIED: `set_csp`→`replace_csp_nonce` (`manager/mod.rs:53-152`) force-adds `'self'` + a per-load
    nonce to **script-src/style-src** for Tauri's own injected (webview-init) scripts → `script-src 'self'`
    (no unsafe-inline) works: Tauri init runs via nonce, our external bundles via `'self'`, injected inline
    scripts blocked. Do NOT set `dangerousDisableAssetCspModification`; NEVER add unsafe-inline/unsafe-eval.
  - CORRECTED rationale (codex MED-4/fable): Tauri augments ONLY script-src/style-src — it does NOT auto-add
    ipc to connect-src. The `ipc: http://ipc.localhost` tokens are REQUIRED and must stay EXPLICIT (Windows
    needs `http://ipc.localhost`, Linux/mac need `ipc:` — both). connect-src excludes `'self'` (popups never
    fetch); that closes fetch/XHR/WS exfil but NOT navigation/form — hence the added `form-action/frame-src/
    child-src/worker-src 'none'` + the Rust navigation guards below. On Linux CSP is `<meta>`-delivered where
    `frame-ancestors` is IGNORED → the navigation/new-window guards are the real anti-framing/exfil control.
- **D3b — Rust navigation guards (NEW, codex HIGH-3).** In `windows.rs` window creation add `on_navigation`
  (allow ONLY the local Tauri asset origin `tauri://localhost` / `http://tauri.localhost`) + deny new webviews
  (`on_new_window` equivalent / `WebviewWindowBuilder` default). WebDriver-test that remote navigation + form
  submission are blocked. Stop claiming CSP alone stops all exfil.
- **D4 — externalize inline scripts + the ONE markup style.** Move each inline `<script>` → `frontend-src/*.js`
  (+ shared module replacing tauri-bridge.js). Replace `style="display:none"` (settings.html:33) → `hidden`.
  CORRECTED justification (fable): the runtime `.style.setProperty("--fill")`/`.style.display`
  (settings.html:96,112,129,132) are CSSOM mutations that `style-src` does NOT block — externalizing them via a
  `[data-fill="N"]{--fill:N%}` rule set (0-4 → 0/25/50/75/100%) is harmless equivalence, NOT a CSP requirement.
  Verified: no `<style>` blocks in any page; inline SVG has only `d` (no presentation attr) → nothing else to do.
- **D5 — per-window capabilities + explicit list + deny-all main + build.rs manifest.** Delete `default.json`;
  create `authorize.json` (`windows:["auth-*"]`: allow-get-verified-info + allow-respond-auth),
  `settings.json` (`["settings"]`: the 9 settings commands), `update-prompt.json` (`["update-prompt"]`:
  allow-respond-update-prompt), `main.json` (`["main"]`: [] reserved deny-all). List all four in
  `app.security.capabilities`. `build.rs`: `try_build` + `AppManifest::commands(&COMMANDS)` (12 commands);
  fail the build if a generated bundle is missing OR STALE (see D9). Build the per-window (window,command)
  usage matrix explicitly before writing grants (settings→9, auth-*→get_verified_info+respond_auth,
  update-prompt→respond_update_prompt; verified per window against the externalized `frontend-src/*.js`).
- **D6 — Rust caller-label checks (PRIMARY DiD).** Add a `WebviewWindow`/`Window` param + a PURE predicate
  `require_label(actual: &str, expected: &str) -> Result<(),String>` placed in the LIBRARY module `commands.rs`
  (NOT `windows.rs`, which is bin-only — codex MED-5/fable: a lib command cannot import from the bin;
  `windows.rs` already imports `commands::sanitize_window_label`). Mapping (explicit 12-command table):
  respond_auth ⇒ `auth-{label(request_id)}`; respond_update_prompt ⇒ `update-prompt`; the 9 settings
  commands (INCLUDING the getters get_config/get_autostart_enabled/get_system_info — Rust is a complete
  independent layer) ⇒ `settings`; get_verified_info ⇒ `auth-*` (12-hex). Wrap infallible getters in
  `Result<_,String>` (transparent to JS/mock; VERIFIED no internal Rust callers — `main.rs mod tests` calls no
  command fn directly). Err+warn (generic message) on mismatch. Unit-test ONLY the pure predicate + the mapping
  table; command-level ordering (validate-before-side-effect) is proven by the WebDriver integration test, not
  claimed as pure unit coverage. SOUND: JS cannot spoof its window label (Tauri resolves Window from the native
  InvokeMessage); deriving the expected auth label from request_id defeats an attacker in auth-<A> passing
  request_id=B (hash(B)≠A). Auth label WIDENED to ≥16 SHA-256 bytes (128-bit) — the current 6-byte/48-bit
  truncation (commands.rs:158-165) isn't collision-safe (codex MED-6); keep hashing (handles arbitrary
  request_id charset), compare the exact full label; `auth-*` glob still matches.
- **D7 — RESOLVED: DROP core:default + ALL plugin grants** (codex HIGH-1; adjudicated over fable's retain).
  Rationale: empty capability selectors match NO window (`capability.rs:162-163` + `authority.rs:459-460`,
  source-verified) → today's core:default/autostart/process grants are INERT; retaining core:default in the new
  per-window capabilities would NEWLY ACTIVATE a broad core API (event emit, window/webview enumeration, path,
  image/rgba, resource close, menu/tray) — a privilege INCREASE, not the status quo. Both legs agree the
  frontend uses no core/event/window JS API (popups close from RUST — commands.rs:153,293; no listeners). ⇒
  each capability grants ONLY its window's app commands; add back a minimal specific perm (e.g.
  core:window:allow-close) to the exact window ONLY if the positive WebDriver suite proves it needed.
- **D8 — RESOLVED: `built-debug` for the trust-boundary spec + static drift guards** (fable's decisive trace,
  adjudicated over codex's "optional"). `is_dev()==!cfg!(feature="custom-protocol")` (`lib.rs:308`); `tauri dev`
  AND the existing `release` webdriver mode (bare `cargo build --release`, which does NOT enable
  `tauri/custom-protocol` — a CLI-only feature) BOTH run `is_dev()==true`; `windows-build` does a real `tauri
  build` but runs no specs. ⇒ NO current CI job exercises the shipped custom-protocol path. Resolution (both
  legs combined): (a) add a `built-debug` mode to `_e2e-webdriver.yml` (`tauri build --debug --no-bundle
  --features webdriver` → launch `target/debug/aztec-accelerator`), run the trust-boundary spec under it on
  Linux (bounds 3-OS cost); (b) keep the 3-OS `mode: dev` matrix for the positive settings/auth specs + the
  P3 ACL-isolation proof (capabilities are compile-time → enforced in dev, and dev applies the real csp via
  `dev_csp.or(csp)` with no devUrl — `manager/mod.rs:353-380`); (c) STATIC test forbids `devUrl` and any
  `devCsp` weaker than `csp` (drift guard — a future devUrl/dev_csp silently falsifies the dev CSP gate);
  (d) assert the DEBUG-form ACL denial reason (see validation).

## Validation (CI-authoritative; GUI-less VPS ⇒ HARD RULE)
- **Static** `scripts/tauri-trust-boundary.test.ts` (`bun test scripts/`): withGlobalTauri:false; the EXACT CSP
  directive set (incl. form-action/frame-src/child-src/worker-src/object-src/base-uri 'none'; no unsafe-*);
  `devCsp` unset (or == csp) AND no `devUrl` (drift guard); only the 4 named capabilities; the EXACT per-window
  (window→command) matrix — NOT just union equality (codex HIGH-2: commands can be swapped while preserving the
  union); build.rs COMMANDS == main.rs handlers == union of grants (set-equality HARD gate); frontend HTML has
  0 inline `<script>`/`<style>`/`on*=`/markup-`style=` + one module script/page; no `window.__TAURI__` in
  frontend-src; emitted bundles scanned for dynamic-import/remote-URL/eval/source-map refs (supply-chain).
- **Playwright mock** (`desktop-ui`): tauri-mock → `window.__TAURI_INTERNALS__.invoke`; assert `window.__TAURI__`
  undefined; build bundles before serve; existing specs pass.
- **WebDriver** `trust-boundary.spec.ts` — the F-012 proof. Invoke ONLY via the injected
  `__TAURI_INTERNALS__.invoke`/bundled core (they carry `__TAURI_INVOKE_KEY__`; a hand-rolled postMessage is
  SILENTLY DROPPED before the ACL — webview/mod.rs:1758 — and would test the wrong boundary):
  - `window.__TAURI__` absent + a real settings command resolves (positive).
  - Inline `<script>`/`<style>`/`fetch('https://…')` each fire `securitypolicyviolation`; no violation on a
    normal allowed invoke; `eval` throws; remote NAVIGATION + form submission are blocked (D3b).
  - **Cross-window denial (isolated ACL proof):** from `settings`/`auth-*`, first invoke an ALLOWED command and
    require SUCCESS (proves the primitive+window), then a FORBIDDEN command → return explicit `{resolved,error}`
    sentinel; FAIL if the primitive is absent or it resolves; assert the rejection reason is the DEBUG-form
    ACL/permission denial ("...not allowed on... window/webview/URL context...", authority.rs:229 — NOT the
    release "Command X not allowed by ACL" string, which never appears under debug_assertions); prove no state
    changed via a follow-up allowed read from another window (canary); cleanup in `finally` (no 60s hang).
  - This ACL-isolation assertion runs at the P3 gate with ZERO Rust label code present (the only moment the ACL
    is provable alone); asserting the ACL-specific reason keeps it meaningful in ongoing CI (a label can't mask
    a broken ACL). A P3 failure here empirically FALSIFIES the central inference → fall back to labels-as-sole-
    enforcement + capabilities documented DiD-only.
  - Add a webdriver-only `update-prompt` trigger: `show_update_prompt_window` is compiled OUT under `webdriver`
    (windows.rs:130) → today no real update-prompt flow exists → add a gated trigger so respond_update_prompt is
    positively tested from the real `update-prompt` label.
  - Existing settings/auth specs stay green (positive proof the capability matrix is complete).
- **Local** (VPS): `bun run frontend:build` + `bun test scripts/` (static) + `bun run lint` +
  `cargo fmt`/`clippy -D warnings`/`test` (the pure predicate + mapping unit tests). Tauri-GUI build +
  WebDriver/Playwright ⇒ CI per the HARD RULE.

## Phases (each with its gate; monotonic — never less secure mid-way)
- **P0 — deps:** add `@tauri-apps/api` devDep pinned to an EXACT 2.x compatible with tauri 2.11.0 (invoke_key
  protocol compat) + `bun.lock`; review the lock diff; honor 7-day min-age. Gate: `bun install
  --frozen-lockfile` + lint.
- **P1 — build + externalize (flags UNCHANGED):** frontend-src modules; `frontend:build` (Bun.build) that
  CLEANS `frontend/assets/` before emitting; before(Dev/Build)Command; mock→internals; externalize scripts +
  the one markup style + the CSSOM `[data-fill]` rules; delete tauri-bridge.js + inline blocks; gitignore
  `frontend/assets/`; build.rs missing-OR-stale-bundle guard; ensure EVERY direct Cargo CI path runs
  frontend:build. Gate: 3 bundles emitted; Playwright mock + static externalization/scan tests green; WebDriver
  green (works with withGlobalTauri still true — internals always present).
- **P2 — flip:** `withGlobalTauri:false` + the strict CSP (D3) + D3b navigation guards. Gate: WebDriver
  (IPC + module-load + CSP negatives incl. navigation/form + no-violation-on-allowed) green under `mode: dev`;
  static CSP + devUrl/devCsp drift test green.
- **P3 — capabilities + build.rs manifest (the ACL flip):** command declaration; per-label capability files +
  explicit list + deny-all main; commit regenerated `gen/schemas/*`. Gate: cargo build/clippy/test; the FULL
  positive WebDriver suite (settings/auth/update — all-or-nothing completeness guard); the isolated cross-
  window-denial proof (ACL-reason, canary, NO label code yet); the new `built-debug` Linux lane runs the
  trust-boundary spec against a custom-protocol binary; static exact-matrix + set-equality green.
- **P4 — Rust caller-label DiD:** WebviewWindow params + the pure `require_label` predicate in commands.rs +
  the 12-command mapping table + widened auth label; unit tests for the predicate/mapping. Gate: cargo test +
  WebDriver positive flows + cross-window negative still rejects (now by ACL AND label).
- **P5 — lock:** static tests run in `lint`/`test:unit`; full `accelerator-status` green (3-OS dev matrix +
  the built-debug Linux trust-boundary lane).

## Security & Adversarial Considerations
- **Threat closed:** a compromised/hostile page in one window invoking another window's privileged command
  (auth popup → enable_safari_support/remove_approved_origin; any window → arbitrary command) + injected-script/
  eval/off-origin-exfil (fetch AND navigation/form). Closed by (a) per-window capabilities (source-proven real
  enforcement), (b) Rust caller-label assertions (primary DiD, unspoofable label), (c) strict CSP + Rust
  navigation guards; validated by the isolated cross-window-denial + CSP/navigation negatives.
- **Least privilege:** DROP core:default + all plugin grants (D7); each window gets only its app commands.
- **Preserves prior findings:** F-004 (no updater:default — only respond_update_prompt), F-010 (no
  autostart:allow-enable), SEC-06/F-014 (respond_auth id↔label binding STRENGTHENED + widened).
- **Supply chain:** exact-pin `@tauri-apps/api` + frozen lockfile + min-age; clean-before-build + stale-bundle
  guard + emitted-bundle scan (gitignored bundles are trusted by script-src 'self').
- **Residual:** the webview engine + Tauri IPC remain trusted (CSP can't stop a native bug); capability/CSP
  DRIFT (a future devUrl/dev_csp/new command missing from COMMANDS/new main window) — the static exact-matrix +
  set-equality + devUrl/devCsp + CSP-token tests are the change-detectors; ensure they run in lint/test:unit.

## Assumptions
### Facts (verified; ✚ = source-confirmed this pass, tauri 2.11.0 / tauri-utils 2.9.2)
- withGlobalTauri:true (:11); no csp; frontendDist ./frontend (:7); windows:[] (:10, no static main window;
  labels settings/auth-<hash>/update-prompt in windows.rs). Inline scripts authorize:38-71/settings:59-176/
  update-prompt:24-44; tauri-bridge.js:9=global; main.rs:487-500 = 12 commands, none read a caller label.
- ✚ App manifest is EMPTY today (acl-manifests.json app keys: none) → app commands skip the ACL
  (webview/mod.rs:1819-1848); declaring AppManifest flips to default-deny per window.
- ✚ `default.json` grants core/autostart/process but with NO window/webview selectors → matches NO window →
  INERT (capability.rs:162-163; authority.rs:459-460). [Corrects v1's "grants all windows".]
- ✚ Tauri augments ONLY script-src/style-src (nonce/hash, force-adds 'self'); connect-src is passed through
  untouched (manager/mod.rs:53-152).
- ✚ csp injected on dev when devCsp unset (dev_csp.or(csp)); is_dev()==!custom-protocol (lib.rs:308) → dev AND
  bare-cargo release both is_dev()==true; only `tauri build` flips it.
- ✚ `main.rs mod tests` calls no command fn directly → Result-wrapping getters is zero-breakage.
- build.rs is NOT bare — it validates AZTEC_VERSION + verified-sites.json; only the tauri build invocation is
  bare (build.rs:21). CI: desktop-ui (Playwright mock) + e2e-webdriver (dev, 3 OS) + windows-build (real tauri
  build, no specs). "WebDriver passes" shows the commands are CALLABLE, not that all 12 are exercised.
### Inferences (verify in impl)
- Generated perm ids = `allow-<kebab-command>`; adjust to the first cargo build's output.
- `windows:["auth-*"]` glob matches `auth-<hash>`; confirm via the WebDriver popup test.
- WebKitGTK renders under style-src 'self' (traced, not executed) — the computed-style WebDriver check confirms.
### Asks (resolved; final codex pass may probe)
- A1 (D7): RESOLVED → DROP (least privilege; add-back-minimal-if-proven).
- A2 (D8): RESOLVED → built-debug for the trust-boundary spec + dev matrix + drift guards. Only the CI-cost
  tradeoff (built-debug on Linux vs all 3 OS) is worth surfacing.
- A3 (freezePrototype): optional DiD; if enabled, test under the built-debug custom-protocol lane.
- A4 (unsafe-inline fallback): CLOSED to `'self'` HARD — nothing left to justify it after D4.

## Seeds (draft)
- `/goal`: F-012 fixed — withGlobalTauri:false + Bun ESM bundles + externalized scripts/styles + strict CSP
  (+ form-action/navigation guards) + per-window capabilities + build.rs AppManifest + Rust caller-label
  checks; static + Playwright-mock + WebDriver (isolated cross-window-denial w/ ACL-reason + canary,
  CSP/navigation negatives, built-debug custom-protocol lane) green in CI; post-impl codex xhigh folded; PR
  into security-hardening CI green.
- `/loop 15m`: drive C10 by phase (P0 deps → P1 build+externalize → P2 CSP+nav flip → P3 caps+build.rs
  manifest [ACL flip: full positive suite + isolated denial proof] → P4 Rust label DiD → P5 lock). Static+lint
  locally each phase; Tauri-GUI/WebDriver in CI. Commit/push per phase.
