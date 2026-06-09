You are a QUALITY-audit finder (cross-model finder in a map-reduce /harden quality run). Maintainability only — NOT correctness/security.

Read your full method, the 8-field certificate format, and the negative list from:
  audit/quality/2026-06-08-ultra-e094d8/raw/_finder-instructions.md

Then audit cluster **C3 core-config-auth** — config persistence + site authorization of `accelerator-core`. Read (production Rust with inline tests):
  - packages/accelerator/core/src/config.rs
  - packages/accelerator/core/src/authorization.rs

Context: `config.rs` persists settings/state to disk via serde; `authorization.rs` decides if a calling web origin is authorized (MetaMask-style), canonicalizing origins via `url::Url` at ingress and validating versions via `AztecVersion::parse` (validation-as-constructor traversal guard). Find Primitive Obsession (origin/version as bare strings), Duplicate Code (canonicalization/serde patterns), Data Clumps, Long Method, Feature Envy (config↔auth reach-through), Temporal Coupling, Speculative Generality, Data Class. Be independent; named smells only, file:line + full certificate. One-line cluster verdict first.