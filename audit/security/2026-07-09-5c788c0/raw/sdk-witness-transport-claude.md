# Security audit — cluster: sdk-witness-transport

Files audited:
- packages/sdk/src/lib/accelerator-transport.ts
- packages/sdk/src/lib/accelerator-prover.ts
- packages/sdk/src/lib/types.ts
- packages/sdk/src/lib/logger.ts
- packages/sdk/src/index.ts

## Finding 1: SDK sends the full ZK witness to whatever process holds the accelerator's loopback port, with no authentication of the responder — and its only nominal "version" gate does not actually gate the send

1. **Title**: Missing server-identity authentication lets any local process that binds 127.0.0.1:59833/59834 harvest the private proving witness

2. **Impact factors**:
   - Confidentiality: violated — the complete `serializePrivateExecutionSteps` witness (the private ZK inputs to a shielded transaction) is transmitted in full to the endpoint the SDK negotiates, without ever verifying that endpoint is the legitimate Aztec Accelerator. Data sensitivity: maximal — this is exactly the data the brief calls "the crown jewel."
   - Authorization: violated — the SDK (relying party) never authorizes/authenticates the server before handing it an operation-critical, sensitive payload. This is the inverse of the app's documented origin-authorization model (which protects the accelerator from unauthorized *web origins*, not the SDK from an unauthorized *accelerator*).
   - Integrity/Availability: secondary — a hostile responder can also return a garbage `proof` field, causing `ChonkProofWithPublicInputs.fromBuffer` to throw and the proving call to reject (visible failure), but this happens only *after* the witness has already left the process, so it doesn't mitigate the confidentiality loss.
   - Blast radius: all users of any dApp built on this SDK — the target ports are fixed, hardcoded, and publicly documented (this very repo), so the technique generalizes to every consumer, not one victim.
   - Attack vector: Local (attacker needs to run *some* local process on the victim's machine — not necessarily privileged, not necessarily the same OS user in all cases). Attack complexity: Low. Privileges required: None (binding an unprivileged loopback TCP port needs no elevation on Windows/macOS/Linux). User interaction: Required, but trivially satisfied — the victim just has to use a dApp that calls `AcceleratorProver.createChonkProof()`, i.e. normal use of the product this SDK ships for.

3. **Evidence confidence**: High — traced end-to-end in source, plus confirmed via direct inspection of the accelerator's own config that the HTTPS listener is off by default (`safari_support: false`, `packages/accelerator/core/src/config.rs:68`) and the headless server never binds HTTPS at all, so squatting is not merely a narrow race window.

4. **OWASP / CWE**: OWASP A07:2021 – Identification and Authentication Failures (also relevant: A08:2021 – Software and Data Integrity Failures, since the response is trusted without verifying its source). CWE-306 (Missing Authentication for Critical Function), CWE-940 (Improper Verification of Source of a Communication Channel).

5. **Trace** (source → sink, every hop):
   - `packages/sdk/src/lib/accelerator-transport.ts:112-138` (`probeHealth`) — fires `GET http://127.0.0.1:{port}/health` and `GET https://127.0.0.1:{httpsPort}/health` in parallel via plain `ky(...)` with no TLS pinning, no bearer token, no shared secret, no client cert. Whichever responds first (`Promise.any`) wins; there is no check that the *responder* is the legitimate Tauri/headless accelerator process versus any other process holding that loopback port.
   - `packages/sdk/src/lib/accelerator-prover.ts:158-205` (`#probeAndParseHealth`) — takes whatever `{response, protocol}` came back and, if `response.ok` and the body parses as JSON, unconditionally hands it to `#classifyHealth` and pins the winning `protocol` (`commitStatus(..., {pin:"set", protocol})`, line 194-197).
   - `packages/sdk/src/lib/accelerator-prover.ts:212-260` (`#classifyHealth`) — the *only* thing resembling a gate. It is trivially satisfiable by an attacker: if the response body omits `available_versions` **and** omits/uses `aztec_version: "unknown"` (or simply omits `aztec_version`), the function falls through to `return { available: true, needsDownload: false, ... }` (line 259) — i.e. a bare `{}` JSON body is sufficient to be classified `available: true`. No version knowledge is required at all.
   - `packages/sdk/src/lib/accelerator-prover.ts:273-285` (`createChonkProof`) — gates only on `status.available`. Critically, when `status.needsDownload` is `true` (line 280-283) the code does **not** stop or fall back — it only logs and fires a `"downloading"` UI phase, then falls through to `return this.#proveRemote(executionSteps)` (line 285) regardless. So even in the one case where the classifier flags a version it doesn't recognize, the witness is sent anyway.
   - `packages/sdk/src/lib/accelerator-prover.ts:293-329` (`#proveRemote`) — serializes the private execution steps (`serializePrivateExecutionSteps(executionSteps)`, line 299) and POSTs the raw bytes via `accelerator-transport.ts:144-157` (`postProve`) to `${baseUrl}/prove` as `application/octet-stream`. **This is the sink**: the witness bytes are on the wire to an unauthenticated endpoint at this point, before any response is even read.

6. **Missing control**: No authentication of the accelerator's identity before it is trusted with the witness — e.g. a pinned TLS certificate fingerprint established out-of-band at first pairing (TOFU), or an HMAC/bearer token minted by the real accelerator and read by the SDK from a tightly-permissioned local file the counterfeit process could not produce. The version-compatibility check in `#classifyHealth` is the closest thing to a control and it (a) is fully spoofable with a static, content-free response and (b) does not even gate the witness send when it fails to positively confirm compatibility (`needsDownload` is UX-only).

7. **Exploit/violation scenario**:
   1. Attacker gets any unprivileged local process running on the victim's machine (a wide precondition covering: unrelated malware already present, a bundled/adjacent app the user installed, or — on a shared/multi-tenant host — a different unprivileged OS user account, since loopback ports are not namespaced per-user by default on Linux/macOS/Windows).
   2. That process binds `127.0.0.1:59834` (HTTPS) — free in the *default* configuration even while the legitimate desktop app is running normally, since `safari_support` defaults to `false` and the HTTPS listener is only started after an explicit Settings opt-in — or binds `127.0.0.1:59833` (HTTP) during any window the legitimate accelerator isn't running (most simply: the very common case where the user's dApp ships this SDK but the user never installed the companion Aztec Accelerator app at all — the SDK is designed to gracefully auto-detect-or-fallback, so this is an expected, unremarkable end-state for a large fraction of users).
   3. It serves `GET /health → 200 {}` (or any body without a mismatching `aztec_version`).
   4. The victim visits any dApp using this SDK and triggers a private proof. The SDK's dual probe reaches the attacker's listener, classifies it `available: true`, pins the protocol, and POSTs the full serialized witness to `/prove`.
   5. Attacker's listener logs the raw request body — the private ZK inputs of the transaction (amounts, notes, nullifier secrets, etc.) — fully deanonymizing/exposing the transaction's private data. It can optionally return a canned/garbage response (or nothing, causing a visible client-side error) — either way the leak already occurred.

8. **Preconditions**: Attacker capability = ability to execute any unprivileged local process on the victim's host (not full compromise of the browser, the dApp, or the user's wallet — a materially weaker bar). Network condition = one of the two fixed ports (59833 HTTP or 59834 HTTPS) not currently held by a legitimate accelerator instance — true 100% of the time for 59834 by default, and true 100% of the time for 59833 whenever the user hasn't installed/launched the companion app.

9. **Why existing mitigations fail**: The app's documented guards — the loopback Host-header allowlist (anti-DNS-rebinding), deny-by-default per-origin authorization, and the verified-sites registry — all protect the *real* accelerator from *unauthorized web origins* connecting to it. None of them address the orthogonal trust direction exploited here: the SDK never verifies it is talking to the *real* accelerator in the first place. A counterfeit local listener does not need to bypass the Host-allowlist or origin-authorization logic at all, because it simply doesn't implement them — it only has to answer HTTP, which requires no cryptographic material and no knowledge of any app secret. This is a genuinely new angle, distinct from the already-documented SEC-02 (bb binary/digest trust plane) and SEC-03 (updater size cap) caveats, and distinct from the "absent-Origin curl/script" and headless `--allow-all` opt-ins, none of which touch client-side verification of the server.

10. **Instances** (same root cause — "no responder-identity check, and the pseudo-check that exists doesn't gate the send"):
    - `packages/sdk/src/lib/accelerator-transport.ts:112-138` — `probeHealth`, no identity verification of either probed endpoint.
    - `packages/sdk/src/lib/accelerator-transport.ts:144-157` — `postProve`, witness POST carries no auth token / cert pin / shared secret.
    - `packages/sdk/src/lib/accelerator-prover.ts:212-260` — `#classifyHealth`, trivially satisfiable, content-free gate.
    - `packages/sdk/src/lib/accelerator-prover.ts:273-285` — `createChonkProof`, `needsDownload` computed but not used to block `#proveRemote`.
    - `packages/sdk/src/lib/accelerator-prover.ts:293-329` — `#proveRemote`, the actual witness-transmission sink.

---

No other findings met the bar for a concrete, non-speculative trace in this cluster:
- Logging (`accelerator-prover.ts:314-321`, `340`) only carries booleans, durations, version strings, and the server's `error`/`message` 403 fields — no witness/proof bytes are ever logged in these files. Whether the `error`/`message` 403 fields could enable log-forging depends on a downstream LogTape sink that this SDK does not configure (`logger.ts` only calls `getLogger`, no sink attached) — sink behavior is consumer-owned and out of this cluster's files, so this was not flaggable as a concrete trace and is treated as a non-finding.
- `AZTEC_ACCELERATOR_PORT` / `AZTEC_ACCELERATOR_HTTPS_PORT` env vars (`accelerator-prover.ts:109-119`) are parsed with `Number.parseInt`, which strips any non-numeric suffix — no URL/host injection is possible through them, and they only select between attacker-adjacent local ports already covered by Finding 1, not an arbitrary remote host.
- The protocol pin/cache (`accelerator-transport.ts:59-100`) only toggles between the two fixed, config-supplied host/ports — it cannot be steered by a response body to an attacker-arbitrary host, so it does not add a distinct vulnerability beyond extending Finding 1's window by the 10s cache TTL.
