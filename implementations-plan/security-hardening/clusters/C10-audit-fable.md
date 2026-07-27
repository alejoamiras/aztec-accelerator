# C10 / F-012 — Fable (Opus) deep-tier audit — VERDICT: CHANGES-REQUESTED

(Verified against Tauri 2.11.0 source, not just app behavior. Full transcript captured from the agent result.)

## Decisive findings
1. **Central inference CORRECT + source-proven** (webview/mod.rs:1793-1845; authority.rs:132,439-470; the
   current `gen/schemas/acl-manifests.json` has an EMPTY app manifest → app commands skip the ACL today).
   `AppManifest::commands()` sets `has_app_acl=true` → genuine default-deny flip. Two consequences:
   `has_app_manifest` is a GLOBAL switch (declaring ANY command gates ALL 12 at once → the set-equality static
   test is a HARD gate, not cleanliness); and the reject happens BEFORE dispatch (denial cannot execute side
   effect).
2. **(HIGH) Cross-window test cannot prove the ACL + can pass spuriously.**
   - Once P4 Rust labels land, the test greens even if the ACL does nothing → the ACL is provable IN ISOLATION
     only at the END OF P3, BEFORE any label code exists. Pin the ACL cross-window-denial proof to the P3 gate
     with ZERO label code present.
   - In debug builds (`tauri dev` AND `tauri build --debug`, both `debug_assertions` on), the ACL rejection
     message is the VERBOSE `resolve_access_message` form ("...not allowed on any window/webview/URL
     context..."), NOT "Command X not allowed by ACL" (that release string only appears at webview/mod.rs:1842).
     A test matching the release string never matches. Assert the DEBUG-form ACL/permission denial reason.
   - Reject-for-wrong-reason (arg deserialize, misspelled cmd) also satisfies "rejects". And the invoke-key
     pre-check (webview/mod.rs:1758) SILENTLY DROPS keyless messages BEFORE the ACL → a hand-rolled
     `window.ipc.postMessage` hangs and tests the wrong boundary. Test MUST invoke via the injected
     `__TAURI_INTERNALS__.invoke` / bundled `@tauri-apps/api/core` (they carry the key), first call an ALLOWED
     command and require success, then the forbidden one → assert ACL reason + prove no state changed via a
     follow-up allowed read from another window.
3. **(HIGH/MED) D8 — `tauri dev` NEVER exercises the shipped custom-protocol path.** `is_dev() ==
   !cfg!(feature="custom-protocol")` (lib.rs:308). `tauri dev` AND the `release` webdriver mode (bare
   `cargo build --release --features webdriver`, which does NOT enable `tauri/custom-protocol` — that's a
   CLI-only feature enabled by `tauri build`) both run `is_dev()==true`. `windows-build` does a real `tauri
   build` but does NOT run the WebDriver specs. ⇒ under the plan's D8 default, NO CI job runs the trust-boundary
   assertions against a production-path binary. dev DOES apply the real CSP (csp via `dev_csp.or(csp)`, real
   nonce path, `tauri://localhost` serving with no devUrl) and DOES enforce capabilities (compile-time) — so
   the gate is not worthless — but it rests on the fragile `dev_csp.or(csp)` + no-`devUrl` coincidence; a future
   `devUrl`/`dev_csp` line silently falsifies the CSP gate, and a stale on-disk `frontend/assets/*.js` can
   false-green. FIX: adopt `built-debug` (`tauri build --debug --no-bundle --features webdriver` → launch
   `target/debug/aztec-accelerator`) for the trust-boundary spec (release mode is NOT a substitute — still
   is_dev). Run just the trust-boundary spec under built-debug to control CI cost.

## Refinements
- **D3:** `script-src 'self'` VERIFIED safe (set_csp→replace_csp_nonce force-adds `'self'` + nonce for Tauri's
  injected scripts, manager/mod.rs:126-152). "auto-augments connect-src (ipc)" is FALSE — augments script/style
  ONLY; keep the explicit `ipc: http://ipc.localhost` (Windows needs http://ipc.localhost, Linux/mac need
  `ipc:` — BOTH required). Excluding 'self' from connect-src is correct. No `<style>` blocks in any page; the
  inline SVG (authorize.html:17-19) has only `d` (no presentation attrs) → D4 SVG-complete.
- **D4:** right action, WRONG justification. Only the ONE markup `style="display:none"` (settings.html:33) is
  CSP-governed; the runtime `.style.setProperty("--fill")`/`.style.display` (settings.html:96,112,129,132) are
  CSSOM mutations that `style-src` does NOT block (never needed unsafe-inline). Externalize via `[data-fill]`
  as harmless equivalence, NOT as a CSP requirement. Close A4 to `'self'` HARD (nothing left to justify
  unsafe-inline once the one attribute is gone).
- **D6:** auth label SOUND/not forgeable (JS can't spoof its window label; Tauri resolves Window from the
  native InvokeMessage; hash(B)!=A defeats an attacker in auth-<A> supplying request_id=B). PROBLEM 1: the
  predicate CANNOT live in windows.rs (bin, main.rs:5) — commands.rs is lib (lib.rs) and can't reference the
  bin; put `require_label(actual:&str, expected)->Result<(),String>` (pure) in commands.rs next to
  sanitize_window_label; windows.rs already imports it. PROBLEM 2: adding a WebviewWindow param makes commands
  un-unit-testable in isolation → only the PURE predicate is unit-testable; command-level ordering is
  WebDriver-integration territory (don't overclaim). Result-wrapping getters is transparent to JS/mock, but
  CHECK `main.rs mod tests` for direct Rust callers before assuming zero-breakage.
- **D7:** fable says retain-or-drop both safe (core:default has no window create/close). *(See adjudication —
  resolved to DROP; fable missed that core:default is currently INERT so "retain" = "newly activate".)*
- **Assumptions:** promote the ACL-flip inference to FACT; DELETE the "auto-augments connect-src" inference;
  A2→built-debug (only the CI-cost tradeoff is worth surfacing to the user, not the security choice); A4→close
  to 'self' hard. Supply chain: pin `@tauri-apps/api` to a 2.x compatible with tauri 2.11.0 (invoke_key
  protocol compat) + frozen lockfile + min-age; keep the build.rs missing-bundle guard (with has_app_manifest
  on, a missing bundle would embed HTML pointing at absent scripts).
- **Could NOT confirm (need a real build/run):** exact generated perm-id spelling; runtime `auth-*` glob match
  vs `auth-<hash>`; WebKitGTK render under style-src 'self'; whether `main.rs mod tests` calls command fns
  directly.

## Reconciliation of the two legs (my adjudication of the divergences)
- **D7 (codex DROP vs fable retain) → DROP.** Codex's analysis is more precise: `capability.rs:162-163` +
  `authority.rs:459-460` — empty capability selectors match NO window, so today's `core:default`/autostart/
  process grants are INERT (source-verified by me). Retaining core:default in the NEW per-window capabilities
  therefore NEWLY ACTIVATES a broad core API (event emit, window/webview enumeration, path, image/rgba,
  resource close, menu/tray) that is dormant today — a privilege INCREASE, not the status quo fable assumed.
  BOTH legs agree the frontend needs no core/event/window JS API (windows close from Rust; no event listeners).
  ⇒ DROP core:default + ALL plugin grants; each capability grants ONLY its window's app commands; add back a
  minimal specific perm (e.g. core:window:allow-close) to the exact window ONLY if the positive WebDriver suite
  proves it needed.
- **D8 (codex built-debug-optional vs fable built-debug-required) → built-debug REQUIRED for the trust-boundary
  spec.** Fable's `is_dev()==!custom-protocol` trace (lib.rs:308) is decisive and neither codex nor my own read
  caught it: the existing `release` webdriver mode is ALSO is_dev()==true, so NO current CI job exercises the
  production custom-protocol path. Resolution = BOTH: (a) add a `built-debug` mode to `_e2e-webdriver.yml`
  running `tauri build --debug --no-bundle --features webdriver` and launch `target/debug/aztec-accelerator`,
  run at least the trust-boundary spec under it (Linux, to bound CI cost); (b) KEEP codex's static guards
  forbidding `devUrl` and a weaker `dev_csp`; (c) pin the ACL-isolation proof to the P3 gate (pre-label);
  (d) assert the DEBUG-form ACL denial reason so ongoing CI attributes denials to the ACL, not the label.
- **auth-label width (codex widen vs fable sound):** both right on different axes (fable: unforgeable; codex:
  6-byte/48-bit truncation isn't collision-free). Resolution: keep hashing (handles arbitrary request_id
  charset — the reason sanitize_window_label exists) but WIDEN to ≥16 bytes (128-bit) so collision margin is a
  non-issue; compare the exact full label. P4 refinement, not a blocker.

Both audits CHANGES-REQUESTED; the plan's direction is sound and the central inference is source-proven. The
folds are test-design + phasing + least-privilege + CI-path hardening — no architectural reversal.
