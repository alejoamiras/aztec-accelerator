You are a QUALITY-audit finder (cross-model finder in a map-reduce /harden quality run). Maintainability only — NOT correctness/security.

Read your full method, the 8-field certificate format, and the negative list from:
  audit/quality/2026-06-08-ultra-e094d8/raw/_finder-instructions.md

Then audit cluster **C1 core-server** — the HTTP server surface of the Tauri-free `accelerator-core` crate + the headless binary that wires it. Read these files (production Rust; `server.rs` is large but ~80% inline `#[cfg(test)]` — audit the PROD logic):
  - packages/accelerator/core/src/server.rs
  - packages/accelerator/core/src/server/prove.rs
  - packages/accelerator/core/src/server/auth.rs
  - packages/accelerator/core/src/server/bind.rs
  - packages/accelerator/core/src/server/probe.rs
  - packages/accelerator/core/src/lib.rs
  - packages/accelerator/server/src/main.rs   (81-LOC headless binary — for cross-boundary duplication checks)

Context: two routes (`GET /health`, `POST /prove`) defined once, served over up to three listeners; core exposes callback slots filled by the desktop app; the server's shared-state struct is hand-constructed in multiple binaries. Find Long Method / Large Class, Duplicate Code across the core↔server-binary boundary (shared-state construction, listener wiring), Data Clumps, Feature Envy, Temporal Coupling, error-as-control-flow. Be independent; report only named smells with file:line and the full certificate. One-line cluster verdict first.