# C10 / F-012 — fable planning leg (deep) — KEY DECISIONS (full text in task afc06ffaa9cbfe5e5)

## CRITICAL reframing (load-bearing, source-verified)
There are currently NO Rust caller-label checks (commands take only State/AppHandle), AND
`capabilities/default.json` grants NONE of the 12 custom app commands — yet they all work. ⇒ **Tauri v2
does NOT gate app-local commands by default**: every window (incl. an `auth-*` popup) can already invoke
EVERY command (`enable_safari_support`, `remove_approved_origin`, …). THAT ungated surface is the real
trust-boundary hole — NOT closed by CSP or withGlobalTauri. The master "keep the Rust caller-label checks as
DiD" assumes checks that don't exist → the plan must ADD them as PRIMARY enforcement; the capability split
is the complementary Tauri-native layer whose real semantics are proven in CI.

## Decisions
1. **withGlobalTauri:false**: minimal `Bun.build` per-page ESM bundle importing `{invoke} from
   "@tauri-apps/api/core"` → `dist/{page}.js`; frontendDist→`./dist`; before(Dev/Build)Command:
   `bun run build:frontend`; add `@tauri-apps/api` devDep. (The `__TAURI_INTERNALS__` shim is the true
   lighter floor but undocumented → rejected as primary.) This build lands FIRST + de-risks (works with
   withGlobalTauri still true, since internals is always present).
2. **Strict CSP** (`app.security.csp`): `default-src 'self'; script-src 'self'; style-src 'self';
   img-src 'self'; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; base-uri 'self';
   frame-ancestors 'none'; frame-src/child-src/worker-src/form-action 'none'`. Tauri auto-augments
   script-src w/ a per-response nonce for its bootstrap + connect-src w/ ipc → `'self'` works, proven by a
   green WebDriver IPC run. Externalize the ONE markup `style="display:none"` (settings.html:33) → a
   `.hidden` class (all other style changes are CSSOM, not CSP-governed). `unsafe-inline` style only as an
   empirically-gated fallback.
3. **Externalize** each inline `<script>` → `frontend/src/{authorize,settings,update-prompt,common}.ts`
   (common replaces tauri-bridge.js); HTML loads one `<script type="module" src="./{page}.js">`.
4. **Per-window caps**: split default.json → `authorize.json` (`windows:["auth-*"]` glob: get_verified_info
   + respond_auth), `settings.json` (the 9 settings commands + autostart:allow-disable/is-enabled),
   `update-prompt.json` (respond_update_prompt). `build.rs`: declare all 12 commands via
   `AppManifest::commands(&cmds)` so per-command `allow-<kebab>` perms generate (verify the exact id form
   against regenerated gen/schemas/acl-manifests.json).
5. **Rust caller-label checks (GUARANTEED, added)**: add `WebviewWindow`/`Window` params +
   `require_label(window, pred)` helper; respond_auth ⇒ label == `auth-<hash(request_id)>` (also
   strengthens SEC-06); respond_update_prompt ⇒ `update-prompt`; settings mutators ⇒ `settings`;
   get_verified_info ⇒ label starts `auth-`. Err+warn on mismatch. Unit-test the matcher.
6. **CI validation** (GUI-less VPS): (a) static `bun test scripts/frontend-security.test.ts` — dist has 0
   inline `<script>`/`on*=`/markup-`style=`, 1 module script each; tauri.conf withGlobalTauri:false + CSP
   assertions; capabilities least-privilege (authorize excludes settings commands). (b) Playwright mock:
   change tauri-mock to `window.__TAURI_INTERNALS__.invoke`; frontendDir→dist; build before serve. (c)
   WebDriver `csp.spec.ts`: `typeof window.__TAURI__ === "undefined"` + a real get_config resolves; inline
   `<script>` blocked (securitypolicyviolation); eval throws; **cross-window denial** — from an auth popup,
   `__TAURI_INTERNALS__.invoke('remove_approved_origin',...)` REJECTS (passes whether ACL or Rust-label
   denies; fails loudly only if the boundary is open — THE proof F-012 is fixed).

## Phases: 0 deps → 1 build+externalize (flags unchanged) → 2 flip withGlobalTauri:false+CSP → 3 caps+build.rs
→ 4 Rust caller-label DiD → 5 lock static tests. Each with a WebDriver/cargo/Playwright gate.

## Key Asks (for consolidation): style-src 'self' vs unsafe-inline-fallback; dist/ vs in-place *.js;
drop process:default (chosen) + retain autostart:* under settings; freezePrototype:true (extra DiD).
