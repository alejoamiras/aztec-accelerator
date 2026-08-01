## Authorized clients can monopolize the queue and retain up to 400 MiB of request bodies

1. **Impact factors**

   - **Property:** Availability; authorization is also over-broad because approval grants access to the entire global scheduling budget.
   - **Blast radius:** All proving clients and the accelerator process; memory pressure can affect the desktop session.
   - **Data sensitivity:** Private transaction proving is interrupted, although this path does not directly disclose witnesses.
   - **Exploitability:** Browser-to-loopback or local-process vector; low complexity. Requires an approved/compromised origin, or another client already permitted by the documented localhost policy. Initial origin approval may require user interaction; none is required afterward.

2. **Evidence confidence:** High.

3. **OWASP / CWE mapping**

   [OWASP A06:2025 — Insecure Design](https://owasp.org/Top10/2025/A06_2025-Insecure_Design/); [CWE-770 — Allocation of Resources Without Limits or Throttling](https://cwe.mitre.org/data/definitions/770.html). No more precise CWE from the current 2025 Top 25 applies.

4. **Trace**

   1. An HTTP request enters [`prove.rs:224–232`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:224).
   2. Origin approval is checked once at [`prove.rs:233`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:233), but the approved origin is not retained for per-origin accounting.
   3. Each request consumes one slot from the shared global inflight semaphore at [`prove.rs:218–221`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:218) and [`prove.rs:235–238`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:235).
   4. Each slot may buffer 50 MiB at [`prove.rs:132`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:132) and [`prove.rs:194–211`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:194), invoked at [`prove.rs:243–246`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:243). With eight configured slots, the code explicitly permits approximately 400 MiB of buffered bodies.
   5. Attacker-selected cache-miss versions can additionally trigger concurrent downloads before serialization at [`prove.rs:249–275`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:249).
   6. The body remains live while each request waits for the single proving permit at [`prove.rs:317–329`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:317).
   7. A proof may retain that permit for up to five minutes at [`bb.rs:6–7`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:6) and [`bb.rs:230–235`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:230).
   8. Once all slots are attacker-held, every legitimate request reaches [`prove.rs:220–221`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:220) and is rejected as `ProveQueueFull`.

5. **Missing control**

   There is no per-origin quota, fair scheduling, request-rate limit, or aggregate byte/resource budget. The global count limit treats one abusive approved origin as entitled to every slot and allows every slot to retain the maximum-sized body while waiting for serialized work.

6. **Exploit story**

   1. A compromised previously approved dApp opens eight concurrent `/prove` requests.
   2. Each request supplies a 50 MiB body. Distinct published `x-aztec-version` values can be used to add parallel cache downloads.
   3. All requests pass the single origin check and consume every global inflight slot.
   4. One request proves while the others retain their bodies awaiting the single semaphore.
   5. Other origins receive `429 ProveQueueFull`. The attacker submits a replacement whenever a slot opens, sustaining roughly 400 MiB of buffering and continuous prover utilization.
   6. This can indefinitely deny proofs and may terminate the accelerator through memory pressure.

7. **Preconditions**

   - The origin has previously been approved, is compromised after approval, or the requester is otherwise permitted by the documented localhost policy.
   - The attacker can create valid or sufficiently expensive proving jobs to keep the serialized prover occupied.
   - For download amplification, the requested published versions are not already cached.

8. **Why mitigations fail**

   - The 50 MiB cap is per request, not aggregate.
   - The eight-request gate bounds the attack but still reserves every slot for one actor and permits about 400 MiB of bodies.
   - The 30-second timeout ends once a body is fully uploaded; it does not limit queue residence.
   - The five-minute timeout applies separately to serialized proofs, allowing eight requests to pre-book a long queue.
   - Immediate `429` shedding protects additional resources but is itself the attacker’s denial-of-service outcome.
   - This does not rely on re-reporting the accepted absent-`Origin` behavior; an already approved browser origin is sufficient.

9. **Security consequence**

   A single approved actor can exclude every other client from a supposedly multi-origin local service and force sustained memory, CPU, network, and disk consumption disproportionate to its authorization.

10. **Instances**

   - [`prove.rs:132–138`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:132)
   - [`prove.rs:188–221`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:188)
   - [`prove.rs:235–275`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:235)
   - [`prove.rs:317–329`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:317)
   - [`bb.rs:6–7`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:6)
   - [`bb.rs:225–235`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:225)

## Verified `bb` files are reopened by pathname, allowing replacement before execution

1. **Impact factors**

   - **Properties:** Confidentiality, integrity, authorization, and availability.
   - **Blast radius:** Private witnesses processed by the substituted executable, proof responses, and any user-level resource accessible to the accelerator process.
   - **Data sensitivity:** Private transaction witnesses are passed directly to the executable.
   - **Exploitability:** Local filesystem vector; moderate race complexity. Requires write/rename access to the chosen executable or its parent directory. No UI interaction is required beyond a normal proof request.

2. **Evidence confidence:** High.

3. **OWASP / CWE mapping**

   [OWASP A08:2025 — Software or Data Integrity Failures](https://owasp.org/Top10/2025/A08_2025-Software_or_Data_Integrity_Failures/); [CWE-367 — TOCTOU Race Condition](https://cwe.mitre.org/data/definitions/367.html); fallback locations additionally match [CWE-427 — Uncontrolled Search Path Element](https://cwe.mitre.org/data/definitions/427.html). No precise current Top-25 CWE describes the root cause.

4. **Trace**

   1. A requested version is selected from the HTTP header at [`prove.rs:249–268`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:249) and passed into `bb::prove` at [`prove.rs:329`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/prove.rs:329).
   2. The cache integrity check returns only a `PathBuf` at [`bb.rs:35–39`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:35); its dependency signature likewise returns `Result<PathBuf, String>` at [`cache_layout.rs:209`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/versions/cache_layout.rs:209).
   3. `bb::prove` stores that pathname at [`bb.rs:191–192`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:191).
   4. The verified file is no longer pinned while the process creates a workspace and writes the private witness at [`bb.rs:194–198`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:194). A writable pathname can be replaced during this interval.
   5. A fresh `Command` is constructed from the pathname and receives the private witness path at [`bb.rs:206–219`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:206).
   6. `spawn()` reopens and executes whatever object currently occupies that pathname at [`bb.rs:225–230`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:225).
   7. The substituted process can read the witness argument, exfiltrate it, invoke the original prover to remain stealthy, and control the proof file later accepted at [`bb.rs:243–255`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:243).

5. **Missing control**

   The integrity decision is not bound to the executable object actually spawned. The code needs an immutable/held executable identity across verification and execution, or an installation directory that the modeled attacker cannot modify. Production execution should also avoid unverified `~/.bb` and `PATH` fallbacks.

6. **Exploit story**

   1. A hostile local process obtains rename/write access to a cached `versions/<version>/bb` or another selected candidate.
   2. It monitors the file and waits for the accelerator to finish hashing it.
   3. During the subsequent workspace and witness-write interval, it atomically renames the verified executable aside and places a wrapper at the original pathname.
   4. The accelerator spawns the wrapper and gives it `--ivc_inputs_path <private-file>`.
   5. The wrapper copies or exfiltrates the witness, invokes the saved legitimate `bb` with the same arguments, and exits successfully.
   6. It restores the original executable afterward, so the marker passes on later requests and the proof request appears normal.

7. **Preconditions**

   - The attacker has local write or rename permission on the candidate executable or its parent—such as a same-account hostile process, an over-privileged cache manager, or a writable custom `~/.bb`/`PATH` location.
   - A proof request selects that executable.
   - Read-only packaged sidecars are not vulnerable to replacement unless their containing installation is writable; the version cache and configurable fallbacks remain relevant.

8. **Why mitigations fail**

   - F-007 rehashes the cache file, but only before the race window; the checked handle is discarded.
   - The `exists()` checks on fallback candidates do not authenticate their contents at all.
   - Private temporary-file ACLs do not help because the substituted child runs as the accelerator user and is intentionally given the witness path.
   - `kill_on_drop` and the five-minute timeout apply after execution and cannot undo immediate witness theft or user-level code execution.
   - This is distinct from the accepted SEC-02 control-plane issue: the attack replaces a locally checked file after verification.

9. **Security consequence**

   The recent cache-marker hardening is incomplete: it establishes what bytes existed during verification, not what bytes the operating system executes. A successful replacement can silently disclose private witnesses while preserving apparently valid proof behavior.

10. **Instances**

   All executable-selection branches return a mutable pathname to the common delayed spawn sink:

   - Operator override: [`bb.rs:28–32`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:28)
   - Marker-verified cache: [`bb.rs:35–39`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:35)
   - Bundled sidecar: [`bb.rs:42–50`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:42)
   - `~/.bb` fallback: [`bb.rs:53–58`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:53)
   - Unix `PATH` fallback: [`bb.rs:61–68`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:61)
   - Common check-to-spawn window and sink: [`bb.rs:191–229`](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/bb.rs:191)

No additional issue in `win_acl.rs` met the required concrete exploit-trace threshold.