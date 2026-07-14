# Verdict: CHANGES-REQUESTED

The load-bearing ACL claim is true for the locked stack, but the plan has three blocking defects: D7 broadens privileges, the cross-window test can pass without proving ACL enforcement, and the CSP does not provide the claimed off-origin containment.

## Central claim disposition: TRUE

For locked `tauri 2.11.0` / `tauri-build 2.6.2`, `AppManifest::commands()` does more than make permissions grantable:

1. It generates a non-empty application ACL manifest ([tauri-build ACL source](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-build-2.6.2/src/acl.rs:274)).
2. That sets `has_app_manifest` ([tauri-build ACL source](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-build-2.6.2/src/acl.rs:408)).
3. The invoke path then applies ACL checks to app-local commands; without a matching grant, it rejects before dispatch ([Tauri invoke path](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-2.11.0/src/webview/mod.rs:1819)).

Today, no app manifest exists, so that branch is skipped and all registered commands at [main.rs:487](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/src-tauri/src/main.rs:487) are framework-default-allow. After declaration, an ungranted `(window, command)` is default-deny. The capability layer is genuine enforcement, not theater. This matches Tauri’s permission model: app permissions must be referenced by a capability to be granted. [Tauri permissions](https://v2.tauri.app/security/permissions/)

## HIGH findings

### 1. D7 would activate a broad core API that is not active today

**Claim attacked:** Retain `core:default` because popups may need window close or events ([plan:66](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/implementations-plan/security-hardening/clusters/C10-plan.md:66)).

**Why wrong:** The current [default.json](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/src-tauri/capabilities/default.json:1) has neither `windows` nor `webviews`. In the locked resolver, empty target-pattern lists match no window. Therefore the statement that it “grants all windows the same set” is false.

Adding explicit selectors while retaining `core:default` would newly expose:

- event emit/emit-to;
- all-window/webview enumeration and metadata;
- path resolution;
- image creation/from-path/RGBA access;
- resource closing;
- extensive menu and tray creation/mutation/removal.

See the generated manifest at [acl-manifests.json:1](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/src-tauri/gen/schemas/acl-manifests.json:1) and [Tauri core permissions](https://v2.tauri.app/reference/acl/core-permissions/).

No page currently uses a core window or event API. Authorization and update windows are closed from Rust at [commands.rs:153](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/src-tauri/src/commands.rs:153) and [commands.rs:293](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/src-tauri/src/commands.rs:293). `core:default` does not include `core:window:allow-close` anyway. Raw autostart permissions are also unnecessary: the frontend invokes custom Rust wrappers, and those wrappers call the plugin natively.

**Fix:** Resolve D7 to **DROP**. Each capability should contain only its app-command grants. If JavaScript closing is later required, grant only `core:window:allow-close` to the exact window after testing it. Drop all autostart/process grants.

### 2. The cross-window rejection test can pass spuriously and cannot prove the ACL layer

**Claim attacked:** The negative test “fails loudly only if the boundary is actually open” and may pass whether ACL or Rust rejects ([plan:88-92](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/implementations-plan/security-hardening/clusters/C10-plan.md:88)).

**Why wrong:** A bare “promise rejected” assertion also passes when:

- `__TAURI_INTERNALS__.invoke` is missing or malformed;
- WebDriver is on the wrong or already-closed window;
- the popup closes or times out during the probe;
- the test catches/swallow errors;
- argument deserialization fails before authorization;
- D6’s Rust guard rejects while the ACL has silently failed open.

The existing authorization harness deliberately swallows several WebKit window errors at [auth-flow.spec.ts:26](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/e2e-webdriver/auth-flow.spec.ts:26), and the page swallows `get_verified_info` failures at [authorize.html:48](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/src-tauri/frontend/authorize.html:48).

The static union-equality test is also insufficient. Commands can be swapped between capability files while preserving the union. The plan itself recognizes this at lines 28–35 but does not update validation to assert the exact matrix.

**Fix:**

- From the exact window under test, first call an allowed command through the same low-level primitive and require success.
- Then call the forbidden command and return an explicit `{resolved, error}` sentinel; fail if the primitive is absent or the call resolves.
- Assert an ACL-specific rejection message, distinct from D6’s generic Rust error. That prevents Rust from masking a broken capability layer.
- Use an observable canary and verify no state changed.
- Put popup cleanup in `finally`; do not wait 60 seconds on failure.
- Statically assert the complete exact mapping, not just union equality.
- Keep separate unit/integration evidence that every Rust handler applies its guard before state access.

There is also no existing real-WebDriver positive update-prompt flow: `show_update_prompt_window` is compiled out under `webdriver` at [windows.rs:130](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/src-tauri/src/windows.rs:130). The Playwright update tests are mocks. Add a controlled webdriver-only prompt so `respond_update_prompt` is positively tested from the real `update-prompt` label.

### 3. `connect-src` does not close off-origin exfiltration

**Claim attacked:** The CSP closes “off-origin-exfil” ([plan:112-116](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/implementations-plan/security-hardening/clusters/C10-plan.md:112)).

**Why wrong:** `connect-src` controls fetch/XHR/WebSocket/beacon-style connections. It does not prevent top-level navigation. The CSP also omits `form-action`; that directive does not fall back to `default-src`, so form submission remains unrestricted. [W3C CSP](https://www.w3.org/TR/CSP/)

All windows are created without `on_navigation` or `on_new_window` restrictions at [windows.rs:43](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/src-tauri/src/windows.rs:43). Compromised allowed bundle code can navigate to an attacker URL carrying data even though `fetch()` is blocked.

Additionally, Linux receives CSP through a `<meta>` policy, and `frame-ancestors` is ignored in meta-delivered CSP. [MDN frame-ancestors](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Content-Security-Policy/frame-ancestors)

**Fix:**

- Add `form-action 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'`.
- Add Rust `on_navigation` allowing only the local Tauri asset origin.
- Add `on_new_window` that denies new webviews unless explicitly required.
- WebDriver-test that remote navigation and form submission are rejected.
- Stop claiming CSP alone prevents all exfiltration.

## MEDIUM findings

### 4. D3’s mechanism is misstated, although the proposed IPC policy is viable

**Claim attacked:** Tauri auto-adds a bootstrap nonce and IPC origins to `connect-src` ([plan:43-47](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/implementations-plan/security-hardening/clusters/C10-plan.md:43)).

**Why risky:** Locked Tauri augments `script-src`/`style-src` using hashes and nonce tokens found in compiled HTML. It does not add `ipc:` or `http://ipc.localhost` to `connect-src`; those must be configured explicitly, as Tauri’s own example does. [Tauri CSP documentation](https://v2.tauri.app/security/csp/)

Tauri’s IPC initialization scripts are WebView initialization scripts, not ordinary page inline scripts requiring the claimed bootstrap nonce. Therefore:

- `script-src 'self'` is safe for the external relative bundles;
- the explicit `ipc: http://ipc.localhost` sources are required and correct;
- excluding `'self'` from `connect-src` is correct for the current frontend;
- external `style.css` should work under `style-src 'self'` on WebKitGTK.

The D4 inventory is complete for the current files: one markup `style`, four CSSOM mutations, no `<style>`, and no SVG style/presentation attribute requiring an exception. Assigning a function to `btn.onclick` from an allowed external script is not CSP inline-string execution.

**Fix:** Correct the rationale. Never fall back to `'unsafe-inline'` if the external stylesheet fails—it would not authorize a broken same-origin stylesheet path anyway. Fail the WebDriver computed-style test and fix the asset/CSP origin.

### 5. D6’s helper location cannot compile, and its command coverage is ambiguous

**Claim attacked:** Centralize `require_label` in `windows.rs`; require only “settings mutators” to use the settings label ([plan:61-65](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/implementations-plan/security-hardening/clusters/C10-plan.md:61)).

**Why wrong:** `commands.rs` belongs to the library crate ([lib.rs:11](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/src-tauri/src/lib.rs:11)); `windows.rs` is a binary-only module ([main.rs:5](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/src-tauri/src/main.rs:5)). Library commands cannot import a helper from the binary module.

“Settings mutators” also leaves the getter policy ambiguous. `get_config`, `get_autostart_enabled`, and `get_system_info` must be checked too if Rust is claimed as a complete independent layer.

**Fix:** Move labels, sanitizer, matchers, and `require_label` into a new exported library module used by both commands and window creation. Define an explicit 12-command mapping.

The injected `Window`/`WebviewWindow` itself is sound: Tauri obtains it from the native `InvokeMessage`, not the JS payload, so JS cannot spoof its label. Deriving the expected auth label from `request_id` is therefore sound.

Wrapping getters in `Result<T, String>` does not alter successful JavaScript values; existing `invoke()` callers already await promises. There are no internal Rust callers to update.

### 6. The auth label truncates an unguessable UUID to only 48 bits

**Claim attacked:** `sanitize_window_label` is “collision-free” and suitable for the request-binding boundary.

**Why risky:** It uses only six SHA-256 bytes at [commands.rs:158-165](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/src-tauri/src/commands.rs:158). That is not collision-free and unnecessarily reduces a UUID’s security margin.

**Fix:** Use `auth-{request_id}` directly—the UUID characters are valid Tauri label characters—or use the full hash. Then compare the exact full label.

### 7. D8 is directionally correct but overstates production parity

**Claim attacked:** `tauri dev` is equivalent enough that built-debug is unnecessary, and the release workflow is a production-mode backstop ([plan:70-77](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/implementations-plan/security-hardening/clusters/C10-plan.md:70)).

**Assessment:** `tauri dev` does enforce the configured CSP here: `devCsp` is absent, so `csp` is used, and there is no `devUrl`. ACL enforcement is not relaxed. The gate is not worthless. [Tauri SecurityConfig](https://docs.rs/tauri-utils/latest/tauri_utils/config/struct.SecurityConfig.html)

However, the “release” WebDriver path uses raw `cargo build --release` at [_e2e-webdriver.yml:56](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/.github/workflows/_e2e-webdriver.yml:56). Tauri’s `custom-protocol` feature is CLI-managed and not a default crate feature, so this release-profile binary still uses Tauri’s `cfg(dev)` configuration. It is not a true `tauri build` production-mode backstop.

**Fix:** Keeping dev for the three-OS PR matrix is acceptable, but statically forbid `devUrl` and a weaker `devCsp`. Add at least one built-debug production-mode lane if production-protocol parity or `freezePrototype` is relied upon.

### 8. The ignored bundle pipeline can embed stale or unreviewed trusted code

**Claim attacked:** Presence checks for three generated bundles are sufficient.

**Why risky:** Gitignored bundles are trusted by `script-src 'self'`. A presence-only check accepts stale output after frontend source or dependency changes. `@tauri-apps/api: "^2"` also allows a broad resolution range; the lockfile pins an install, but the plan does not require reviewing the lock/package or emitted bundle.

**Fix:** Pin an exact API version, review the lock diff, clean the output directory before building, and make `build.rs` reject outputs older than or inconsistent with sources/lock/build configuration. Ensure every direct Cargo CI path runs `frontend:build`. Scan output for dynamic imports, remote URLs, `eval`, and source-map references.

## Assumptions audit

### Facts

- Correct: current global, missing CSP, popup inline scripts, 12 handlers, labels, no static main window, and existing CI topology.
- Incorrect: `default.json` grants all windows its plugin/core permissions. With no target selectors, it matches no window.
- Misleading: `build.rs` is “bare.” It performs version and verified-sites validation; only its Tauri build invocation is bare ([build.rs:1-21](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/src-tauri/build.rs:1)).
- Misstated evidence: current WebDriver does not exercise all 12 commands. Framework source proves they are callable; “WebDriver passes” does not.
- Incorrect: there is an existing real update-prompt positive WebDriver flow.
- Incorrect: the release WebDriver path is a true Tauri production build.

### Inferences

- Promote to fact: `AppManifest::commands()` flips app commands to ACL-gated/default-deny for the locked versions.
- True: permission IDs are kebab-cased; `auth-*` matches the generated auth labels; bundled `invoke` delegates to `__TAURI_INTERNALS__.invoke`.
- False: Tauri auto-adds IPC origins to `connect-src`.
- False mechanism, correct outcome: no page-bootstrap nonce is required for Tauri initialization; `script-src 'self'` still works.
- True with invariants: dev mode enforces CSP and ACL here.
- False: `connect-src` closes all off-origin exfiltration.
- False: popup flows require `core:default`.

### Asks

- A1/D7 should no longer be open: **drop `core:default` and every raw plugin grant**.
- A2/D8: dev mode is acceptable if `devUrl`/weaker `devCsp` are statically forbidden; built-debug is production-parity hardening, not mandatory for basic ACL/CSP enforcement.
- A3 `freezePrototype`: surface as an explicit compatibility/security decision. If enabled, test it under a true custom-protocol build.
- A4: do not permit an `'unsafe-inline'` fallback. Any relaxation requires a separate reviewed security exception.

## Phasing consequence

P0–P2 are not less secure than today. P3 becomes less secure if D7 is retained because it activates broad core permissions. P3 must also land the manifest and complete capability matrix atomically; the ACL flip is all-or-nothing. P4 is safe after a corrected P3, but the final gate must independently prove ACL rejection so D6 cannot hide a capability regression.