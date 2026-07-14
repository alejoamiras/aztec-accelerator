# C10 / F-012 — tauri-trust-boundary — lessons

## GATE 1 — source-level verification of the plan's load-bearing inferences (2026-07-14)

Verified independently against the EXACT crate versions in `packages/accelerator/src-tauri/Cargo.lock`:
`tauri 2.11.0`, `tauri-utils 2.9.2`, `tauri-build 2.6.2`. Vendored source under
`~/.cargo/registry/src/index.crates.io-*/`. These confirmations were produced BEFORE the codex+fable
double audit returned, so the audits serve as an adversarial cross-check.

### 1. CENTRAL INFERENCE — TRUE (high confidence). The capability layer is REAL enforcement, not theater.
`tauri-2.11.0/src/webview/mod.rs:1819-1848` (the IPC invoke path):
```rust
// we only check ACL on plugin commands or if the app defined its ACL manifest
if (plugin_command.is_some() || has_app_acl_manifest)
   && request.cmd != FETCH_CHANNEL_DATA_COMMAND
   && invoke.acl.is_none()
{ /* reject: "Command {} not allowed by ACL"; */ return; }
```
- `has_app_acl_manifest` = `runtime_authority.has_app_manifest()` = `resolved_acl.has_app_acl`
  (`ipc/authority.rs:132`, `:120`), computed at BUILD time by the ACL resolver from whether the app
  declared commands via `AppManifest` in build.rs.
- **Today** (bare `tauri_build::build()`, no AppManifest → `has_app_acl=false`): for an APP-LOCAL command
  (not `plugin:`-prefixed), `(false || false)` = false → the ACL branch is SKIPPED → the command runs
  regardless of capabilities. **This is exactly why all 12 custom commands work ungranted today.**
- **After `AppManifest::commands(&COMMANDS)`** → `has_app_acl=true`: `(false || true)` = true, and if
  `resolve_access` returned `None` (no capability grants this command to the calling window), it REJECTS.
- `resolve_access` (`ipc/authority.rs:439-471`) denies when the command is absent from `allowed_commands`
  for the window, filtering by `cmd.windows.iter().any(|w| w.matches(window))` — i.e. per-window glob match.
  So `windows:["auth-*"]` etc. genuinely scopes a command to matching window labels.
- **Conclusion:** declaring the AppManifest FLIPS app-local commands from default-ALLOW to ACL-gated
  (default-DENY unless a capability grants (window,command)). D5 is real enforcement; the cross-window
  denial WebDriver test will prove it. D6 (Rust caller-label) is genuine belt-and-suspenders + adds the
  request_id↔label binding the ACL cannot express.

**NEW IMPLEMENTATION RISK surfaced by this (fold into plan): `has_app_acl` is ALL-OR-NOTHING.** The instant
the manifest is declared, EVERY app-local command becomes ACL-gated for EVERY window. So the per-window
capability files must COMPLETELY cover the real production (window,command) usage matrix — miss one grant
and that flow silently breaks at runtime. The static set-equality test (perms==build.rs==handlers) checks
TOTALS, not per-window assignment. The real guard is the EXISTING positive-flow specs (settings.spec.ts,
auth-flow.spec.ts, update prompt) — they go red if a window can't invoke a command it needs. P3's gate MUST
run the full positive WebDriver suite, not just the new negative test. Build the usage matrix explicitly:
  - `settings` window → get_config, get_autostart_enabled, set_autostart, set_speed, remove_approved_origin,
    get_system_info, enable_safari_support, disable_safari_support, set_auto_update  (verify against settings.js)
  - `auth-*` window → get_verified_info, respond_auth
  - `update-prompt` window → respond_update_prompt
  (12 commands total = the generate_handler! set; confirm each window's real usage before writing grants.)

### 2. D3 CSP — SOUND (high confidence). `script-src 'self'` does NOT break Tauri's IPC bootstrap.
`tauri-2.11.0/src/manager/mod.rs:53-150` (`set_csp` → `replace_csp_nonce`):
- Tauri injects its own initialization/IPC bootstrap inline `<script>`/`<style>` with a per-response random
  `nonce`, and APPENDS `'nonce-{n}'` to `script-src`/`style-src` in the served CSP — gated on
  `dangerous_disable_asset_csp_modification.can_modify("script-src")`, which is TRUE by default
  (`tauri-utils config.rs:2924`, default `Flag(false)`).
- It also appends build-time `csp_hashes` for any inline scripts/styles present in the frontendDist HTML at
  BUILD time — but D4 removes ALL inline scripts/styles, so no hashes are generated for app content, and a
  RUNTIME-injected attacker inline script (not in the build-time HTML) gets neither a nonce nor a hash →
  BLOCKED. Net: `script-src 'self'` → after augmentation `'self' 'nonce-XXX'` → Tauri init runs via nonce,
  our external `assets/*.js` run via `'self'`, injected inline scripts blocked. Exactly the intended outcome.
- Therefore D3 is correct to (a) NOT set `dangerousDisableAssetCspModification` and (b) NOT add
  `unsafe-inline`/`unsafe-eval`.

### 3. D8 — RESOLVED: keep `mode: dev` for the PR gate (high confidence). No `built-debug` needed.
- Decisive fact: `tauri-utils-2.9.2/src/config.rs:2897`: *"If `dev_csp` is not specified, this value [`csp`]
  is also injected on dev."* → NOT setting `devCsp` means the strict `csp` is injected under `tauri dev`.
- CI reality: the PR gate (`accelerator.yml:355`) runs `_e2e-webdriver.yml` with **`mode: dev`** →
  `bunx tauri dev --no-watch --features webdriver` (`_e2e-webdriver.yml:70`). The release gate
  (`release-accelerator.yml:85`) runs **`mode: release`** → the real `target/release` binary (`:68`).
- `tauri.conf.json` has NO `devUrl`/`beforeDevCommand` — the frontend is a static `frontendDist: "./frontend"`
  served through Tauri's asset protocol, so `set_csp` (and thus CSP + nonce augmentation) runs in dev too;
  capabilities are compiled in and enforced identically in dev and release.
- **Action:** set `app.security.csp` only; leave `devCsp` UNSET (so csp covers both). The dev-mode PR-gate
  WebDriver run will exercise the real strict CSP + the real ACL. `built-debug` (codex-leg's proposal) is
  therefore unnecessary workflow churn. The release gate already covers the production binary as a backstop.
- Residual to watch: confirm the WebDriver launch under dev builds still applies capabilities (it does —
  they're compile-time), and that the debug-build rejection path (`webview/mod.rs:1825` `#[cfg(debug_assertions)]`
  detailed message vs release generic) doesn't matter to the test (it asserts rejection, not message text).

### Supporting facts verified
- `app.security.capabilities: Vec<CapabilityEntry>` is a real field (`config.rs:2954`) → D5's explicit
  `app.security.capabilities` list is valid config.
- `freeze_prototype` is a real toggle (`config.rs:2910`, default false) → A3 (freezePrototype DiD) available.
- `capabilities/default.json` today grants `core:default`, `autostart:allow-disable`,
  `autostart:allow-is-enabled`, `process:default` (PLUGIN perms) to ALL windows — NOT the 12 custom app
  commands, which are ungated by the has_app_acl=false path above. F-004's note (no `updater:default`) stays.

## Codex double-audit verdict (2026-07-14): CHANGES-REQUESTED — 3 HIGH + 5 MEDIUM
Codex corroborated the central inference from source (tauri-build acl.rs:274/408 → has_app_manifest → webview
invoke path) and confirmed D8 (dev enforces CSP+ACL here). Fold list (all verified against source):

- **HIGH-1 — D7 → DROP (source-verified by me too).** My "default.json grants all windows the same set" is
  FALSE: `capability.rs:162-163` — `windows`/`webviews` default to EMPTY vecs (no `["main"]` defaulting);
  `resolve_access` (authority.rs:459-460) treats empty patterns as matching NO window. So the current
  core:default/autostart/process grants are INERT — the frontend only calls the 12 custom commands via invoke
  (ungated today) and never a plugin/core command from JS; auth/update windows are closed from RUST
  (commands.rs:153,293), and core:default doesn't even include core:window:allow-close. ⇒ DROP core:default +
  ALL plugin grants; each capability grants only its window's app commands; if a core API is later needed,
  grant the minimal specific perm (e.g. core:window:allow-close) to the exact window after the positive suite
  flags it. Retaining core:default would NEWLY activate a broad core API (event emit, window/webview
  enumeration, path, image/rgba, resource close, menu/tray) — a regression, not the status quo.
- **HIGH-2 — cross-window negative test can pass spuriously.** A bare "promise rejected" also passes when
  __TAURI_INTERNALS__ is missing, wrong/closed window, popup times out, error swallowed (auth-flow.spec.ts:26
  + authorize.html:48 already swallow errors), args fail to deserialize, or D6's Rust guard rejects while the
  ACL silently failed open. FIX: (a) first invoke an ALLOWED command from the window-under-test through the
  same primitive and require SUCCESS; (b) then the forbidden command → return explicit {resolved,error}
  sentinel; fail if primitive absent or it resolves; (c) assert an ACL-SPECIFIC rejection message ("not
  allowed by ACL") DISTINCT from D6's generic Rust error, so D6 can't mask a broken ACL; (d) observable canary
  — verify no state changed; (e) cleanup in finally (no 60s hang); (f) static test asserts the EXACT per-window
  matrix, not just union equality. Also: `show_update_prompt_window` is compiled OUT under `webdriver`
  (windows.rs:130) → no real update-prompt WebDriver flow exists → add a controlled webdriver-only prompt
  trigger so respond_update_prompt is positively tested from the real `update-prompt` label.
- **HIGH-3 — CSP doesn't close off-origin exfil.** connect-src blocks fetch/XHR/WS but NOT top-level
  navigation or form submission; CSP omits `form-action` (does NOT fall back to default-src); windows are
  created with no on_navigation/on_new_window (windows.rs:43). On Linux CSP is `<meta>`-delivered and
  `frame-ancestors` is IGNORED in meta CSP. FIX: add `form-action 'none'; frame-src 'none'; child-src 'none';
  worker-src 'none'`; add Rust `on_navigation` (allow only the local asset origin) + `on_new_window` (deny);
  WebDriver-test remote-navigation + form-submit are rejected; stop claiming CSP alone stops all exfil.
- **MED-4 — D3 mechanism misstated (policy viable).** Correct outcome, wrong rationale: Tauri's IPC init
  scripts are WEBVIEW INIT scripts, not page inline scripts needing a bootstrap nonce; `ipc: http://ipc.localhost`
  are NOT auto-added to connect-src — they must be explicit (we already list them ✓); excluding 'self' from
  connect-src is correct; external style.css works under style-src 'self'; D4 inventory is COMPLETE (1 markup
  style, 4 CSSOM mutations, no <style>, no SVG presentation attrs; `btn.onclick=fn` from external script is NOT
  inline-string exec). FIX: correct the rationale wording; never fall back to unsafe-inline.
- **MED-5 — D6 helper cannot compile where placed.** `commands.rs` is in the LIBRARY crate (lib.rs:11);
  `windows.rs` is BINARY-only (main.rs:5) → a library command can't import require_label from the binary
  module. FIX: move labels/sanitizer/matchers/require_label into a NEW EXPORTED LIBRARY module used by both
  commands + window creation; define the explicit 12-command→label mapping; check the GETTERS too
  (get_config/get_autostart_enabled/get_system_info) if Rust is a complete independent layer. Codex CONFIRMED:
  the injected Window/WebviewWindow is unspoofable (Tauri takes it from the native InvokeMessage, not JS
  payload); deriving the expected auth label from request_id is sound; wrapping getters in Result<T,String>
  doesn't break JS callers (no internal Rust callers).
- **MED-6 — auth label truncates the UUID to 48 bits.** `sanitize_window_label` uses only 6 SHA-256 bytes
  (commands.rs:158-165) — not collision-free, needlessly shrinks the UUID's margin. FIX: use `auth-{request_id}`
  directly (UUID chars are valid Tauri label chars) or the full hash; compare the exact full label. (Touches
  the C9 label scheme — `auth-*` glob still matches; STRENGTHENS SEC-06.)
- **MED-7 — D8 overstates production parity.** dev DOES enforce csp+ACL here (gate not worthless), BUT the
  "release" WebDriver lane uses raw `cargo build --release` (_e2e-webdriver.yml:56) and Tauri's `custom-protocol`
  is a CLI-managed feature NOT enabled by raw cargo → that binary still runs Tauri's `cfg(dev)` config, so it's
  NOT a true production-mode backstop. FIX: keep dev for the 3-OS PR matrix but STATICALLY forbid `devUrl` and
  a weaker `devCsp`; drop the "release gate is production parity" claim; a built-debug production-mode lane is
  optional DiD (only needed if freezePrototype/protocol parity is relied on).
- **MED-8 — ignored bundle pipeline can embed stale/unreviewed trusted code.** Gitignored bundles are trusted
  by script-src 'self'; a presence-only check accepts stale output; `@tauri-apps/api: "^2"` is a broad range.
  FIX: pin an EXACT @tauri-apps/api version; review the lock diff; clean the output dir before building;
  build.rs rejects stale/inconsistent output (not just missing); every direct Cargo CI path runs frontend:build;
  scan emitted bundles for dynamic imports / remote URLs / eval / source-map refs.

Assumptions corrections to fold: default.json does NOT grant all windows (empty selectors → no match);
build.rs is NOT "bare" (does version + verified-sites validation; only the tauri build invocation is bare);
"WebDriver passes" does NOT prove all 12 commands are exercised (framework source proves callability); no
existing real update-prompt positive WebDriver flow; the release WebDriver path is not a true production build.

## Fable double-audit verdict: CHANGES-REQUESTED
Source-verified against tauri 2.11.0. Full transcript + reconciliation in `clusters/C10-audit-fable.md`.
Convergent with codex on the central inference (source-proven) + test-design weaknesses + D6 crate boundary.
Divergences adjudicated: D7→DROP (codex, over fable's retain — fable missed core:default is currently inert);
D8→built-debug required (fable's is_dev()==!custom-protocol trace, decisive). All folded into plan v2.

## Final fresh-context codex pass — INFRA FAILURE (AFK consult log)
The deep-tier's final fresh-context codex pass (step 6) was KILLED by the environment TWICE mid-research
(background task IDs bjhe6zcji-era reruns b1p1gcu9z + b8umlfcpg), each after several minutes reading source,
before producing any verdict — no recoverable output in the rollout jsonl (only research fragments). Codex
stayed authenticated (`codex login status` = logged in); the kills are infra (long background codex runs get
reclaimed), not an auth/OAuth issue. Note: the FIRST-round audit codex run (the CHANGES-REQUESTED one) DID
complete fine — only the longer final pass gets reclaimed. Mitigation: relaunched a LEAN, time-boxed
(`timeout 600`, effort `high`) final pass focused ONLY on the 3 contested decisions (D7/D8/test-design) with
source refs pre-supplied to cut research time (task b14l3s8mi). Per the AFK protocol, if this also dies I close
GATE 1 on my own judgment — justification: (a) TWO complete source-verified audits (codex+fable) already folded;
(b) I independently source-verified every load-bearing claim (webview/mod.rs:1819 ACL flip, capability.rs:162 +
authority.rs:459 empty-selector inertness, manager/mod.rs:53 CSP nonce, lib.rs:308 is_dev, the crate boundary,
the empty app manifest, no direct Rust test callers); (c) both divergences adjudicated with source citations;
(d) GATE 3 (mandatory post-impl codex xhigh on the ACTUAL diff) is the designed backstop that re-examines
everything against real code. The final PLAN pass is belt-and-suspenders, not the load-bearing gate.

**The LEAN final pass COMPLETED (b14l3s8mi, exit 0) → CHANGES-REQUESTED, effectively approval + 1 MEDIUM.**
Transcript: `clusters/C10-audit-codex-final.md`. Both adjudications CONFIRMED: **D7 CORRECT** (IPC transport +
Tauri init need no core:default; no verified popup flow needs core events/window; add back only a proven narrow
perm); **D8 CORRECT** (Linux built-debug adequately covers the compile-time custom-protocol branch; the 3-OS dev
matrix covers WebView/IPC platform differences; all-OS built-debug would be DiD, not a Gate-1 necessity).
ONE MEDIUM test-design fold (FOLDED into plan validation): proving an ALLOWED command works does NOT prove the
FORBIDDEN command NAME is real — a typo/nonexistent command earns the SAME ACL denial + leaves the canary
unchanged (false pass). Fix: FIRST invoke the EXACT negative-target command from its AUTHORIZED window with valid
args and verify its EFFECT (proves the name/args real), THEN the byte-identical command from the unauthorized
window → assert rejection + unchanged canary. Match INVARIANT ACL-message fields (command name, attacker label,
"not allowed"/window-context wording), NOT the full URL/string (Rust-generated → not OS-brittle, but pin only
stable substrings). "No other confident HIGH/MEDIUM issue." ⇒ **GATE 1 CLOSED — plan v2 approved, implementation-ready.**

## GATE 2 — implementation log
### P0 (deps) DONE — commit 470ca75
`@tauri-apps/api@2.11.1` (exact pin; published 2026-06-17 ⇒ passes the 7-day min-age; matches the tauri 2.11.0
crate minor line for invoke_key protocol compat). Gate green: frozen-lockfile consistent, package.json sorted.

### P1 (externalize + build guard) DONE — commit 0a86794
frontend-src/{bridge,authorize,settings,update-prompt}.js (import invoke from @tauri-apps/api/core);
scripts/build-frontend.ts (Bun.build → gitignored frontend/assets/*.js + fnv1a64 .build-manifest.json;
clean-before-build); frontend:build script; 3 HTML pages → one <script type=module> each (inline blocks +
tauri-bridge.js removed); settings markup style=display:none→hidden + runtime --fill→[data-fill] CSS +
.row[hidden] rule; mock→window.__TAURI_INTERNALS__.invoke (__TAURI__ left undefined); build.rs
missing/stale-bundle guard (fnv1a64 matching the TS); setup-accelerator builds bundles for desktop jobs +
tauri before(Dev/Build)Command. Static externalization test (scripts/tauri-trust-boundary.test.ts, 4 tests).
Flags UNCHANGED (withGlobalTauri still true — P2 flips them).
- Gate green: `frontend:build` emits 3 bundles; build.rs guard NEGATIVE-TESTED (missing bundle → "missing
  frontend bundle"; tampered source → "STALE"); `cargo check --features webdriver` compiles (46s); static +
  full scripts suite (14) green; biome + rustfmt clean.

### GOTCHA — gen/schemas/* are build-regenerated, platform+feature-specific; DO NOT commit the churn.
Local `cargo check --features webdriver` on Linux regenerated `gen/schemas/*`, ADDING a `linux-schema.json`
(base has only macOS/desktop) and a `webdriver` ACL key (base built without the feature). These are
editor-assist artifacts (the runtime ACL is compiled fresh from capability files + AppManifest at build; CI
does NOT enforce their freshness — else cross-platform CI would already fail on the regen diff). I accidentally
`git add -A`'d them, then a `git checkout security-hardening -- gen/schemas/` REGRESSED capabilities.json to a
stale pre-F-004 version (the local `security-hardening` ref lags my parent). FIX: restore ALL gen/schemas from
HEAD~1 (the parent), `git update-index --skip-worktree` the 4 tracked schemas so builds don't re-dirty the
tree, and add `gen/schemas/linux-schema.json` to `$(git rev-parse --git-path info/exclude)`.
**P3 caveat:** when adding the AppManifest, do NOT try to regenerate/commit gen/schemas on this Linux+webdriver
box — the committed macOS non-webdriver schemas stay as editor artifacts; new capability permissions validate
at build time regardless (schema is OUTPUT, not input). Use targeted `git add`, never `-A`, for the rest of C10.

### P2b (navigation guards) DONE — commit 20f63ee
windows.rs `open_or_focus_window`: `.on_navigation(is_local_asset_url)` (allow only tauri://localhost /
http://tauri.localhost) + `.on_new_window(|_,_| NewWindowResponse::Deny)`. Closes the navigation/form-submit
exfil vector CSP connect-src misses (codex HIGH-3; Linux meta-CSP ignores frame-ancestors). Pure matcher
unit-tested (9 allow/block cases). Both `tauri::Url` + `tauri::webview::NewWindowResponse` are exported;
`on_navigation: Fn(&Url)->bool`, `on_new_window: Fn(Url,NewWindowFeatures)->NewWindowResponse<R>`. clippy -D
clean both feature sets.

### P3 (per-window capability ACL + AppManifest) DONE — commit 161138b — the ACL flip, VERIFIED
- build.rs: `tauri_build::try_build(Attributes::new().app_manifest(AppManifest::new().commands(&[12])))`.
- **EMPIRICALLY VERIFIED the flip**: the regenerated `target/.../out/acl-manifests.json` now has an
  `__app-acl__` key (absent before) → `has_app_acl=true` → app-local commands are now ACL-gated. The resolved
  `out/capabilities.json` shows authorize→['auth-*'](2), settings→['settings'](9), update-prompt(1) — exact
  per-window scoping. Runtime cross-window-DENIAL proof = the WebDriver spec (CI).
- GOTCHA — **permission ids are KEBAB, not snake.** The AppManifest.commands() doc says "allow-$command where
  $command is snake_case", but the identifier validator REJECTS underscores ("identifiers can only include
  lowercase ASCII, hyphens…"). So capability `permissions` must be `allow-get-config` (kebab), NOT
  `allow-get_config`. My plan's inference (kebab) was right; the doc comment is misleading. Confirmed by the
  autogenerated `permissions/autogenerated/get_config.toml` → `identifier = "allow-get-config"`.
- GOTCHA — AppManifest.commands() auto-generates `permissions/autogenerated/*.toml` (allow-/deny- per command)
  on EVERY build ("DO NOT EDIT" header) — pure output from the COMMANDS list → gitignored `/permissions/autogenerated`.
- Explicit `app.security.capabilities: ["authorize","settings","update-prompt"]` allowlist builds clean
  alongside the auto-discovered files (CapabilityEntry::Reference).
- DEVIATION from D5: OMITTED the reserved empty `main.json`. There is no `main` window (windows:[]), and
  has_app_acl=true already default-DENIES any window not explicitly granted — an empty-permissions capability
  would only add validation risk for zero gain. The default-deny IS the deny-all main.json would provide.
- Static drift guards (5 P3 tests): exactly the 3 capability files; exact per-window matrix; auth excludes all
  settings mutators; set-equality build.rs COMMANDS == main.rs generate_handler! == union of grants == 12;
  config allowlist == the 3. 12 static tests total green; clippy -D + cargo test clean.

### P4 (Rust caller-label DiD) DONE — commit 68a6bf4
require_label (pure) + require_auth_window/is_auth_label in the LIB commands.rs (not bin windows.rs); a
`window: tauri::WebviewWindow` param on all 12 commands; settings mutators+getters → "settings",
respond_update_prompt → "update-prompt", respond_auth → auth-{hash(request_id)} (binds window↔request,
strengthens SEC-06), get_verified_info → any auth-<32hex>. Getters wrapped in Result (VERIFIED zero internal
Rust callers). Widened sanitize_window_label 6→16 bytes (48→128 bit); both callers use the shared fn so they
stay consistent; no e2e/test hardcodes the old width. 3 predicate unit tests. clippy -D both features clean.
The ACL denies cross-WINDOW-GLOB calls before dispatch (so the label check is unreachable there — its live
job is the cross-REQUEST binding within the auth-* glob). commitlint: lowercase-lead the subject ("rust …",
"webdriver …") — "WebDriver"/"Rust"/"DiD" leading are rejected as start/pascal/upper-case.

### P5 (lock) DONE — no new code — commit 1ee32f9 (bundled with the WebDriver spec)
The static drift tests already gate: `scripts/tauri-trust-boundary.test.ts` runs via `bun test scripts/` =
accelerator `test:unit`, invoked in the `lint` CI job (accelerator.yml:192) which is in accelerator-status.
`biome check .` (in `bun run lint`) covers the new TS/JS. Nothing to wire.

### WebDriver trust-boundary.spec.ts + built-debug lane (D8) DONE — commit 1ee32f9
Real-webview proof (no mocks): no window.__TAURI__; a granted settings cmd resolves; CSP blocks inline
script (securitypolicyviolation)+eval+off-origin fetch; cross-window ACL denial from the auth popup asserting
the ACL "not allowed" reason (distinct from the Rust label "not available") + target proven real from
Settings first (final-codex MED) + state canary. All invokes via injected __TAURI_INTERNALS__ (keyless
postMessage is dropped pre-ACL). D8: `built-debug` mode added to _e2e-webdriver.yml (`tauri build --debug
--no-bundle --features webdriver` → target/debug binary — the only path with tauri's custom-protocol feature)
+ a Linux `e2e-webdriver-builtdebug` lane in accelerator.yml, gated in accelerator-status. Note: `on_new_window`
API present in tauri 2.11 (NewWindowResponse::Deny); on_navigation Fn(&Url)->bool.

### GATE 4 (local) GREEN
cargo fmt --check; clippy -D (default + webdriver); cargo test src-tauri 24 + core 172; 22 static drift tests
(all C10 guards); biome clean; actionlint clean. WebDriver/Playwright ⇒ CI (HARD RULE). GATE 2 COMPLETE.

### GATE 3 (post-impl codex xhigh on the 1937-line diff): CHANGES-REQUESTED — no HIGH, 1 MED + 2 LOW — FOLDED (f34219f)
Transcript: clusters/C10-audit-codex-postimpl.md. Codex confirmed the CORE sound: legitimate flows work
(window labels exactly match the Rust guards), the 12-command ACL set-equality holds + no window gets
another's command, F-004/F-010/SEC-06/F-014 preserved, CSP fits the frontend, NewWindowResponse::Deny works
on WebKitGTK/WKWebView/WebView2, CI wiring correct, the "not allowed" ACL wording is Rust-generated (not
engine-specific). All 22 bun + 4 Rust tests passed under its own run. Folds:
- **MED — nav guard too permissive.** `is_local_asset_url` accepted BOTH `tauri://localhost` AND
  `http://tauri.localhost` on every OS and ignored ports/creds → on Linux/macOS a real
  `http://tauri.localhost:59833/?data=…` loopback HTTP navigation (data-exfil) was allowed. FIX: platform-gate
  the scheme/host (`#[cfg(windows)]` http://tauri.localhost vs `#[cfg(not(windows))]` tauri://localhost) +
  reject any username/password/explicit port; added other-platform + port + userinfo rejection tests.
- **LOW — staleness guard didn't hash OUTPUTS.** It fingerprinted only frontend-src inputs, so a swapped
  `frontend/assets/settings.js` passed (source hashes still matched). FIX: SHA-256 (was fnv1a64; sha2+hex
  build-deps); manifest schema 2 records outputs + package.json + root bun.lock; build.rs verifies all;
  build-frontend.ts snapshots inputs before+after the build (mid-build-edit race). Output-swap NEGATIVE-TESTED
  (`printf >> settings.js` → "STALE or SWAPPED" panic).
- **LOW — WebDriver proof gaps.** (a) off-origin fetch treated any rejection as CSP → now asserts a
  `securitypolicyviolation` with effective/violatedDirective `connect-src` (DNS/TLS failure wouldn't fire it);
  (b) never exercised the nav guards → added an on_navigation/window.open block test (URL + handle-count
  unchanged); (c) the "mutating" cmd removed a known-absent origin (proved dispatch, not mutation) → now
  set_speed round-trips a real change, and the cross-window denial uses set_speed with a speed canary (a
  denied call that WOULD have changed state proves the ACL blocked execution).
GATE-4 re-validated GREEN post-fold (fmt/clippy -D/test 24+172; 22 static; biome; actionlint).

### GATE 5 — PR #393 CI: WebDriver RED on ALL WebKit (macOS + Linux dev + Linux built-debug), fixed.
Symptom: every WebDriver spec (incl. pre-existing smoke/settings/auth-flow) failed in its before-all with
"Settings bootstrap window not found among 1 window(s)" — the settings window opened but its page didn't
render (`#speed-label` absent, wrong title). desktop-ui (Playwright/Chromium, NO CSP) PASSED, so it wasn't
the frontend JS/DOM. ROOT CAUSE (from the uploaded `/tmp/tauri.log` artifact): `ERROR tauri::manager: asset
not found: settings.html`. i.e. Tauri's asset resolver had NO settings.html. DIAGNOSIS: I added
`beforeDevCommand`/`beforeBuildCommand: "bun run frontend:build"` to tauri.conf.json; the working base
(origin/security-hardening) has `build: {frontendDist: "./frontend"}` ONLY. With `beforeDevCommand` set, the
tauri CLI's `tauri dev` changes its frontendDist asset handling (dev-server/devUrl path → codegen embeds an
EMPTY asset set per tauri-codegen context.rs:178 `if dev && dev_url.is_some()`), so `tauri://localhost/
settings.html` resolves to nothing. FIX: REMOVE both before*Command (single-variable revert to the working
base's build config). Bundle-building is already covered: CI by setup-accelerator's `frontend:build` step
(runs before every desktop cargo/tauri path); local by the build.rs missing-bundle panic hint. Debugging
lever: the `Upload logs` step in _e2e-webdriver.yml uploads `/tmp/tauri.log` as
`webdriver-e2e-logs-<os>-<mode>` on failure — `gh run download <run> -n <name>` shows the app's real console
(WebKit errors go there, NOT the CI job log). GATE-3 verdict/folds were unaffected (they're all still in).
