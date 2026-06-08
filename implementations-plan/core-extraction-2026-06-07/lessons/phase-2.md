# Phase 2 — Repoint headless onto core (the Tauri drop)

Base: `feat/core-extraction-phase-2` off `99c3250` (#325). Three edits:
- `server/Cargo.toml`: `aztec-accelerator = { path = "../src-tauri" }` → `accelerator-core = { path = "../core" }`.
- `server/src/main.rs`: imports `aztec_accelerator::` → `accelerator_core::`.
- `server/src/main.rs`: added `bundled_version: std::env::var("AZTEC_BB_VERSION").ok()` — the Phase-3 CI hook
  sets that env from the copy-bb.ts `@aztec/bb.js` resolution; unset → None → core's `"unknown"` default.

## MEASURED RESULT (the headline)
- Headless `cargo tree -p accelerator-server --edges normal --prefix none | sort -u`: **446 → 194 packages
  (−252, −56%)**.
- GUI/serving crates present after: **NONE** — no tauri / tao / wry / rcgen / tokio-rustls / x509-parser /
  rustls-pemfile. The entire Tauri + webview + TLS-serving + cert-gen tree is gone.
- Build: **0** of {tauri,tao,wry,rcgen,tokio-rustls} compiled; clean cold build of the new tree = **~10.9s**.
- `cargo clippy` server clean; headless build green.
- (A rigorous cold-build A/B vs the old Tauri tree is deferred to the Phase-3 CI measurement; the −56% count
  delta + removal of the heaviest crates — tauri/wry/webview proc-macro tree — is the robust, unambiguous win.)

## Attempts
- GREEN, single pass. The Phase-1 wrapper made this a 3-edit repoint with zero surprises.
