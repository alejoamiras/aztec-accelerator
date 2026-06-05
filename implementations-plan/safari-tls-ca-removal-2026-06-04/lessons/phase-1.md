# Phase 1 — certs.rs core: discard the CA key (compiles + existing tests green)

Implemented (feat/tls-discard-ca-key):
- `ca_params(now)` + `leaf_params(now)` helpers (dedup gen vs rotate). Leaf validity = **824 days**
  (under Apple's 825-day cap — confirmed by the opus research to apply even to user-trusted certs).
- `generate_certs()`: generate CA keypair IN MEMORY → CA cert → leaf → write ca.pem + localhost.pem
  + localhost.key, **never ca.key**. The `ca_key` `KeyPair` drops at fn end — the only copy of the
  CA signing key is gone. This is the security win: a trusted CA anchor with no key can't mint anything.
- `certs_exist()`: now validity-checked (leaf parses + not expired), not just `.exists()`; `ca.key`
  no longer required.
- `write_pem_file()`: atomic (distinct temp sibling → fsync → rename → explicit 0o600), no
  truncate-in-place corruption window (codex MED).
- `migrate_legacy_ca_key()`: deletes a legacy on-disk `ca.key` (closes the HIGH for existing users —
  keyless anchor can't sign).
- `regenerate_leaf_if_expiring()`: reworked to the keyless model — can't re-sign under the old CA
  (no key), so it regenerates a FRESH keyless CA+leaf and (macOS) re-installs trust; returns Err if
  trust not granted so the caller keeps serving the old still-valid leaf. ROTATE_BEFORE_DAYS=30.

Validation: `cargo check` exit 0; `cargo test --lib certs` 4 passed. (Existing tests cover rcgen gen +
rustls load + write_pem_file perms; they're behavior-preserved.)

## DEFERRED to Phase 2-4 (need real-macOS validation — can't run Safari/keychain in CI here)
- Precise OLD CA anchor removal by SHA-1 on rotation (avoid keyless-anchor accumulation).
- `ensure_https_identity()` orchestration in main.rs: pre-expiry prompting, interactive-only rotation
  (headless startup DEFERS, keeps old leaf), generate→trust→verify→serve ordering, fail-closed.
- Wire `migrate_legacy_ca_key()` into main.rs startup.
- Unit test: generate writes NO ca.key; macOS integration: trustRoot install + verify-cert + rotation.
