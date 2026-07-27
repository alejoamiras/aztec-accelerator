1. **Unauthenticated Local Accelerator Can Harvest Witnesses**

   **Impact factors:** Confidentiality and Integrity; blast radius is one user; data sensitivity is highest because `executionSteps` are serialized private proving witness inputs. A malicious responder can also return arbitrary proof bytes, causing proof corruption or downstream failure. Exploitability: local attack vector, low complexity, low privileges if the attacker can bind the loopback port before the real accelerator, no user interaction beyond the user/dApp requesting a proof.

   **Evidence confidence:** high.

   **OWASP category + CWE:** OWASP A07: Identification and Authentication Failures; CWE-306: Missing Authentication for Critical Function.

   **Trace:**  
   `AcceleratorProver.createChonkProof()` receives private execution steps at [packages/sdk/src/lib/accelerator-prover.ts:262](packages/sdk/src/lib/accelerator-prover.ts:262).  
   It checks accelerator status before proving at [accelerator-prover.ts:273](packages/sdk/src/lib/accelerator-prover.ts:273).  
   `/health` probes unauthenticated loopback HTTP/HTTPS endpoints at [accelerator-transport.ts:112](packages/sdk/src/lib/accelerator-transport.ts:112), [accelerator-transport.ts:113](packages/sdk/src/lib/accelerator-transport.ts:113), [accelerator-transport.ts:114](packages/sdk/src/lib/accelerator-transport.ts:114).  
   The first successful probe wins via `Promise.any` at [accelerator-transport.ts:116](packages/sdk/src/lib/accelerator-transport.ts:116), [accelerator-transport.ts:117](packages/sdk/src/lib/accelerator-transport.ts:117).  
   A parseable JSON body is classified as available; notably `{}` reaches the legacy success return at [accelerator-prover.ts:177](packages/sdk/src/lib/accelerator-prover.ts:177), [accelerator-prover.ts:179](packages/sdk/src/lib/accelerator-prover.ts:179), [accelerator-prover.ts:217](packages/sdk/src/lib/accelerator-prover.ts:217), [accelerator-prover.ts:259](packages/sdk/src/lib/accelerator-prover.ts:259).  
   That result pins the attacker-controlled protocol at [accelerator-prover.ts:194](packages/sdk/src/lib/accelerator-prover.ts:194), [accelerator-transport.ts:73](packages/sdk/src/lib/accelerator-transport.ts:73), [accelerator-transport.ts:74](packages/sdk/src/lib/accelerator-transport.ts:74).  
   The witness is serialized at [accelerator-prover.ts:299](packages/sdk/src/lib/accelerator-prover.ts:299), then posted to the pinned unauthenticated endpoint at [accelerator-prover.ts:309](packages/sdk/src/lib/accelerator-prover.ts:309), [accelerator-transport.ts:148](packages/sdk/src/lib/accelerator-transport.ts:148), [accelerator-transport.ts:149](packages/sdk/src/lib/accelerator-transport.ts:149).

   **Missing control:** The SDK never authenticates that the `/health` or `/prove` responder is the real accelerator before transmitting the serialized witness. There is no pinned certificate/public key, signed challenge, shared local secret, trusted process attestation, or other server identity check. The health schema also does not require a version match in the legacy path; `{}` is enough to mark the endpoint available.

   **Exploit/violation scenario:**  
   1. The real accelerator is not running, starts late, or fails to bind `127.0.0.1:59833`.  
   2. A malicious local process binds `127.0.0.1:59833`.  
   3. It responds to `GET /health` with `HTTP 200` and body `{}`.  
   4. The SDK marks the accelerator available and pins `http`.  
   5. The user’s dApp calls `createChonkProof()` with private execution steps.  
   6. The SDK serializes the witness and sends it as `application/octet-stream` to `POST http://127.0.0.1:59833/prove`.  
   7. The malicious process stores the witness and may return arbitrary proof JSON.

   **Preconditions:** Attacker can run a local process under any account able to bind the configured loopback port before the genuine accelerator, or can otherwise control the configured accelerator host/port. The victim uses the SDK with acceleration enabled and requests a proof.

   **Why existing mitigations fail:** The Host-header allowlist and per-origin authorization are enforced by the legitimate accelerator server, but this path never proves the SDK is talking to that server. A fake loopback server can mimic `/health` and receive `/prove` before any legitimate server-side authorization code runs. The deny-by-default origin model therefore does not protect the witness from an impersonating local service.

   **Instances:**  
   [packages/sdk/src/lib/accelerator-transport.ts:112](packages/sdk/src/lib/accelerator-transport.ts:112), [accelerator-transport.ts:116](packages/sdk/src/lib/accelerator-transport.ts:116), [accelerator-transport.ts:148](packages/sdk/src/lib/accelerator-transport.ts:148), [packages/sdk/src/lib/accelerator-prover.ts:177](packages/sdk/src/lib/accelerator-prover.ts:177), [accelerator-prover.ts:194](packages/sdk/src/lib/accelerator-prover.ts:194), [accelerator-prover.ts:259](packages/sdk/src/lib/accelerator-prover.ts:259), [accelerator-prover.ts:299](packages/sdk/src/lib/accelerator-prover.ts:299), [accelerator-prover.ts:309](packages/sdk/src/lib/accelerator-prover.ts:309).