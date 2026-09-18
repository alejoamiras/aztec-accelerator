Verdict: **CHANGES-REQUESTED**

No HIGH findings. One real trust-boundary defect must be fixed before merge.

### MEDIUM

1. Navigation guard accepts URLs outside the platform’s actual asset origin

[windows.rs:14](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/src-tauri/src/windows.rs:14)

`is_local_asset_url` unconditionally accepts both:

- `tauri://localhost` on every OS
- `http://tauri.localhost` on every OS

It also ignores ports and credentials. On Linux/macOS, `http://tauri.localhost:<port>/...?data=...` is not Tauri’s embedded-asset protocol; it is a loopback HTTP navigation. If a process is listening there, compromised frontend code can exfiltrate config, authorization request IDs, or origin data despite the intended anti-navigation control. The current test explicitly allows both platform forms and has no port/credential cases.

On Windows, userinfo such as `http://user@tauri.localhost:<port>/` is also accepted by this matcher even though it can interact differently with Wry’s `http://tauri.*` protocol interception.

Specific fix:

- Select the permitted scheme/host using `#[cfg(windows)]` versus `#[cfg(not(windows))]`.
- Require no non-default port, username, or password.
- Add rejection tests for the other platform’s origin, `:59833`, and userinfo.
- Add a real-WebView assertion that attempted top-level navigation remains on the original page.

### LOW

2. The staleness guard does not authenticate or even fingerprint the bundle it ships

[build.rs:23](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/src-tauri/build.rs:23)  
[build-frontend.ts:82](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/scripts/build-frontend.ts:82)

The manifest records only hashes of `frontend-src/*.js`. `build.rs` checks that output files exist, but never hashes their contents.

Concrete false pass:

1. Run `frontend:build`.
2. Replace `frontend/assets/settings.js` with an old or injected bundle.
3. Run raw `cargo build`.
4. `build.rs` reruns but passes because the source hashes still match and the replaced output exists.

Changes to `bun.lock` or `@tauri-apps/api` can similarly leave an old dependency bundle accepted by raw Cargo. Hashing inputs only after `Bun.build` also leaves a source-change race.

Official CI and Tauri builds mitigate this by rebuilding first, but this defeats the guard’s stated protection for raw Cargo paths.

Specific fix:

- Record and verify hashes for the three emitted bundles.
- Include `package.json` and root `bun.lock` among tracked inputs.
- Hash inputs before and after bundling, failing if they changed.
- Validate the manifest schema/algorithm and exact output set. Use SHA-256 if the manifest is described as detecting malicious substitution; a self-authored ignored manifest cannot protect against an attacker able to replace both files.

3. Parts of the WebDriver “proof” can pass without proving the claimed property

[trust-boundary.spec.ts:151](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/e2e-webdriver/trust-boundary.spec.ts:151)  
[trust-boundary.spec.ts:160](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/packages/accelerator/e2e-webdriver/trust-boundary.spec.ts:160)

Three gaps:

- The off-origin fetch uses an unreachable test hostname and treats every rejection as CSP enforcement. DNS, TLS, or CORS failure produces the same result even with permissive `connect-src`.
- The test never exercises `on_navigation` or `on_new_window`, which is why the MEDIUM defect above escaped it.
- The supposedly “REAL, mutating” command removes an origin explicitly known to be absent. It proves dispatch, not mutation.

Specific fix:

- Require a `securitypolicyviolation` event filtered to `effectiveDirective === "connect-src"` and the expected blocked URI, preferably with a reachable canary endpoint.
- Attempt `location` navigation and `window.open`, then assert URL, DOM, handle count, and granted IPC remain intact.
- Seed and remove a real approved origin, or use `set_speed` with restoration, to demonstrate an actual mutation.

### Areas that are sound

- **Legitimate flows:** Window construction labels exactly match the Rust guards: `settings`, `update-prompt`, and `auth-${sha256(request_id)[0..16]}`. Tauri’s `Result` serialization still resolves getters to their raw values. Module scripts in `<head>` are deferred, so DOM timing remains valid.
- **ACL coverage:** The 12 AppManifest commands, 12 `generate_handler!` entries, and capability union are set-equal. No window receives another window’s command. `respond_auth` remains bound to the exact request ID; the 128-bit label widening introduces no stale-width assumptions.
- **Plugin regressions:** There is no `updater:default`, `process:default`, or raw autostart grant. F-004, F-010, SEC-06, and F-014 remain preserved.
- **CSP:** Apart from navigation being a separate defective layer, the policy fits the actual frontend: self-hosted modules/CSS work, inline SVG is unaffected by `img-src`, and `ipc: http://ipc.localhost` is Tauri’s expected IPC allowance. No frontend code needs inline style, eval, remote images, workers, frames, or fetch.
- **New-window denial:** The pinned Wry implementation handles `NewWindowResponse::Deny` on WebKitGTK, WKWebView, and WebView2.
- **CI wiring:** The setup action builds bundles for desktop Cargo paths. The built-debug lane invokes real `tauri build --debug`, launches `target/debug/aztec-accelerator`, runs the trust spec, and is included in the aggregate status job.
- **ACL error assertion:** `"not allowed"` is generated by Rust’s Tauri ACL layer, so it is not WebView-engine-specific; debug and release forms both contain that wording.

Verification performed: all 22 Accelerator Bun unit/static tests passed, including all 12 F-012 drift tests; the four focused compiled Rust label/navigation tests also passed.