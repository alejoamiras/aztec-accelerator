# Recon — accelerator GUI app (post-2.0.0)

## Crates & shape

| Crate | LOC | Role |
|---|---|---|
| `src-tauri` (`aztec-accelerator`, bin `AztecAccelerator`) | 12,291 | Tauri GUI layer: startup sequencer, windows, tray, updater, certs+trust, autostart, crash recovery, uninstall, update marker |
| `core` (`accelerator-core`) | 12,331 | GUI-agnostic: HTTP server (router/host-guard/auth/prove/probe/owner), origin authorization, config store, bb runner, version policy/downloader/cache/leases, update-manifest verify, updater state, win_acl |
| `server` (`accelerator-server`) | 260 | Headless binary (OUT of scope) |

Dependency direction: `server → core ← src-tauri`. Core is build.rs-free; versions injected by callers.

## Key surfaces

- **Listeners**: HTTP `127.0.0.1:59833` (`core/src/server.rs::start`, bind_with_retry 5s);
  HTTPS :59834 (`src-tauri/src/server/tls.rs`, rustls).
- **Ingress guards**: loopback Host+port exact-match (`core/src/server/host.rs`) → CORS → routes;
  Origin canonicalization RFC 6454 (`core/src/authorization.rs::CanonicalOrigin`);
  approval decision (`core/src/server/auth.rs`; persisted allowlist, localhost prompt-once,
  60s popup timeout + queue backstop, deny cooldown).
- **Consent popup**: `windows.rs` lifecycle (queued popups, activation-armed auto-deny) +
  `frontend-src/authorize.js`; click-steal guard in `bridge.js` (700ms); `[Deny][Allow]`
  permanent-only consent (F-014 reversal, PR #421).
- **Certs/trust**: keyless local CA name-constrained to loopback (`certs.rs`: staging set +
  atomic swap, legacy ca.key deletion); trust backends `trust/{macos,windows,linux}.rs`
  (Keychain `security` CLI / certutil CurrentUser Root / NSS DBs); renewal consent window
  (mac/win) vs silent rotation (Linux).
- **Updater**: pinned Ed25519 pubkey from tauri.conf.json; Layer A signed manifest + Layer B
  monotonic floor (`updater.rs`, core `update_manifest.rs`+`updater_state.rs`); pre-flight size
  cap; Windows NSIS update-window marker (`update_marker.rs`: nonce+deadline+path match);
  publish/promote split feed (B6).
- **bb execution**: runtime download → cache (`versions/*`: policy/downloader/cache_layout/
  leases) → worker with bounded stderr + proof validation + tree-kill (B3 #447); digest
  verification fail-closed; Windows checksum pin TOFU residual.
- **Config**: `~/.aztec-accelerator/config.json` via ConfigStore persist capability + schema
  migration + cross-process write lock (B4 #451); win_acl owner-only DACLs (402 LOC, zero tests).
- **Lifecycle**: autostart self-heal resolve-based (`autostart.rs` 2,838 LOC, replaced
  tauri-plugin-autostart); crash recovery per-OS (`crash_recovery.rs`); ownership-checked
  uninstall (`uninstall.rs`, stored-entry-as-oracle); binary rename boundary
  (`AztecAccelerator`, legacy-exe prune #455); publisher flip.

## v2-delta since last audit snapshot (9c4cb0c..HEAD)

28 files, +6,842/−265 on app source. Fix arcs #434–#442 (the 10 claimed-fixed findings —
never independently re-reviewed) + v2 train: #446 consent hardening, #447 bb containment,
#448 real uninstall, #451 config migration+lock, #452 packaged-E2E gate, #455 legacy-exe prune,
#465 version bump. **None of this delta has been adversarially audited.**

## Test surface (facts)

431 Rust tests (413 non-ignored; src-tauri 155 / core 265 / server 11); 18 ignored real-OS
integration run per-OS in CI; 17 WebDriver E2E ×3 OSes (+ linux built-debug lane); 66 Playwright
UI tests ubuntu-only; packaged-E2E draft gate (linux/macos composed proof, windows uninstall,
1.0.7→2.0.0 migration linux-only). Zero-test modules: win_acl.rs, core/server.rs router assembly,
server/auth.rs (indirect only), tray.rs, tls.rs, trust/macos.rs, trust/stub.rs.
