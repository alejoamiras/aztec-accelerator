# Cluster audit: crypto-tls-updater (HIGH crypto emphasis)

Scope: `packages/accelerator/src-tauri/src/certs.rs`, `packages/accelerator/src-tauri/src/server/tls.rs`, `packages/accelerator/src-tauri/src/updater.rs`, `packages/accelerator/core/src/versions/release_metadata.rs`, `packages/accelerator/core/src/versions/version_policy.rs`.

2 findings. Several other angles in-scope for this cluster were investigated and closed as NON-FINDINGS with a concrete reason (see end of file) rather than reported, per instructions.

---

## Finding 1: App auto-updater has no cryptographic binding between the feed's `version` field and its `url`/`signature` fields — a compromised release feed can force-install an old, validly-signed (but stale/vulnerable) build under a fabricated version number

**1. Title:** Updater downgrade/rollback via unauthenticated `version` field decoupled from the Ed25519-verified artifact

**2. Impact factors:**
- Confidentiality: indirect (a forced rollback can reintroduce a build with an already-fixed confidentiality bug, e.g. a pre-SEC-08 build that persists a readable CA key — see certs.rs history).
- Integrity: violated — the app installs and executes attacker-chosen (from among previously-published, validly-signed) code without the user knowingly consenting to a downgrade.
- Authorization: not directly implicated.
- Availability: a rollback to a broken/incompatible old build is also possible via this same path.
- Blast radius: **all users** who query `https://aztec-accelerator.dev/releases/latest.json` (every install, both `auto_update=true` and manual-check users).
- Data sensitivity: high (arbitrary-within-signed-history code execution on the user's machine, silently for auto-update users).
- Attack vector: network, but scoped to whoever controls the feed origin (not a generic MITM — TLS is intact and not bypassed here).
- Attack complexity: low, once feed-write access is obtained (attacker only needs to edit one JSON field; the old artifact + its legitimate signature are already public).
- Privileges required: attacker needs write access to the update feed/origin (S3/CloudFront origin or the CI/CD publish credentials for `aztec-accelerator.dev`) — **but explicitly does NOT need the Ed25519 private signing key**. This is the crux of the finding: it works with a strictly weaker attacker than "holds the signing key."
- User interaction: none for `auto_update=true` users (silent); a single "Update Now" click for manual users, who would see a plausible (fabricated) higher version number.

**3. Evidence confidence:** high — traced through both this repo's call sites and the exact upstream `tauri-plugin-updater` v2 source lines (fetched and quoted below).

**4. OWASP category + CWE:** OWASP A08:2021 (Software and Data Integrity Failures); CWE-345 (Insufficient Verification of Data Authenticity) / CWE-829 (Inclusion of Functionality from Untrusted Control Sphere); adjacent to CWE-347 (Improper Verification of Cryptographic Signature) in the sense that the signature covers the wrong scope (bytes only, not the version claim).

**5. Trace (source → sink), file:line at every step:**
- Untrusted input enters: the JSON body returned by the single configured endpoint `https://aztec-accelerator.dev/releases/latest.json` — `packages/accelerator/src-tauri/tauri.conf.json:16-18` (`plugins.updater.endpoints` + `pubkey`).
- `packages/accelerator/src-tauri/src/updater.rs:27` — `let update = match updater.check().await { Ok(Some(update)) => update, ... }` inside `check_for_update()` (`updater.rs:14-58`).
- Upstream `tauri-plugin-updater` v2 (`plugins/updater/src/updater.rs`, tag `v2`, commit `plugins-workspace`):
  - line 490-493: `let update_response: serde_json::Value = res.json().await?; ... serde_json::from_value::<RemoteRelease>(update_response)` — the entire release descriptor (`version`, per-platform `url`, `signature`) is parsed from one untrusted response with no manifest-level signature.
  - line 530-532: `let should_update = match self.version_comparator.as_ref() { Some(comparator) => ..., None => release.version > self.current_version, };` — the ONLY freshness gate, reading the untrusted `release.version` string; this repo installs the plugin with `tauri_plugin_updater::Builder::new().build()` (`packages/accelerator/src-tauri/src/main.rs:431`) — no custom `version_comparator` is set, so the default (bypassable) gate applies unmodified.
  - line 536-550: `download_url`/`signature` are taken from the SAME untrusted `release` and stored on the returned `Update` struct — not bound to `version` in any way.
  - line 712 (`Update::download`, defined at line 652): `verify_signature(&buffer, &self.signature, &self.config.pubkey)?;` — verifies only that the downloaded bytes match `self.signature` (itself feed-supplied). It does not verify that `self.version` truthfully describes those bytes.
  - line 1453-1462 (`fn verify_signature`): `public_key.verify(data, &signature, true)?` — minisign verification of raw artifact bytes; the `true` flag verifies the trusted-comment's own (global) signature integrity, but the plugin never extracts or enforces any freshness/version claim from that comment.
- Sink 1 (silent path, zero user interaction): `packages/accelerator/src-tauri/src/updater.rs:47-49` — `Some(true) => { perform_update(app, update).await; ... }`.
- Sink 2 (manual path): `PendingUpdate` stored at `main.rs:187-189`, consumed by `respond_update_prompt` (`commands.rs:225` area) → `crate::updater::perform_update(&handle, update).await` (`commands.rs:254`).
- `perform_update` → `update.install(bytes)` at `packages/accelerator/src-tauri/src/updater.rs:170`, then `app.restart()` (`updater.rs:177`).

**6. Missing control:** an independent, cryptographically-bound monotonic-version check — e.g. a locally-persisted "highest version ever successfully verified/installed" ratchet compared against the feed's claimed `version` before treating it as an "update," or a signature scope that covers `version` together with the artifact bytes (minisign's trusted-comment mechanism could carry this, but the plugin does not use it that way).

**7. Exploit/violation scenario:**
1. Attacker obtains write access to the update feed/origin (compromised S3/CloudFront credentials, or compromised CI secret for the release-publish workflow that writes `releases/latest.json`).
2. Attacker edits `latest.json`: sets `version` to an inflated string (e.g. `"99.99.99"`), leaves `url`/`signature` pointing at a real, previously-published OLDER build (e.g. a pre-SEC-08 build that persisted a readable `ca.key`, per this repo's own commit history) — both fields the attacker already has legitimately, from that build's own original, still-valid release.
3. Every install's periodic/manual `check_for_update()` sees `release.version ("99.99.99") > current_version` → true → returns `Some(Update)` pointing at the old artifact.
4. `perform_update` downloads the old bytes; `verify_signature` passes (the bytes genuinely match their own historical, legitimate signature — no forging of Ed25519 needed).
5. `update.install(bytes)` installs the rolled-back build; `app.restart()` relaunches it. Auto-update users get this with **zero interaction**; manual users see a plausible "update available" prompt.

**8. Preconditions:** attacker has write access to the feed/origin (`https://aztec-accelerator.dev/releases/latest.json`) or its publish pipeline — a "compromised upstream" attacker, explicitly one of the four actor types this audit's brief calls in-scope. No access to the Ed25519 private key is required.

**9. Why existing mitigations fail:** the only enforced authenticity control is the Ed25519/minisign signature over the raw artifact bytes (`updater.rs:84-183` doc comments: "Download, verify Ed25519 signature, install"). That check is necessary but insufficient here because it validates *byte integrity*, not *version truthfulness* — and the `version` field used for the downgrade gate comes from the exact same untrusted JSON as the bytes it's supposed to gate. `updater.rs` already documents at length why SEC-03 (the feed-controlled size cap) is availability-only because the cap is "read from the same feed it guards" — the same circular-trust reasoning applies here to the `version` field, but no equivalent control or caveat exists for it. This is a **new angle**, distinct from the two pre-documented residual caveats (SEC-02 is about the separate `bb`-binary GitHub digest trust plane; SEC-03 is about the size cap's availability property) — neither addresses app-updater version/downgrade binding at all.

**10. Instances:**
- `packages/accelerator/src-tauri/src/updater.rs:27` (`check()` call site)
- `packages/accelerator/src-tauri/src/updater.rs:47-49` (silent auto-install, no user interaction)
- `packages/accelerator/src-tauri/src/updater.rs:170` (`update.install(bytes)`)
- `packages/accelerator/src-tauri/src/main.rs:431` (`tauri_plugin_updater::Builder::new().build()` — no `version_comparator` override)
- `packages/accelerator/src-tauri/tauri.conf.json:16-18` (single feed endpoint + pinned pubkey)
- Upstream (corroborating, not this repo): `tauri-apps/plugins-workspace` tag `v2`, `plugins/updater/src/updater.rs` lines 490-493, 530-536, 652-712, 1453-1462.

---

## Finding 2: The keyless-CA design's core security claim ("the only copy of the CA signing key is gone") is not actually enforced in memory — rcgen's optional `zeroize` feature is enabled but never invoked, and would be incomplete even if it were

**1. Title:** CA private key material is not zeroized before drop, despite code comments asserting it is "gone"

**2. Impact factors:**
- Confidentiality: violated for the CA private signing key's in-memory representation (the crown-jewel primitive this design is explicitly built to eliminate).
- Integrity: indirect — recovery of the CA key lets an attacker mint additional trusted-chain leaf certs for 127.0.0.1/::1/localhost, enabling a MITM of the victim's own loopback proving traffic.
- Authorization: not directly.
- Availability: not implicated.
- Blast radius: single user / single machine (the CA and its Keychain trust are per-install, per-user).
- Data sensitivity: high — this is precisely the "mint-any-[localhost]-cert" primitive the code's own comment says closes "the audit HIGH" for the *disk* channel; this finding shows the *memory* channel is not closed.
- Attack vector: local only (requires reading this process's memory).
- Attack complexity: high (needs one of: root, `CAP_SYS_PTRACE` + a permissive `ptrace_scope` for same-uid ptrace, physical/swap-file access, or an accessible crash/core dump — not trivial, but squarely within "another local process/user on the machine," one of this audit's declared actor types).
- Privileges required: local code-execution as the same OS user (under permissive ptrace configs) up to root/physical access, depending on the concrete recovery channel.
- User interaction: none.

**3. Evidence confidence:** high — confirmed by reading both this repo's source (zero occurrences of `zeroize`/`Zeroize` in any accelerator Rust source) and the exact upstream `rcgen` 0.13.2 source (the version pinned in `Cargo.lock`).

**4. OWASP category + CWE:** CWE-226 (Sensitive Information Uncleared Before Release) / CWE-244 (Improper Clearing of Heap Memory Before Release) / adjacent to CWE-320 (Key Management Errors). OWASP A02:2021 (Cryptographic Failures).

**5. Trace, file:line at every step:**
- `packages/accelerator/src-tauri/Cargo.toml:53` — `rcgen = { version = "0.13", features = ["pem", "x509-parser", "zeroize"] }`. This enables the crate's optional `zeroize` dependency (confirmed present in the resolved tree: `packages/accelerator/src-tauri/Cargo.lock:3561-3573` lists `rcgen 0.13.2` depending on `ring` + `zeroize`).
- `packages/accelerator/src-tauri/src/certs.rs:143` — `let ca_key = KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;` (generates the CA signing key in-process).
- `packages/accelerator/src-tauri/src/certs.rs:144` — `let ca_cert = ca_params(now).self_signed(&ca_key)?;` (CA self-signs).
- `packages/accelerator/src-tauri/src/certs.rs:145-146` — `leaf_key` generated, leaf signed by `ca_key`.
- `packages/accelerator/src-tauri/src/certs.rs:151` (comment, directly above the function's closing brace where `ca_key` goes out of scope): *"`ca_key` drops here — the only copy of the CA signing key is gone."* — no `.zeroize()`, no `Zeroizing<T>` wrapper, anywhere in this function or file. Confirmed: `grep -n "zeroize\|Zeroize" packages/accelerator/src-tauri/src/certs.rs` and the same grep across all of `src-tauri/src/` and `core/src/` return **zero matches**.
- `packages/accelerator/src-tauri/src/certs.rs:86-88` (doc comment on `ca_params`): *"...is generated per-call and **discarded** right after it signs the leaf — no CA private key is ever written to disk, so the trusted anchor cannot mint any other cert (closes the audit HIGH)."* — the claim is scoped to disk persistence and is true for that; it is not true for process memory.
- Upstream `rcgen` 0.13.2 (pinned in `Cargo.lock`), fetched from `github.com/rustls/rcgen` at tag `v0.13.2` (commit `447322c`):
  - `rcgen/src/key_pair.rs:68-71` — `pub struct KeyPair { pub(crate) kind: KeyPairKind, pub(crate) alg: &'static SignatureAlgorithm, pub(crate) serialized_der: Vec<u8> }`. No `Drop` impl, no `#[derive(ZeroizeOnDrop)]`.
  - `rcgen/src/key_pair.rs:92-101` — for the ECDSA path (`PKCS_ECDSA_P256_SHA256`), `kind: KeyPairKind::Ec(key_pair)` where `key_pair` is a `ring::signature::EcdsaKeyPair` (confirmed: `Cargo.lock` resolves rcgen against `ring`, not `aws-lc-rs`, for this project).
  - `rcgen/src/lib.rs:656-659` — `#[cfg(feature = "zeroize")] impl zeroize::Zeroize for KeyPair { fn zeroize(&mut self) { self.serialized_der.zeroize(); } }`. This is the plain `Zeroize` trait (a method the CALLER must invoke explicitly) — **not** `ZeroizeOnDrop` — and it clears only `self.serialized_der` (the outer PKCS#8 DER bytes). It never touches `self.kind`, i.e. never reaches the `ring::signature::EcdsaKeyPair` object holding ring's own internal copy of the private scalar. `ring` is well known not to zero its own internal key buffers on drop.

**6. Missing control:** (a) no call site in this repo ever invokes `KeyPair::zeroize()` (or wraps the key in a zeroizing container) before the value is dropped, so enabling the Cargo feature has zero runtime effect; (b) even if such a call were added, rcgen's own `Zeroize` impl for `KeyPair` is incomplete — it does not clear the backend (`ring`)-held key object, only the outer DER encoding.

**7. Exploit/violation scenario:**
1. `certs::generate_and_save()` (first run) or `certs::rotate()` (~biennial renewal, `certs.rs:298-346`) calls `write_new_cert_set` (`certs.rs:141-153`), which allocates `ca_key` (a `rcgen::KeyPair` wrapping a `ring::EcdsaKeyPair` plus a `Vec<u8>` PKCS#8 DER copy).
2. A local attacker (or an automated crash-reporting / hibernation-image / backup path) captures the Accelerator process's memory during or shortly after this call — e.g. `gcore <pid>` / `/proc/<pid>/mem` under a permissive `ptrace_scope` or root, a suspend-to-disk image, or an uploaded crash dump whose heap pages still contain the freed-but-unscrubbed allocation (Rust's default allocator does not clear freed memory).
3. The CA private key (DER or raw EC scalar) is recovered from that capture — nothing in this code path, nor in the enabled rcgen dependency, ever overwrote it.
4. Using the recovered key plus the still-installed, still-trusted `ca.pem` anchor (Keychain-trusted via `certs.rs:362-379`, `install_ca_trust`/`add_trusted_cert`) — whose `NameConstraints` (`certs.rs:97-107`) permit exactly `127.0.0.1` / `::1` / `localhost` — the attacker mints a fresh leaf cert for the loopback proving server that the OS will trust.
5. The attacker (already local, per the precondition) uses that forged, trusted cert to MITM the victim's own `:59834` HTTPS traffic to the Accelerator, intercepting or tampering the proving witness in transit — reopening, via the memory channel, the exact "mint-any-cert" exposure the disk-removal fix was designed to close.

**8. Preconditions:** local attacker (or forensic/crash-artifact access) able to read the Accelerator process's memory at or shortly after a cert-generation/rotation event: root, `CAP_SYS_PTRACE` with a same-uid-permissive `ptrace_scope`, physical/swap access, or an accessible core dump. A materially higher bar than a remote web-page attacker, but within the audit's declared "local process/user" actor and requiring neither disk access to a persisted key file (there is none) nor the macOS Keychain unlock (trust is separate from key possession).

**9. Why existing mitigations fail:** the code explicitly frames "no CA private key is ever written to disk" as closing a prior "audit HIGH," and visibly opted into rcgen's `zeroize` Cargo feature — signaling clear intent to also close the memory-residue angle. That intent is not realized: (a) the feature is inert without an explicit `.zeroize()` call, which is absent everywhere in this codebase, and (b) rcgen's `Zeroize` impl for `KeyPair` only clears the outer DER `Vec<u8>`, not the `ring`-backed signing object — so even adding the call would leave a residual copy. The "closes the audit HIGH" and "the only copy... is gone" claims (`certs.rs:88`, `certs.rs:151`) are accurate only with respect to disk persistence, not process memory, and nothing in the code establishes or tests the memory-hygiene property they otherwise imply.

**10. Instances:**
- `packages/accelerator/src-tauri/src/certs.rs:141-153` (`write_new_cert_set`) — both `ca_key` (line 143, the higher-severity instance, since the entire security model for this key rests on transient-memory-only existence) and `leaf_key` (line 145, a secondary instance — its primary confidentiality control is the 0600 on-disk file permission, so the memory-residue window here is a smaller marginal risk on top of an already-intentionally-persisted secret).
- `packages/accelerator/src-tauri/Cargo.toml:53` (the inert feature flag).
- Upstream (corroborating): `rustls/rcgen` tag `v0.13.2` (commit `447322c`), `rcgen/src/key_pair.rs:68-101`, `rcgen/src/lib.rs:656-659`.

---

## Areas investigated and closed as NON-FINDINGS (with reason)

- **`certs.rs` rotation atomicity (`swap_into`'s three sequential `fs::rename` calls not being atomic as a whole; possible mismatched leaf-cert/leaf-key pair from a crash or concurrent `rotate()`/`generate_and_save()` race).** Verified this is already fail-closed: `rustls` 0.23.38's `with_single_cert` (`rustls/src/server/builder.rs:65-71`, confirmed via upstream source at the pinned commit) explicitly documents and enforces "This function fails ... if the `SubjectPublicKeyInfo` from the private key does not match the public key for the end-entity certificate," so any mismatched pair makes `certs::load_rustls_config()` (`certs.rs:257-273`) return `Err`. `main.rs:107-117`'s `try_start_https` already catches exactly this (`// A broken/mismatched cert set (e.g. a crash mid-rotation leaving a new leaf with the old key) must NOT silently wedge HTTPS`) and self-heals via `reset_safari_support`. No bypass found; HTTP (the primary channel) is never affected either way. NON-FINDING.
- **macOS `security` subprocess argument injection (SHA-1/label strings into argv).** All calls use `std::process::Command::new("security").arg(...)` (`certs.rs:366-370`, `398-401`, `412-414`) — direct argv, no shell interpolation — so no injection is possible regardless of the content of the SHA-1 string. Manipulating that string would also require the local user to already control their own login Keychain, not a privilege boundary crossing. NON-FINDING.
- **`version_policy::is_valid_version` traversal gate.** Re-derived the character-class/length/leading-dot/`..`-substring rules by hand against a traversal/injection corpus; found no accepted string that escapes `versions_base_dir()` or injects into a shell (there is no shell use downstream in this file). Matches the already-extensive test corpus. NON-FINDING.
- **`release_metadata.rs` digest/URL construction.** No issues beyond the already-documented SEC-02 circular-trust caveat (GitHub digest + binary share one control plane) — no new angle found.
- **Directory permissions in `rotate()` (`certs.rs:317-319` `create_dir_all` with no explicit `chmod 0700`, unlike `generate_certs()` at `certs.rs:158-163`).** Real inconsistency, but traced the only trigger path (`regenerate_leaf_if_expiring` invoked from a background thread that only runs immediately after `try_start_https`'s own `certs_exist()` check already passed in the same call) and found it requires the user to delete the certs directory in a sub-second window during their own app's startup — and even then, `write_pem_file` still chmods each file `0600` individually, so no private-key bytes are ever exposed, only (in the contrived case) directory listing metadata. Below the bar for a reportable finding.
