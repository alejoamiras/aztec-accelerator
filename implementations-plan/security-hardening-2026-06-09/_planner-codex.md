**Implementation Plan**

Ship this as 5 PRs, in this order. Do not release until all 5 are merged, `accelerator.yml` + `sdk.yml` are green, WebDriver E2E is green, and `/harden security` is rerun on the final aggregate commit.

1. **PR 1: Localhost ingress/auth hardening + headless default change + `/health` shaping**  
Scope: SEC-01, SEC-04, SEC-05. This is the highest-risk PR and the only intentionally breaking server-behavior change besides SEC-06.  
Code shape: add a global ingress check that validates `Host`/authority before any route logic, with exact allowlist `{127.0.0.1, localhost, [::1]}` and exact listener port match. Make it listener-specific by changing `router(state)` into `router_for_port(state, expected_port)` and using `59833` from HTTP startup and `59834` from HTTPS startup. Reject missing `Host`, non-loopback hosts, alternate numeric forms, trailing-dot/variant hosts, and wrong-port hosts with `400 invalid_host`.  
Code shape: refactor `/prove` auth into “validated loopback host first, then origin auth”. If `Origin` is present, canonicalize and run the existing approval flow. If `Origin` is absent, only allow the request after the loopback-host check; that preserves curl/Node/local-script callers and closes the DNS-rebinding same-origin bypass. Replace `prove_skips_auth_when_no_origin_header` with `prove_allows_no_origin_only_with_trusted_loopback_host`, and add the negative forged-Host regression.  
Code shape: headless startup in `packages/accelerator/server/src/main.rs` should have three explicit modes: `allow_all` (`--allow-all` or `ACCEL_ALLOW_ALL=1`), `allowlist` (`ALLOWED_ORIGINS=...`), and default gated mode (`auth_manager=Some`, empty config). In default gated mode, non-localhost browser origins are denied; localhost origins and loopback no-Origin callers still work. I would make `--allow-all` and `ALLOWED_ORIGINS` mutually exclusive and fail loud on both.  
Code shape: keep `/prove` CORS permissive. Tightening `/prove` CORS would break the MetaMask-style approval flow because unknown origins must be able to preflight and reach the auth gate. Instead, fix SEC-05 by splitting `/health` behavior: public cross-origin `/health` returns only minimal liveness (`status`, `api_version`), while detailed fields (`version`, `aztec_version`, `available_versions`, `bb_available`, `https_port`, debug runtime) are only returned to approved origins or loopback no-Origin callers.  
Code shape: resolve SEC-04 by not narrowing localhost auto-approve by default. After the Host fix, the remote-web attacker path is gone; the residual is local-foothold. Preserve zero-config localhost for the playground/dev dApps, but add an explicit opt-out: `auto_approve_localhost: bool` in config, default `true`, plus a headless env override and a desktop Settings toggle. When `false`, localhost origins fall through normal approval on desktop and are denied unless allowlisted in headless.  
Tests: add Host parser unit tests for `127.0.0.1`, `localhost`, `[::1]`, wrong port, `0.0.0.0`, decimal IPs, hex IPs, trailing dot, and missing Host. Add `/prove` tests for forged `Host: evil.com:59833`, forged `Host: 127.0.0.1:59834` on the HTTP listener, localhost origin still auto-approved, unknown origin still pops auth, and headless deny-by-default for non-localhost browser origins. Add `/health` tests for public-minimal cross-origin response and detailed local/approved response. Add a small SDK unit test that a minimal OK `/health` body still yields `available: true`.

2. **PR 2: Per-request auth IDs across core + Tauri popup**  
Scope: SEC-06.  
Code shape: change `AuthorizationManager` from “pending keyed only by canonical origin” to two maps: `origin -> request_id` for piggybacking and `request_id -> { origin, senders }` for resolution. `request()` should return `(receiver, request_id, is_first)`. `request_id` should be opaque and unguessable, not an incrementing counter.  
Code shape: change the popup callback path from `show_auth_popup(origin)` to `show_auth_popup(origin, request_id)`. Pass both in the popup URL. Change `respond_auth(origin, allowed, remember)` to `respond_auth(request_id, origin, allowed, remember)`. Resolve only by `request_id`; keep `origin` only for display, verified-site lookup, and window labeling.  
Code shape: timeout cleanup in `windows.rs` should resolve by `request_id`, not origin.  
Tests: add manager tests for “same origin piggybacks same request_id”, “wrong request_id cannot resolve another request”, and “stale request_id no-ops”. Update Playwright popup tests to assert `requestId` is passed through. Existing WebDriver auth-flow tests should stay green and become the end-to-end regression for the changed IPC contract.

3. **PR 3: Updater artifact size caps**  
Scope: SEC-03.  
Code shape: extend `latest.json` generation in `release-accelerator.yml` so each platform object includes a `size` field in bytes. Tauri’s updater already preserves extra JSON in `Update.raw_json`, so the app can read this without forking the feed format.  
Code shape: stop relying on `update.download()` as the only control point. In `src-tauri/src/updater.rs`, add a repo-owned download path that reads `update.download_url`, expected `size` from `update.raw_json`, and a hard ceiling constant. Stream with a running byte cap, reject if declared size is missing/invalid, reject if declared size exceeds the ceiling, reject if `Content-Length` disagrees upward, and reject if streamed bytes exceed the smaller of `{feed size, ceiling}`. Then verify minisign locally and pass bytes to `update.install(bytes)`.  
Code shape: expose the updater pubkey to Rust at build time from `tauri.conf.json` rather than duplicating it manually.  
Tests: unit-test JSON size extraction for static platform format, reject missing/oversized size, and add an integration test with a local HTTP server that advertises/streams an oversized artifact and proves the app aborts before full buffering. Keep the release updater smoke jobs unchanged except for asserting generated `latest.json` has nonzero `size` values.

4. **PR 4: `bb` extraction caps + SEC-02 deferred tracking**  
Scope: SEC-07 and the SEC-02 tracking note required by the locked decision.  
Code shape: wrap the gzip/tar reader with a decompressed-byte limiter, and separately reject `bb` tar entries whose declared uncompressed size exceeds a constant cap. Do not trust the compressed 64 MB cap alone.  
Code shape: keep the current digest-fetch model, but strengthen the TODO at `fetch_github_asset_digest` to explicitly say the online digest is channel-circular, nightly digests must not be pinned in-app, and the real fix is upstream publisher signatures once Aztec signs `bb`. Add a matching GitHub issue / roadmap note after merge.  
Tests: add extraction tests for “declared uncompressed size too large”, “decompressed stream exceeds cap”, and “normal tarball still extracts”. No behavior change to the current download URL or digest flow.

5. **PR 5: macOS cert cleanup and rotation hardening**  
Scope: SEC-08, SEC-09.  
Code shape: make `migrate_legacy_ca_key()` return a result instead of logging-only. Retry deletion, then re-check the path. If `ca.key` still exists, treat that as a security failure, not best-effort success.  
Code shape: on startup, if legacy `ca.key` cleanup fails, do not start Safari HTTPS. Surface a visible warning and keep HTTP working. Do not silently proceed with a trusted anchor plus a readable CA private key.  
Code shape: change cert rotation to stage a full cert-set directory, verify that the staged CA is trusted and that the staged leaf chains to that staged CA, then atomically switch the “current cert set” pointer. The cleanest implementation is a versioned cert-set directory plus an atomic `current` symlink swap, not 3 file renames.  
Tests: add unit tests for “cleanup failure leaves `ca.key` present and returns error”, “mismatched staged leaf/CA is rejected before swap”, and “atomic current-pointer swap keeps the live set internally consistent”. Add a macOS manual smoke step for Safari enable/rotate because Keychain trust and `security verify-cert` are not well covered by Linux CI.

**SEC-01 / SEC-04 Resolution**

SEC-01 must be fixed at the network boundary with exact loopback `Host` validation. That is the real DNS-rebinding fix.

SEC-04 should not be “fixed” by removing localhost auto-approve by default. That would break the playground and local dev dApps for a residual attacker class that is no longer remote-web after SEC-01. The right move is: keep default localhost auto-approve, add an explicit hardening knob, and document the residual local-foothold risk.

**Test Plan**

Every request-building Rust test under `packages/accelerator/core/src/server.rs` will need a trusted `Host` header once ingress validation exists. Add a small test helper so this is a one-time cleanup, not repetitive churn.

The key new regressions are:
- forged-Host DNS rebinding block
- no-Origin only allowed with trusted loopback Host
- headless default denies non-localhost browser origins
- `/health` public shape is minimal cross-origin but still detailed for local/approved callers
- popup resolution is by opaque request ID, not raw origin
- updater aborts oversized artifacts before full buffering
- tar extraction rejects oversized decompression
- Safari HTTPS refuses to run when legacy `ca.key` persists
- staged leaf must chain to staged CA before rotation

Current CI impact:
- `accelerator.yml` headless smoke should still pass unchanged if no-Origin loopback callers remain allowed.
- `_e2e.yml` / `sdk.yml` should still pass unchanged for the same reason; those Bun/Node callers are local and no-Origin.
- WebDriver auth flow should keep passing after the request-ID change once the popup frontend is updated.
- If you choose to deny no-Origin `/prove` across the board, `_e2e.yml` and SDK E2E will break. I would not do that.

**Migration & Docs**

Update `packages/accelerator/README.md`, the `//!` doc in `packages/accelerator/server/src/main.rs`, and the release-notes block in `.github/workflows/release-accelerator.yml`.

The headless docs need to say:
- unset `ALLOWED_ORIGINS` no longer means “open to everyone”
- default is localhost-only / loopback-only behavior
- `--allow-all` or `ACCEL_ALLOW_ALL=1` restores today’s open mode
- `ALLOWED_ORIGINS` remains the recommended explicit allowlist for browser CI
- non-browser local callers are still not a safe production exposure story

I would only touch `CLAUDE.md` if you want that contributor summary to mention the new headless default. Nothing there currently requires it.

**Security / Adversarial Considerations**

After PR 1, the attacker’s next move is local foothold, not DNS rebinding. Exact-host allowlisting must reject `[::ffff:127.0.0.1]`, decimal/hex IPs, `0.0.0.0`, missing Host, and wrong-port Host values. Do not rely on `User-Agent` or Fetch Metadata; Safari support is too uneven.

After PR 1, cross-site `/health` probing should still reveal only “accelerator exists” at most. That residual fingerprint is intentional because the SDK, landing page, and playground rely on zero-config detection. The version/cache/HTTPS-state leak is what needs to die.

After PR 2, an IPC caller can no longer resolve another request just by knowing the origin. The residual attacker is only a code path that can already invoke trusted Tauri commands and steal the opaque request ID.

After PR 3, the updater attacker moves from memory-DoS to signed-artifact compromise. That is acceptable here because the minisign trust model is the intended control; the fix is about availability, not authenticity.

After PR 4, the `bb` attacker still wins if upstream GitHub is compromised because SEC-02 is deferred. Make that debt explicit in code and in a tracking issue.

After PR 5, the remaining cert risk is operational: macOS trust prompts and on-disk layout migration. That is why I would keep this PR last and require a manual macOS smoke pass.

**Assumptions**

**Facts**
- `/prove` currently fails open on absent `Origin` and headless open mode is `auth_manager: None` in [packages/accelerator/core/src/server/auth.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/core/src/server/auth.rs:14) and [packages/accelerator/server/src/main.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/server/src/main.rs:41).
- Global wildcard CORS and detailed `/health` are in [packages/accelerator/core/src/server.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/core/src/server.rs:193).
- Localhost auto-approve is unconditional today in [packages/accelerator/core/src/authorization.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/core/src/authorization.rs:212).
- Popup resolution is raw-origin keyed today in [packages/accelerator/src-tauri/src/commands.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:107).
- The updater currently buffers then verifies in [packages/accelerator/src-tauri/src/updater.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/updater.rs:60).
- `bb` digest fetch TODO site is [packages/accelerator/core/src/versions/mod.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/core/src/versions/mod.rs:282).
- Legacy `ca.key` cleanup and staged rotation are in [packages/accelerator/src-tauri/src/certs.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/certs.rs:178) and [packages/accelerator/src-tauri/src/certs.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/certs.rs:291).

**Inferences**
- The SDK’s browser `/prove` path should send `Origin` because it uses `ky.post(...)` from page JS, but this is not pinned by a browser E2E today.
- The playground/dev flow relies on localhost zero-config based on docs and architecture, but there is no explicit end-to-end regression that asserts “localhost never prompts”.

**Asks**
- I recommend shipping both `--allow-all` and `ACCEL_ALLOW_ALL=1`, and treating them as incompatible with `ALLOWED_ORIGINS`.
- I recommend implementing SEC-04 as a configurable opt-out, not a default narrowing.
- I recommend the public cross-origin `/health` shape be exactly `{"status":"ok","api_version":1}` unless you want one more field for compatibility.