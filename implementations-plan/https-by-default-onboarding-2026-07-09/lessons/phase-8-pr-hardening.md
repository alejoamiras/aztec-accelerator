# Phase 8 — PR hardening (merge onto security-hardening + codex bug-hunt fixes + certainty tests)

Re-engaged 2026-07-27. Three inputs drove this phase: (1) `main` had moved 23 commits ahead (the
183-file security-hardening campaign), (2) three PR-scoped codex `gpt-5.6-sol` bug-hunts had found real
bugs, (3) the owner asked to increase install certainty across Ubuntu/Windows/macOS. Owner decision:
**no migration** — treat HTTPS-by-default as a clean install (drop the `safari_support` serde alias).

## Merge (origin/main → branch)

A read-only Sonnet analyst mapped the divergence first. Beyond the 4 textual conflicts (accelerator.yml,
settings.html, commands.rs, index.md) main's campaign silently broke the feature in ways the compiler
wouldn't catch:

- `windows.rs` gained a required `focus_on_create` field → our two new WindowConfig literals wouldn't compile.
- The Tauri capability model became an explicit per-window allowlist → our `onboarding`/`renewal`
  windows and renamed `enable_https`/etc. commands had ZERO IPC grants (runtime-only failure).
- Every command now takes `require_label(window, LABEL)` (F-012) → ours needed it retrofitted.
- Frontend moved to bundled ES modules + strict `script-src 'self'; style-src 'self'` CSP → our inline
  `<script>`/`<style>` in onboarding/renewal/settings were dead. Ported to `frontend-src/*.js` +
  external CSS; windows now close from Rust (no core:window grant).
- `crash_recovery` API changed to Result/bool → `set_autostart_inner` rebuilt from main's hardened body.
- `build.rs` hard-codes the bundle list + F-012 command surface; `tauri-trust-boundary.test.ts` pins the
  window/command/capability set — both updated to the 5-window, 20-command topology.

Merge commit audited by codex (`gpt-5.6-sol@xhigh`): verdict "sound, no lost hunks", one Low
(native `window.close()` failures swallowed) → fixed (they now propagate so wireButton re-enables).

## codex bug-hunt fixes (all folded)

SDK: `#isHealthy` + prover now require `status:"ok"` + `api_version:1` (mirror `probe.rs`), enforced in
`httpsOnly` too; `/health` body read once under a deadline+cap (no `clone().json()` hang);
config-generation guard + single-flight probes (no stale repin); pinned-HTTPS `/prove` network failure
demotes to HTTP once then WASM (`httpsOnly` → WASM); monotonic cache clock; honest `httpsOnly` docs.

Trust: macOS `verify-cert -l -L` (a CA is rejected as a leaf without `-l` → launch gate false-negative)
+ `delete-certificate -t`; Windows live-cert identity by **serial** (CN reserved for delete-all); Linux
ancestor-walk safe-path guard, `-V -u L` trust validation (not bare presence), tri-state removal;
removal failures now propagate (Settings `Err`, CLI non-zero exit).

Enable path: `enable_https` awaits the real bind before persisting (spawn_https returns a oneshot); the
"already trusted" short-circuit spawns when unbound; opt-out persists `https_enabled=false`;
`certs_exist()` also requires the leaf+key load into rustls (recovers a mixed swap); renewal restarts to
serve the new leaf; the launch HTTPS gate runs on a blocking task so a hung trust query can't block HTTP.

## Certainty tests

- **Tier 1 (shipped, runs on all 3 OSes)**: `tests/tls_handshake.rs` — in-process rustls handshake
  against the real generated cert set, verified by both `localhost` and `127.0.0.1`. Proves leaf/key
  match + chain-to-CA + loopback name constraints + webpki acceptance, with no OS store or browser.
  Wired into the `cert-trust` matrix (`cargo test --test tls_handshake`) so cert-gen regressions fail
  fast on macOS + Windows too.
- **Tier 4 (shipped)**: static guard in `tauri-trust-boundary.test.ts` pinning the NSIS
  `${If} $UpdateMode <> 1` guard around the uninstall `-delstore Root` (audit R1 — an auto-update must
  never wipe trust).
- **Existing coverage that already addresses "browser trusts after install"**: the `cert-trust` Linux
  leg installs the CA into `~/.pki/nssdb` and chain-validates a leaf via `certutil -V` — NSS is exactly
  Chrome/Chromium's verification path, so this is the browser-trust mechanism, minus the pixels.

### Deferred: Tier 2 (real-browser E2E, 3 OSes) + Tier 3 (Windows certutil command-shape)

NOT shipped, deliberately — I can't verify browser automation blind on macOS/Windows from a Linux box,
and a broken-blind CI job downgrades quality (the owner's explicit bar). Recommended next increment,
with the exact approach:

- **Tier 2**: per-OS, seed the CA into a store the browser trusts *silently on a CI runner* (Linux:
  `~/.pki/nssdb`, already done by `install_ca_trust`; macOS: `security add-trusted-cert -d` into the
  **admin/system** keychain — silent with the runner's sudo; Windows: `certutil -addstore Root` into the
  **LocalMachine** Root — silent when elevated, unlike the CurrentUser Root dialog). Start the real
  HTTPS server (a tiny harness bin serving `load_rustls_config()` on 59834), then drive Playwright
  `chromium` to `https://127.0.0.1:59834/health` and assert `200` + JSON with no cert error. The
  LocalMachine/admin-store seeding is the unlock that makes this headless-runnable (the CurrentUser
  paths prompt).
- **Tier 3**: a Windows-only test exercising the `certutil` argv shapes against the non-prompting
  `CurrentUser\CA` store (add/verify-by-serial/remove) to catch arg-shape drift without the Root dialog.

The owner will smoke the two consent dialogs (Windows CurrentUser-Root prompt, macOS Keychain password)
+ uninstall on real machines — the irreducibly-manual bit.

## Round-2 codex (ultra Rust + max SDK), PR-scoped — folded

A second PR-scoped pair of hunts (one at **ultra**) found refinements to the round-1 fixes:

**SDK (all fixed + regression-tested):**
- `/prove` demotion is now generation-aware + per-request: a mid-proof `setAcceleratorConfig` no longer
  lets the retry POST the witness to the new endpoint (High); concurrent HTTPS failures each fall back
  on their OWN attempted scheme instead of a shared demote-gate that left the 2nd caller rethrowing.
- prefer-HTTPS grace gates on the health CONTRACT (`#isHealthy`), not just `response.ok`, so a foreign
  fast HTTP can't beat a healthy-but-slower HTTPS.
- `readJsonBounded`: a deadline-cancelled PARTIAL body is no longer parsed as healthy (`timedOut`
  flag); over-cap `cancel()` is fire-and-forget (no hang); the bodyless fallback clears its timer +
  measures encoded bytes.

**Rust (fixed):**
- Linux `certutil` safe-path guard now checks OWNERSHIP (root or self) on the canonical binary + every
  ancestor, not just mode bits — an attacker-OWNED `0755` tree is rejected (`libc::geteuid`).
- `renew_cert` only restarts when NO proof is in flight (try-acquires the prove permit and holds it
  across the diverging restart) — Tauri's `exit(0)` restart would otherwise orphan `bb` + leave the
  witness temp dir; if a proof is running it defers (old leaf valid ~30 more days).
- Linux rotates the expiring leaf BEFORE loading/binding TLS, so the fresh leaf is served this session
  instead of the acceptor holding the old one until it expires.
- `enable_https_inner` treats `https_bound == true` as success even if its own spawn lost the bind
  race with the launch gate — no more persisting `https_enabled = false` while HTTPS is actually live.

## Round-3 (convergence re-audit of the round-2 fixes) — folded

Both sessions were resumed on the round-2 diff to check the fixes AND whether the fixes introduced new
bugs. Both confirmed the round-2 fixes sound, and each found one more real race:

**SDK (High, fixed):** the generation snapshot didn't bind the *health result* or the *initial* POST
URL. Two live paths still sent the witness to an unprobed endpoint: (a) a probe that started against A
and completed after `setAcceleratorConfig(B)` had its commit discarded but still returned
`available:true`, and the prove then targeted B; (b) an `onPhase("serialize"|"transmit"|"proving")`
callback calling `setAcceleratorConfig(B)` — the initial `postProve()` read the *mutable* `baseUrl`
after the callbacks. Fixed: `createChonkProof` captures the generation BEFORE probing and re-checks it
after; `#proveRemote` takes that generation, snapshots BOTH prove URLs before any callback runs,
re-checks the generation after the callbacks, and POSTs to the snapshot (initial AND retry). Also
(Low) an endless stream of ZERO-LENGTH chunks starved the `setTimeout` deadline — added an in-loop
wall-clock check that doesn't depend on the timer firing.

**Rust (High, fixed):** the Linux ownership check was bypassable via a replaceable SYMLINK —
`certutil_bin()` returned the *original* path while validation canonicalized the target, so a
foreign-owned symlink currently pointing at `/usr/bin/certutil` passed and could be re-pointed before
`Command::new`. Fixed with `safe_canonical()`: validate, then return and execute the CANONICAL path.

**Rust (Medium, fixed):** the listener race was narrowed but not closed — both the launch gate and the
enable path could sample `https_bound == false` and each spawn. Fixed properly with a compare-and-swap
on a new `https_starting` atomic in `HeadlessState`: `spawn_https` returns `Option<Receiver>` and only
ONE attempt ever runs; a caller that loses the CAS waits on `https_bound` via the bounded
`wait_for_https_bound` (~3s, exits early if the owner released without binding) instead of concluding
failure. This also removes the AddrInUse path that made the rotate-before-serve race reachable.

**Accepted residuals (documented, not fixed — all degrade gracefully):**
- *macOS removal query-error fails open*: a `find-certificate` that ERRORS mid-removal-postcheck (vs
  "not found") can report removed. The login keychain is essentially always unlocked while the app
  runs, and the delete-FAILURE path IS covered; the query-error residual is low. (The CLI conflates
  "not found" and "error" in its exit code, so a clean fix isn't possible.)
- *Windows presence ≠ effective trust (Disallowed store precedence)*: a serial in Root is trusted
  UNLESS the same cert is also in CurrentUser\Disallowed. Self-inflicted; and the SDK's HTTP fallback
  covers the served-untrusted-leaf case. Not worth an unverifiable-on-Linux Windows chain-policy call.
- *Linux partial-store renewal*: rotation succeeds if ANY NSS store accepts the new anchor; a store
  that was trusted but temporarily unavailable during the ~2-year rotation could reject the new chain
  next launch. Recoverable via "Remove certificate trust" + re-enable. Full prior-coverage tracking
  wasn't worth the risk to the rotation path.
