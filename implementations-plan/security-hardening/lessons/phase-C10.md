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

## Codex / Fable double-audit verdicts
(to be appended when the background audits return)
