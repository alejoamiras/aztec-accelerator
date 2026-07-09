1. **Spoofable incumbent probe lets a local impostor keep the accelerator port**

   **Impact factors:** Confidentiality, Integrity, Availability. Blast radius: one user on Windows. Data sensitivity: proving witness/private ZK inputs if dApps later submit `/prove` to the impostor on `127.0.0.1:59833`.

   **Exploitability:** Attack vector: local. Attack complexity: low. Privileges required: low/none beyond binding an available user-space loopback port. User interaction: required later, when the user uses a dApp that submits a proving request.

   **Evidence confidence:** High.

   **OWASP + CWE:** OWASP A07 Identification and Authentication Failures; CWE-287 Improper Authentication.

   **Trace:**  
   Local attacker controls a listener on `127.0.0.1:59833` -> real app fails to bind in `packages/accelerator/core/src/server.rs:191` and `packages/accelerator/core/src/server.rs:192` via `bind_with_retry` -> retry returns `AddrInUse` at `packages/accelerator/core/src/server/bind.rs:30` and `packages/accelerator/core/src/server/bind.rs:37`-`41` -> desktop startup probes the incumbent at `packages/accelerator/core/src/server/probe.rs:25` and `packages/accelerator/core/src/server/probe.rs:33` -> accepts any 2xx JSON body with only `status == "ok"` and `api_version == 1` at `packages/accelerator/core/src/server/probe.rs:14`-`17` and `packages/accelerator/core/src/server/probe.rs:38`-`44` -> production caller exits cleanly when the probe is true at `packages/accelerator/src-tauri/src/main.rs:238`-`245`, leaving the attacker’s process owning the accelerator endpoint.

   **Missing control:** The self-probe has no process authenticity check. It does not verify a per-install secret, signed challenge, named-pipe/IPC identity, executable identity, certificate pin, or any other property that distinguishes the real accelerator from a foreign local process.

   **Exploit/violation scenario:**  
   1. A low-privileged local process starts before Aztec Accelerator and binds `127.0.0.1:59833`.  
   2. It serves `GET /health` with HTTP 200 and body `{"status":"ok","api_version":1}`.  
   3. On Windows, the real accelerator starts, fails to bind, probes `/health`, classifies the impostor as healthy Aztec, and exits with code 0.  
   4. The attacker keeps serving the port and implements `/prove`.  
   5. When the user visits an approved dApp or otherwise submits a proving request, the witness body is sent to the attacker-controlled localhost service.

   **Preconditions:** Windows desktop path; attacker can run a local process before or during accelerator startup; port `59833` is free when the attacker binds it; user later performs proving through a client that targets `127.0.0.1:59833`.

   **Why existing mitigations fail:** The Host allowlist does not help because the malicious process is the one receiving traffic on loopback. Per-origin authorization in the real accelerator does not run because the real accelerator exited. The probe’s “healthy Aztec” check is only a public JSON shape and is trivial for a foreign local process to mimic.

   **Instances:**  
   `packages/accelerator/core/src/server/probe.rs:14`-`17`  
   `packages/accelerator/core/src/server/probe.rs:24`-`44`