1. **Concurrent body buffering bypasses the prove semaphore**

   **Impact factors:** Availability violation; blast radius is one local user/app instance; data sensitivity is low for the direct DoS, though queued buffers contain private witness bytes in process memory. Exploitability: attack vector network via malicious approved browser origin to loopback, or local via no-Origin script; attack complexity low; privileges required none after origin approval/allowed no-Origin path; user interaction required for a malicious web page visit and, if not already approved, origin approval.

   **Evidence confidence:** High.

   **OWASP category + CWE:** OWASP API4:2023 Unrestricted Resource Consumption; CWE-770 Allocation of Resources Without Limits or Throttling.

   **Trace:** `/prove` receives an HTTP request at `packages/accelerator/core/src/server/prove.rs:101` -> authorization runs before buffering at `packages/accelerator/core/src/server/prove.rs:110` -> the full request body is buffered up to `MAX_BODY_SIZE` of 50MB at `packages/accelerator/core/src/server/prove.rs:99` and `packages/accelerator/core/src/server/prove.rs:112` -> only after buffering does the handler wait on `state.prove_semaphore.acquire()` at `packages/accelerator/core/src/server/prove.rs:121` -> while waiting, each task retains its already-buffered `body` until `bb::prove(&body, ...)` at `packages/accelerator/core/src/server/prove.rs:192`.

   **Missing control:** No aggregate memory/concurrency limit exists before request-body buffering. The semaphore limits only `bb` execution, not the number of concurrent 50MB witness bodies resident in memory.

   **Exploit/violation scenario:** A malicious dApp from an already approved origin opens many concurrent `fetch("http://127.0.0.1:59833/prove", { method: "POST", body: 50MB_blob })` requests. Each request passes origin authorization, buffers up to 50MB, then waits for the single prove permit. Hundreds of queued requests can consume gigabytes of memory before any `bb` process starts, causing the accelerator or desktop session to become unstable or crash.

   **Preconditions:** The attacker needs an approved browser origin, user approval during the prompt, localhost auto-approval where configured, or a local/no-Origin client path that is intentionally allowed. The Host allowlist must be satisfied with an allowed loopback host.

   **Why existing mitigations fail:** The Host allowlist only constrains the target host, not request volume. Deny-by-default origin auth blocks unapproved browser origins, but approved origins are still not resource-limited. The 50MB cap is per request, not global. The prove semaphore is acquired after body buffering, so it does not bound queued witness memory.

   **Instances:** `packages/accelerator/core/src/server/prove.rs:99`, `packages/accelerator/core/src/server/prove.rs:112`, `packages/accelerator/core/src/server/prove.rs:121`, `packages/accelerator/core/src/server/prove.rs:192`.