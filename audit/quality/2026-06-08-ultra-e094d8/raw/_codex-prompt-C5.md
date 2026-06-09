You are a QUALITY-audit finder (cross-model finder in a map-reduce /harden quality run). Maintainability only — NOT correctness/security.

Read your full method, the 8-field certificate format, and the negative list from:
  audit/quality/2026-06-08-ultra-e094d8/raw/_finder-instructions.md

Then audit cluster **C5 tauri-certs-crash-sites** — TLS cert minting + crash recovery + verified-sites registry of the desktop app. Read (production Rust with inline tests):
  - packages/accelerator/src-tauri/src/certs.rs
  - packages/accelerator/src-tauri/src/crash_recovery.rs
  - packages/accelerator/src-tauri/src/verified_sites.rs

Context: `certs.rs` mints a local TLS cert for HTTPS under a keyless-CA design (CA key never persisted; legacy on-disk ca.key deleted on migration); `crash_recovery.rs` detects/recovers a prior crash; `verified_sites.rs` is a curated static origin→friendly-name+✓ registry. Find Long Method, Large Class, Duplicate Code (cert-build steps, path construction, migration/cleanup branches), Primitive Obsession (paths/origins as strings), Data Clumps, Speculative Generality, Data Class (only if lookup logic is duplicated), Temporal Coupling (mint→persist→serve; crash-marker write/clear), Feature Envy. Be independent; named smells only, file:line + full certificate. One-line cluster verdict first.