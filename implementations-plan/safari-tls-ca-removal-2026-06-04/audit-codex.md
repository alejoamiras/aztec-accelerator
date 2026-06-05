**Verdict**

`needs-rework`

**Ranked concerns**

1. The core trust assumption is still too weak to ship. The plan’s default path relies on trusting a self-signed non-CA leaf via `trustAsRoot` ([plan.md](</Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/safari-tls-ca-removal-2026-06-04/plan.md:42>), [plan.md](</Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/safari-tls-ca-removal-2026-06-04/plan.md:46>)). Apple’s trust model does support `trustAsRoot` for a non-root cert, but Apple’s current local-network TLS guidance still tells developers to create/manage a local CA, and Apple DTS explicitly warns that self-signed server certs have edge cases and recommends CA-issued leafs. `security verify-cert` is a useful preflight, not proof of Safari/WebKit behavior. If you keep the bare-leaf design, the gate must be a real WebKit/URLSession integration, not only `verify-cert`.

2. The fallback is conceptually sound, but underspecified. `rcgen` can sign one leaf and then discard the CA key; serving only needs the leaf/key afterward ([certs.rs](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/certs.rs:147>)). But if fallback becomes the real path, every rotation creates another trusted CA anchor. “Forward-only keychain cleanup” is defensible for legacy anchors; it is much less defensible for newly-created one-shot CAs.

3. Rotation still has an outage cliff. Current startup silently reissues the leaf ([main.rs](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:70>), [certs.rs](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/certs.rs:183>)). The plan replaces that with deferred non-silent rotation ([plan.md](</Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/safari-tls-ca-removal-2026-06-04/plan.md:55>)). Safer, yes, but after expiry a headless startup just loses HTTPS. That is fail-closed, but still a production failure unless there is explicit pre-expiry prompting well before day 0.

4. One assumption is sloppier than it should be. “Nothing but `load_rustls_config` consumes the CA files” is false as written: CA files are also read by regeneration and trust management ([certs.rs](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/certs.rs:204>), [certs.rs](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/certs.rs:244>)). The external call-site inventory is otherwise solid: Settings generate/trust ([commands.rs](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:146>)), startup trust/rotation ([main.rs](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:75>)).

5. The updater E2E is good coverage, but it is not the same trust boundary. The current smoke test exercises a CA-backed fake prod host, not Safari localhost ([updater-feed-server.ts](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/scripts/updater-feed-server.ts:4>), [updater-smoke.sh](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/scripts/updater-smoke.sh:91>)). Updating it helps, but it does not prove the make-or-break Safari assumption.

**What’s solid**

Deleting `ca.key`/`ca.pem` does neutralize the current at-rest HIGH for users who never leaked the old key; the lingering anchor alone cannot mint anything new ([certs.rs](</Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/certs.rs:111>), [plan.md](</Users/alejoamiras/Projects/aztec-accelerator/implementations-plan/safari-tls-ca-removal-2026-06-04/plan.md:60>)). Atomic writes, validity-aware `certs_exist`, `0o600`, and SAN/EKU dedup are all good hardening.

Most likely production failure: `verify-cert` passes, but Safari/WebKit still rejects the directly-trusted non-CA leaf, so HTTPS quietly disappears.

Sources: Apple 103769 https://support.apple.com/en-us/103769 ; Apple local-network TLS guidance https://developer.apple.com/documentation/network/creating-an-identity-for-local-network-tls ; Apple DTS forum guidance https://developer.apple.com/forums/thread/61245 ; rcgen docs https://docs.rs/rcgen/latest/rcgen/struct.CertificateParams.html