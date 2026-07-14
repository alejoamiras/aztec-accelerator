# F-012 implementation plan: Tauri trust boundary

## Target state

The desktop frontend becomes a small, explicitly built ESM application:

- `withGlobalTauri` is `false`.
- Every page imports the supported `@tauri-apps/api/core` API through bundled modules.
- No inline `<script>`, `<style>`, `style=`, or runtime `element.style` mutations remain.
- A strict CSP permits only bundled assets and Tauri’s two documented IPC transports.
- Each window label receives only its required custom commands.
- Every Rust command also validates the caller label before reading or mutating state.
- CI builds the actual Tauri application and exercises real IPC/CSP on Linux, macOS, and Windows.

This replaces the current global bridge at `packages/accelerator/src-tauri/frontend/tauri-bridge.js:9`, the inline popup scripts, and the shared capability at `packages/accelerator/src-tauri/capabilities/default.json:1-11`.

## 1. Buildless frontend migration

### Decision

Use Bun’s existing bundler to produce three small, standalone ESM bundles. Do not introduce Vite, a dev server, direct imports from undocumented Tauri-generated assets, or a handwritten wrapper around `window.__TAURI_INTERNALS__`.

| Option | Cost/risk | Decision |
|---|---|---|
| Bun `build` with three entries | One dependency and one build command; Bun already exists throughout package scripts and CI | Use |
| Add esbuild directly | Another direct dependency despite Bun already providing the required bundling | Reject |
| Add Vite | Dev-server/configuration overhead for three static pages | Reject |
| Load Tauri “ESM assets” directly | Bare `@tauri-apps/api` imports are not browser-resolvable without a bundler | Reject |
| Handwritten IIFE using internals | Couples application code to undocumented `__TAURI_INTERNALS__` behavior | Reject |
| Keep `window.__TAURI__` | Requires `withGlobalTauri:true`, preserving the finding | Reject |

Tauri documents package imports as the bundler path and the global as the vanilla-JS alternative requiring `withGlobalTauri`; this plan takes the supported bundler path. [Tauri JavaScript API](https://v2.tauri.app/reference/javascript/api/)

### File layout

Create source modules outside the embedded asset directory:

```text
packages/accelerator/src-tauri/
├── frontend/
│   ├── authorize.html
│   ├── settings.html
│   ├── update-prompt.html
│   ├── style.css
│   └── assets/                  # generated, gitignored
│       ├── authorize.js
│       ├── settings.js
│       └── update-prompt.js
└── frontend-src/
    ├── tauri-bridge.js
    ├── authorize.js
    ├── settings.js
    └── update-prompt.js
```

`frontend-src/tauri-bridge.js` will:

```js
import { invoke } from "@tauri-apps/api/core";

export { invoke };
export function wireToggle(...) { ... }
export function wireButton(...) { ... }
export function showErrorHint(...) { ... }
```

Each page module imports only what it needs from `./tauri-bridge.js`. Bun bundles the dependency graph into each standalone output, without code splitting, minification, or source maps. Duplication is preferable to hashed shared chunks and HTML-manifest machinery for three tiny pages.

Add `@tauri-apps/api: "^2"` to `packages/accelerator/package.json`; `bun.lock` supplies the reproducible resolved version.

Add:

```json
"frontend:build": "bun build src-tauri/frontend-src/authorize.js src-tauri/frontend-src/settings.js src-tauri/frontend-src/update-prompt.js --outdir src-tauri/frontend/assets --target browser --format esm"
```

Then:

- Prefix the existing `prebuild` with `bun run frontend:build`; the shared setup action already invokes it before desktop Rust builds.
- Prefix `test:e2e:ui` and `test:unit` with `bun run frontend:build`.
- Add `build.beforeDevCommand` and `build.beforeBuildCommand` to `tauri.conf.json`, both running `bun run frontend:build` with `cwd: ".."`; set `wait:true` for the dev hook.
- Keep `frontendDist: "./frontend"` at `tauri.conf.json:7`, avoiding HTML/CSS copy machinery.
- Gitignore `src-tauri/frontend/assets/*.js`.
- Make `build.rs` fail with an actionable “run `bun run frontend:build`” error if any of the three generated bundles is absent, preventing a direct Cargo build from silently embedding HTML with missing scripts.

## 2. Externalize scripts and remove inline styling

### HTML entrypoints

Replace both the current bridge tag and inline block in each page with one external module:

```html
<script type="module" src="assets/authorize.js"></script>
<script type="module" src="assets/settings.js"></script>
<script type="module" src="assets/update-prompt.js"></script>
```

Concretely:

- Move `authorize.html:38-71` to `frontend-src/authorize.js`.
- Move `settings.html:59-176` to `frontend-src/settings.js`.
- Move `update-prompt.html:24-44` to `frontend-src/update-prompt.js`.
- Remove the old `tauri-bridge.js` tags at line 8 of all three pages.
- Preserve all `textContent` rendering of query parameters and origins.

### Inline styles

`settings.html:33` is the only literal `style=` attribute, but `settings.html:96,112,129,132` also create runtime inline styles. Remove all five cases:

- Change the Safari row to `<div id="safari-row" class="row" hidden>`.
- Toggle `element.hidden` for Safari and the empty-origin state.
- Add `data-fill="4"` to the speed slider.
- Replace `style.setProperty("--fill", ...)` with `speedSlider.dataset.fill = String(index)`.
- Add five selectors to `style.css`:

```css
[hidden] {
  display: none !important;
}

.speed-slider[data-fill="0"] { --fill: 0%; }
.speed-slider[data-fill="1"] { --fill: 25%; }
.speed-slider[data-fill="2"] { --fill: 50%; }
.speed-slider[data-fill="3"] { --fill: 75%; }
.speed-slider[data-fill="4"] { --fill: 100%; }
```

This permits `style-src 'self'` without `'unsafe-inline'`.

### Playwright mock migration

The plain-browser tests currently inject `window.__TAURI__` at `packages/accelerator/e2e/tauri-mock.js:49-64`. Change the mock to supply only:

```js
window.__TAURI_INTERNALS__ = {
  invoke: async (cmd, args, options) => { ... }
};
```

The bundled supported API delegates `invoke` to this injected primitive. Do not set `window.__TAURI__`; add a Playwright assertion that it remains `undefined`.

## 3. Strict CSP and global removal

Set this exact configuration under `app.security`:

```json
{
  "csp": {
    "default-src": "'self'",
    "script-src": "'self'",
    "style-src": "'self'",
    "connect-src": "ipc: http://ipc.localhost",
    "img-src": "'self'",
    "object-src": "'none'",
    "base-uri": "'none'",
    "frame-ancestors": "'none'"
  },
  "capabilities": ["authorize", "settings", "update-prompt", "main"]
}
```

Set `app.withGlobalTauri` from `true` at `tauri.conf.json:11` to `false`.

Tauri documents `connect-src ipc: http://ipc.localhost` and appends its required hashes/nonces to the compiled CSP itself. [Tauri CSP documentation](https://v2.tauri.app/security/csp/)

Platform behavior:

- macOS and Linux use `ipc://localhost/...`; `ipc:` permits that custom scheme.
- Windows WebView2 maps the custom protocol to `http://ipc.localhost/...`.
- Both sources must remain even when one platform appears green.
- If either is omitted, Tauri’s custom-protocol fetch is CSP-blocked. Tauri may fall back to `window.ipc.postMessage`, so IPC success alone can mask the incorrect CSP; CI must also assert that an allowed invocation produces no `securitypolicyviolation`.

No other connection source is justified:

- The frontend never fetches the accelerator server at `127.0.0.1:59833`.
- Updater network access remains Rust-side.
- The remote updater endpoint in `tauri.conf.json:14-18` therefore does not belong in frontend `connect-src`.
- Excluding `'self'` from `connect-src` intentionally prevents arbitrary same-origin `fetch`; static script, stylesheet, and image loading are governed by their own directives.
- `img-src 'self'` is sufficient for the current inline SVG and local assets; no `data:`, `blob:`, or remote images are required.
- Do not add `'unsafe-inline'`, `'unsafe-eval'`, or `'wasm-unsafe-eval'`.

## 4. Per-window capabilities and Rust defense-in-depth

### Build-time command declaration

Replace the bare `tauri_build::build()` at `build.rs:21` with `tauri_build::try_build(...)`, preserving the existing version and verified-sites checks.

Declare exactly the commands registered at `main.rs:487-500`:

```rust
const COMMANDS: &[&str] = &[
    "get_config",
    "get_autostart_enabled",
    "set_autostart",
    "set_speed",
    "remove_approved_origin",
    "get_system_info",
    "get_verified_info",
    "respond_auth",
    "enable_safari_support",
    "disable_safari_support",
    "set_auto_update",
    "respond_update_prompt",
];
```

Pass them through:

```rust
tauri_build::Attributes::new()
    .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS))
```

This generates `allow-<command>` permissions instead of leaving registered application commands globally callable. Tauri explicitly requires `AppManifest::commands` to change the default all-window behavior. [Tauri capabilities documentation](https://v2.tauri.app/security/capabilities/)

### Capability files

Delete `capabilities/default.json` and create:

| Capability | Window labels | Exact permissions |
|---|---|---|
| `authorize.json` | `["auth-*"]` | `allow-get-verified-info`, `allow-respond-auth` |
| `settings.json` | `["settings"]` | `allow-get-config`, `allow-get-autostart-enabled`, `allow-set-autostart`, `allow-set-speed`, `allow-remove-approved-origin`, `allow-get-system-info`, `allow-enable-safari-support`, `allow-disable-safari-support`, `allow-set-auto-update` |
| `update-prompt.json` | `["update-prompt"]` | `allow-respond-update-prompt` |
| `main.json` | `["main"]` | `[]` |

All four retain `platforms: ["linux", "macOS", "windows"]`.

The empty `main` capability is intentional. `tauri.conf.json:10` currently creates no main window; settings is `"settings"` (`windows.rs:58-70`), authorization uses `"auth-<hash>"` (`windows.rs:83-104`), and updates use `"update-prompt"` (`windows.rs:135-151`). Reserving `main` as deny-all prevents a future default main window from silently inheriting privileged commands.

Remove these current broad grants from frontend capabilities:

- `core:default`
- `autostart:allow-disable`
- `autostart:allow-is-enabled`
- `process:default`

The frontend uses Rust wrappers, not raw plugin APIs. In particular, the update prompt must retain only `respond_update_prompt`; raw updater and process commands must remain inaccessible so the signed-update path cannot be bypassed.

Explicitly listing the four capability identifiers in `tauri.conf.json` prevents a stray capability file from being auto-enabled.

### Rust caller-label checks

Add `tauri::WebviewWindow` to every custom command and validate it before touching state:

- Settings commands require label exactly `"settings"`.
- `get_verified_info` requires an authorization label matching `auth-` plus exactly 12 lowercase hexadecimal characters.
- `respond_auth` performs the stronger check that the caller label equals `auth-{sanitize_window_label(request_id)}` before `auth.resolve`.
- `respond_update_prompt` requires exactly `"update-prompt"` before saving preferences, taking `PendingUpdate`, or closing the prompt.

Wrap infallible getters in `Result<T, String>` so caller rejection is expressible. Existing frontend code already treats invokes as promises.

Centralize label constants/predicates in `windows.rs`, and use them both in window creation and command validation. Log rejected calls with command and caller label, but return a generic “command not permitted from this window” error.

Add pure Rust tests for:

- Valid settings/update labels.
- Valid generated auth labels.
- Malformed and near-match auth labels.
- A request ID whose computed auth label differs from the caller.
- Every command-to-window group.
- Validation occurring before auth resolution, config mutation, pending-update consumption, or window closing.

These checks remain valuable even after capabilities because the public Tauri global is not the enforcement boundary: compromised bundled code can still reach its own allowed `invoke` path.

## 5. Validation on a GUI-less VPS and in CI

### Static and unit gates

Add `scripts/tauri-trust-boundary.test.ts` to verify:

- `withGlobalTauri === false`.
- CSP directives and tokens equal the policy above.
- Only the four named capabilities are enabled.
- The union of capability custom permissions equals the `build.rs` command list and `main.rs` handler list.
- No capability contains raw `core`, `autostart`, `process`, or updater permissions.
- No HTML contains inline script blocks, `<style>`, `style=`, or a non-module application script.
- No frontend source references `window.__TAURI__`.
- Exactly three generated entry bundles exist after `frontend:build`.

Run locally/VPS:

```text
bun install --frozen-lockfile
bun run --cwd packages/accelerator frontend:build
bun run lint
bun run --cwd packages/accelerator test:unit
bun run --cwd packages/accelerator test:e2e:ui
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Playwright remains the fast UI/behavior gate. Its current plain HTTP server (`playwright.config.mjs:13-17`) does not prove Tauri’s CSP or capabilities.

### Authoritative Tauri runtime gate

Extend the existing three-platform WebDriver matrix at `.github/workflows/accelerator.yml:338-355`:

1. Change the reusable workflow mode from `dev` to `built-debug`.
2. Build with:

   ```text
   bunx tauri build --debug --no-bundle --features webdriver
   ```

3. Launch `src-tauri/target/debug/aztec-accelerator` under the existing Linux Xvfb/tray/DBus setup.
4. Preserve the existing Windows installer build at `accelerator.yml:422-433` as the release-package gate.
5. Add `e2e-webdriver/trust-boundary.spec.ts` to `wdio.conf.ts`.

The new runtime spec will:

- Assert `window.__TAURI__` is absent.
- Exercise an allowed real settings command and assert it succeeds.
- Invoke `respond_update_prompt` from the settings window through the low-level injected primitive and assert Tauri rejects it as unauthorized.
- During an allowed IPC call, collect `securitypolicyviolation` events and assert none occur. This catches an omitted `ipc:` or `http://ipc.localhost` even if Tauri’s postMessage fallback makes the command appear successful.
- Intentionally inject an inline script, inline `<style>`, and a `fetch("https://example.invalid")`; assert each is blocked and the event’s `originalPolicy` contains the expected directives.
- Assert external `style.css` is applied and changing the speed slider via `data-fill` produces no violation.
- Extend the auth-flow test to prove real auth IPC succeeds and a settings-only command is rejected from the `auth-*` window.
- Continue the existing real settings persistence and authorization-flow coverage at `e2e-webdriver/settings.spec.ts:44-63` and `auth-flow.spec.ts:162-252`.

Expected probe violations are kept separate from normal-operation violations. Any unexpected violation fails the job, and `/tmp/tauri.log` plus WebDriver output remains uploaded on failure.

## 6. Phasing and gates

### Phase 1 — Bundled module frontend

Implement the Bun entries, shared module, HTML changes, mock migration, and inline-style removal.

Gate:

- `frontend:build` emits exactly three bundles.
- Existing Playwright suites pass.
- Static test finds no inline executable/style surface and no public Tauri global reference.

### Phase 2 — CSP and public-global shutdown

Set `withGlobalTauri:false` and the exact CSP.

Gate:

- Static CSP test passes.
- `bunx tauri build --debug --no-bundle --features webdriver` succeeds on Linux.
- WebDriver settings IPC succeeds without a CSP violation.
- Inline script/style/network probes are blocked.

### Phase 3 — Capability and Rust authorization boundary

Declare commands in `build.rs`, split capability files, explicitly enable them, and add caller-label validation.

Gate:

- Capability schemas compile during the Tauri build.
- Command/manifest/capability set-equality test passes.
- Rust caller-label tests pass.
- Allowed settings/auth IPC succeeds; cross-window negative calls fail before side effects.

### Phase 4 — Cross-platform campaign gate

Run the built-debug WebDriver suite on Linux, macOS, and Windows and preserve all existing accelerator jobs listed in `accelerator-status` at `.github/workflows/accelerator.yml:488-517`.

Gate:

- Three-platform Tauri build and WebDriver matrix green.
- Playwright UI job green.
- Windows packaged build/launch green.
- Clippy, Rust tests, lint, headless smoke, release smoke, and integration E2E remain green.

## Security considerations

- `withGlobalTauri:false` reduces accidental exposure but is not treated as an authorization control.
- CSP blocks injected executable/style content and frontend network destinations; it does not protect against malicious Rust code, a compromised signed bundle, or WebView vulnerabilities.
- Capabilities constrain a compromised page to its own command set.
- Rust caller checks protect against capability drift and incorrectly broadened future configuration.
- The dynamic `auth-*` grant is acceptable because no capability grants frontend window creation, while `respond_auth` additionally binds the caller label to its request ID.
- The update prompt retains only the verified Rust update command; no raw updater or process permission is restored.
- Tauri’s compile-time nonce/hash additions remain enabled; `dangerousDisableAssetCspModification` must not be set.
- New frontend network, image, WebAssembly, inline styling, or window-creation requirements require a security review rather than broadening CSP/capabilities opportunistically.

## Assumptions

### Facts

- `frontendDist` points directly at the static frontend (`tauri.conf.json:6-8`).
- The public global is enabled (`tauri.conf.json:11`).
- All three pages contain inline scripts and load the global bridge.
- Settings contains literal and programmatic inline styles.
- All custom commands are in one handler (`main.rs:487-500`).
- Current capability permissions are not tied to window labels (`capabilities/default.json:1-11`).
- Playwright and three-platform WebDriver gates already exist (`accelerator.yml:308-355`).

### Inferences

- The frontend has no legitimate direct network dependency, so IPC-only `connect-src` is safe.
- Raw autostart/process/core permissions are unnecessary because UI operations flow through custom Rust commands.
- There is no current `"main"` webview; its capability should therefore be explicitly deny-all.
- The current checkout does not yet contain caller-label command guards; if another parallel leg lands them first, implementation should preserve and extend those guards rather than duplicate them.

### Asks

- Non-blocking campaign confirmation: use built-debug `tauri build --no-bundle` for every WebDriver platform to satisfy the real-build rule without tripling release-optimization time.
- Before merging, confirm no downstream branch introduces a privileged `"main"` window or direct frontend fetch; otherwise its exact permissions/destination must be reviewed explicitly.

**KEY DIVERGENCE:** use a three-entry Bun ESM build plus a deny-all reserved `main` capability, instead of adding a frontend framework or relaxing CSP/global exposure to preserve the buildless model.