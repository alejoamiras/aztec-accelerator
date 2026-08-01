Two findings met the reporting bar.

## Finding 1: Attacker-controlled certificate files can be promoted to unrestricted trusted roots

1. **Title.** Incomplete enforcement of the keyless, loopback-constrained CA design allows a substituted certificate set to be installed as a trusted root.

2. **Impact factors.**

   - **Confidentiality:** An attacker retaining the substituted CA private key can issue certificates for arbitrary domains or `localhost`. With traffic interception, proxy/DNS control, or ownership of port 59834 while the accelerator is unavailable, this can expose browser credentials, sessions, or private transaction witnesses.
   - **Integrity/authorization:** The user authorizes an Accelerator-specific, keyless, loopback-constrained CA, but the application may install an attacker-controlled, unconstrained CA whose signing key still exists.
   - **Blast radius:** The macOS login Keychain, Windows CurrentUser Root store, or every discovered Linux NSS browser store.
   - **Exploitability:** Local attack vector; low complexity for persistent-file substitution; requires code execution as the victim OS user but no administrator/root privileges. Normal HTTPS enablement and, on macOS/Windows, approval of the expected native trust dialog are required. The rotation race is a higher-complexity additional instance.

3. **Evidence confidence.** **High.** The accepted predicate and all three trust-store sinks are explicit. This is not a claim that the application writes its generated CA key; it is an incomplete enforcement of that mitigation because the trust path accepts a different CA whose key the attacker retains.

4. **OWASP / CWE mapping.** [OWASP A08: Software and Data Integrity Failures](https://owasp.org/Top10/A08_2021-Software_and_Data_Integrity_Failures/); [CWE-345: Insufficient Verification of Data Authenticity](https://cwe.mitre.org/data/definitions/345.html); [CWE-295: Improper Certificate Validation](https://cwe.mitre.org/data/definitions/295.html). Related [CWE-20: Improper Input Validation](https://cwe.mitre.org/data/definitions/20.html), which is #18 in the [2025 CWE Top 25](https://cwe.mitre.org/top25/archive/2025/2025_cwe_top25.html).

5. **Trace.**

   - **Source:** Predictable persistent paths `~/.aztec-accelerator/certs/{ca.pem,localhost.pem,localhost.key}` are selected at [certs.rs:38-44](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/certs.rs:38). A hostile process running as the victim user can replace these owner-owned files.
   - **Acceptance:** `certs_exist()` accepts the set based only on positive leaf time, leaf/key compatibility, and the leaf signature under the supplied CA at [certs.rs:145-152](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/certs.rs:145).
   - **Checks performed:** The CA and leaf are parsed and their signature relationship checked at [certs.rs:162-184](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/certs.rs:162); the leaf/key pair is accepted by rustls at [certs.rs:323-336](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/certs.rs:323); only the leaf’s `notAfter` is checked at [certs.rs:345-354](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/certs.rs:345).
   - **Preservation:** A passing attacker-created set causes regeneration to be skipped at [certs.rs:228-233](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/certs.rs:228).
   - **Trust handoff:** The unchanged `ca.pem` path is sent to the trust backend at [certs.rs:440-451](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/certs.rs:440) → [trust/mod.rs:110-114](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/trust/mod.rs:110).
   - **Sinks:**
     - macOS: `security add-trusted-cert -r trustRoot` at [trust/macos.rs:24-29](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/trust/macos.rs:24).
     - Linux: `certutil -A -t C,,` at [trust/linux.rs:283-299](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/trust/linux.rs:283), reached from [trust/linux.rs:412-448](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/trust/linux.rs:412).
     - Windows: `certutil -user -addstore Root` at [trust/windows.rs:59-65](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/trust/windows.rs:59), reached from [trust/windows.rs:132-147](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/trust/windows.rs:132).

6. **Missing control.** Before trust installation, the application does not authenticate the certificate set’s provenance or enforce the intended CA/leaf profiles: self-signature, CA constraints, exact loopback-only `NameConstraints`, CA/key usages, leaf SANs, leaf EKU, and validity bounds. More fundamentally, accepting a pre-existing CA cannot establish that its signing key was destroyed. The generated certificate’s identity also is not bound securely across generation, pathname handoff, and trust-store import.

7. **Exploit story.**

   1. Malware running as the victim user creates an unconstrained root CA with the expected common name and retains its private key.
   2. It creates a matching `localhost` leaf and writes the CA, leaf, and leaf key to the three predictable certificate paths.
   3. `certs_exist()` accepts the internally consistent set, so `generate_and_save()` does not replace it.
   4. During normal HTTPS enablement, the user sees the expected Accelerator CA name and approves the native trust prompt; Linux NSS installation does not require an equivalent OS credential prompt.
   5. The attacker can now mint certificates for arbitrary websites. It can also mint a trusted `localhost` certificate, claim port 59834 before the real app, and receive private proving requests intended for the Accelerator.

8. **Preconditions.**

   - The attacker can execute as the victim OS account and modify that account’s certificate directory.
   - HTTPS trust has not yet been installed, was removed, or is being renewed.
   - The victim completes normal enablement/renewal and approves the trust operation where prompted.
   - TLS interception additionally requires a traffic position or control of the intended local listener.

9. **Why mitigations fail.**

   - `0700` directory and `0600` file permissions protect against other OS users, not hostile processes running as the same user.
   - The signature and key-match checks prove only that the attacker created a self-consistent set.
   - `Zeroizing` and early destruction apply only to keys generated by this process; they say nothing about a substituted CA.
   - The intended loopback constraints are added only during generation at [certs.rs:95-114](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/certs.rs:95) and are never required when loading an existing CA.
   - Per-OS post-install checks merely confirm that the supplied attacker certificate became trusted.

10. **Instances.**

   - Persistent live-set adoption: [certs.rs:145-184](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/certs.rs:145), [certs.rs:228-233](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/certs.rs:228), and the three installation sinks above.
   - Rotation TOCTOU: deterministic staged paths are created at [certs.rs:54-60](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/certs.rs:54), then trusted without revalidating their identity/profile at [certs.rs:412-428](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/certs.rs:412) through [trust/macos.rs:161-167](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/trust/macos.rs:161), [trust/linux.rs:526-548](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/trust/linux.rs:526), or [trust/windows.rs:182-191](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/trust/windows.rs:182).

## Finding 2: Partial TLS handshakes create unbounded resident tasks and sockets

1. **Title.** The HTTPS listener has neither a handshake deadline nor a concurrent-connection limit.

2. **Impact factors.**

   - **Availability:** A local attacker can exhaust the Accelerator’s file descriptors, task memory, and CPU, denying HTTPS and potentially degrading the shared process’s HTTP listener, prover execution, and filesystem operations.
   - **Blast radius:** At minimum port 59834 and Safari/HTTPS-only clients; process-wide descriptor or runtime exhaustion can affect all proving service functions.
   - **Data sensitivity:** No direct disclosure, but the unavailable workload is private transaction-witness proving.
   - **Exploitability:** Local loopback attack vector, low complexity, no special privileges, no authentication, and no user interaction. Any local account or sandbox able to open loopback TCP connections can attempt it.

3. **Evidence confidence.** **High.** Every accepted socket receives an independent task, and the TLS accept future is awaited without any deadline or semaphore.

4. **OWASP / CWE mapping.** [OWASP API4:2023 Unrestricted Resource Consumption](https://owasp.org/API-Security/editions/2023/en/0xa4-unrestricted-resource-consumption/); [CWE-770: Allocation of Resources Without Limits or Throttling](https://cwe.mitre.org/data/definitions/770.html), #25 in the [2025 CWE Top 25](https://cwe.mitre.org/top25/archive/2025/2025_cwe_top25.html).

5. **Trace.**

   - **Source:** Any loopback peer is accepted at [server/tls.rs:59-61](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/server/tls.rs:59).
   - **Allocation:** The acceptor and application are cloned and an unrestricted Tokio task is spawned for every socket at [server/tls.rs:68-71](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/server/tls.rs:68).
   - **Sink:** Each task waits indefinitely for an attacker-controlled TLS handshake at [server/tls.rs:72-77](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/server/tls.rs:72), retaining its socket and task state.
   - **Amplification after exhaustion:** Persistent `accept()` errors are immediately retried without backoff at [server/tls.rs:62-65](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/server/tls.rs:62), allowing descriptor exhaustion to become a CPU-heavy error loop.

6. **Missing control.** There is no bounded semaphore around accepted connections, no TLS-handshake timeout, and no delay or terminal handling for persistent resource-exhaustion errors such as `EMFILE`.

7. **Exploit story.**

   1. The attacker repeatedly connects to `127.0.0.1:59834`.
   2. Each connection sends nothing, or sends only an incomplete TLS record such as `16 03`, and remains open.
   3. The listener accepts every connection and spawns a task blocked in `TlsAcceptor::accept`.
   4. Repeating this until the process descriptor limit is reached prevents legitimate clients from connecting.
   5. Once `accept()` begins returning descriptor-exhaustion errors, the immediate retry loop can additionally consume CPU.

8. **Preconditions.** HTTPS must be enabled and bound on port 59834, and the attacker must be able to make loopback TCP connections. No access to the victim’s files, Tauri IPC, HTTP authorization flow, or trust store is required.

9. **Why mitigations fail.**

   - Loopback binding excludes remote hosts but does not distinguish benign and hostile local processes.
   - Host, Origin, body-size, and proving-concurrency controls are HTTP-layer defenses; the attack stalls before `serve_connection` reaches the router at [server/tls.rs:79-84](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/server/tls.rs:79).
   - The handshake-error branch only releases resources if the handshake eventually returns an error.
   - `bind_with_retry` governs initial listener ownership, not accepted-connection resource consumption.

10. **Instances.** One production instance in scope: the unbounded accept → spawn → incomplete-handshake path at [server/tls.rs:59-77](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/server/tls.rs:59), amplified by the immediate accept-error retry at [server/tls.rs:62-65](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/server/tls.rs:62).