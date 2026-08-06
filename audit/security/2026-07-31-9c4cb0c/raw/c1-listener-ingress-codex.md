## Unauthenticated health responses are trusted as process identity

1. **Title:** A local port squatter can impersonate a healthy accelerator and influence lifecycle/update decisions.

2. **Impact factors:** Integrity and availability are violated. On Windows, the legitimate accelerator exits cleanly, leaving the attacker-controlled listener in place. On other platforms, an attacker can falsely satisfy the post-update health requirement and commit the monotonic update floor for a build whose server never successfully bound. No direct confidentiality impact is claimed. Attack vector is local TCP; complexity is low; no elevated privileges are required because port 59833 is unprivileged; user interaction is limited to launching, restarting, or updating the app.

3. **Evidence confidence:** High.

4. **OWASP / CWE mapping:** [OWASP A08:2025 Software or Data Integrity Failures](https://owasp.org/Top10/2025/A08_2025-Software_or_Data_Integrity_Failures/); [CWE-346: Origin Validation Error](https://cwe.mitre.org/data/definitions/346.html); also [CWE-306: Missing Authentication for Critical Function](https://cwe.mitre.org/top25/archive/2025/2025_cwe_top25.html), ranked #21 in the 2025 Top 25.

5. **Trace:**

   - The probe sends an unauthenticated request to the current owner of port 59833 at [probe.rs:25](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:25) and accepts its response at [probe.rs:33](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:33).
   - Any 2xx response passes [probe.rs:38](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:38).
   - “Identity” consists only of attacker-supplied `status == "ok"` and `api_version == 1` at [probe.rs:14](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:14), producing `true` at [probe.rs:44](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:44).
   - The mapped Windows caller consumes that result at [main.rs:296](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:296) and exits at [main.rs:301](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:301).
   - Independently, the floor probe accepts an attacker response at [probe.rs:60](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:60), applies the same public shape check at [probe.rs:65](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:65), and returns the attacker-controlled `version` at [probe.rs:68](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:68).
   - Three matching responses satisfy the caller at [main.rs:347](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:347) and commit the launch floor at [main.rs:354](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:354).

6. **Missing control:** There is no authenticated process identity, per-install challenge/secret, or OS-backed single-instance ownership proof. Update health is not tied to a listener successfully bound by the current process.

7. **Exploit story:**

   1. A low-privilege local process binds `127.0.0.1:59833` before app startup.
   2. It serves `HTTP 200` with `{"status":"ok","api_version":1,"version":"<installed-app-version>"}`.
   3. On Windows, after the legitimate bind fails, the response is classified as another accelerator and the legitimate app exits successfully.
   4. On platforms where the floor tracker continues running, the attacker repeats the public version value three times.
   5. The application commits the update floor despite its own server never having demonstrated health.

8. **Preconditions:** The attacker can run a local process and win the port-binding race. The app must start/restart while the attacker holds the port; floor poisoning additionally requires knowing the installed version, which is not secret.

9. **Why mitigations fail:** The five-second bind retry only delays a persistent squatter. A 2xx check and three consecutive responses prove stability, not identity. The version is public and supplied by the responder. The legitimate server’s Host middleware never executes because the attacker implements the responding server.

10. **Instances:** Shared classifier [probe.rs:14](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:14); redundant-instance probe [probe.rs:24](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:24); update-floor probe [probe.rs:54](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:54); public re-exports [server.rs:25](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server.rs:25).

## Health probes buffer an unbounded attacker-controlled response

1. **Title:** A local port owner can force unbounded response-body allocation during health probing.

2. **Impact factors:** Availability is violated through memory exhaustion and process termination; sufficiently large allocations may also pressure the host. The blast radius is the complete desktop accelerator process, not merely the HTTP listener. No sensitive data is exposed. Attack vector is local TCP, complexity is low, privileges are not required, and the user need only start or update the application.

3. **Evidence confidence:** High.

4. **OWASP / CWE mapping:** [OWASP API4:2023 Unrestricted Resource Consumption](https://owasp.org/API-Security/editions/2023/en/0xa4-unrestricted-resource-consumption/); [CWE-770: Allocation of Resources Without Limits or Throttling](https://cwe.mitre.org/data/definitions/770.html), ranked #25 in the [2025 CWE Top 25](https://cwe.mitre.org/top25/archive/2025/2025_cwe_top25.html).

5. **Trace:**

   - Attacker-controlled responses enter at [probe.rs:33](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:33) and [probe.rs:60](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:60).
   - After only a status check at [probe.rs:38](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:38) or [probe.rs:61](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:61), the complete response is collected and deserialized into an unrestricted `serde_json::Value` at [probe.rs:41](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:41) and [probe.rs:64](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:64).
   - In the version path, an oversized `version` string is additionally copied at [probe.rs:68](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:68).

6. **Missing control:** No maximum response size is enforced through `Content-Length` validation or a capped streaming read before JSON parsing.

7. **Exploit story:**

   1. The attacker pre-binds port 59833.
   2. It returns `200 OK` with a chunked JSON body containing a multi-gigabyte padding or `version` string.
   3. The probe accumulates the body in the accelerator’s memory.
   4. Loopback throughput permits substantial allocation before the three-second deadline, potentially terminating the accelerator through OOM.

8. **Preconditions:** The attacker must control port 59833 when a probe runs. The redundant-instance probe is reached after a Windows bind conflict; the version tracker also probes during its post-launch window.

9. **Why mitigations fail:** The three-second timeout bounds elapsed time, not bytes received or allocated. Loopback transfer is fast, and chunked encoding avoids any useful declared size. The 2xx check occurs before the body read and does not constrain it.

10. **Instances:** [probe.rs:41](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:41), [probe.rs:64](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:64), with an additional large-string copy at [probe.rs:68](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:68).

## Browser origins can indefinitely monopolize authorization popups and queue capacity

1. **Title:** Repeated unknown-origin requests lack a creation-rate limit or denial cooldown.

2. **Impact factors:** Availability is violated. An attacker can occupy the desktop authorization UI indefinitely and keep all ten pending-origin slots full, preventing a legitimate new dApp from obtaining authorization. Already-approved origins remain able to pass the origin check. No witness confidentiality breach is claimed without the user approving an origin. Attack vector is a malicious webpage; complexity is low with attacker-controlled subdomains; no privileges are required; user interaction is only visiting and leaving the page open.

3. **Evidence confidence:** High.

4. **OWASP / CWE mapping:** [OWASP API4:2023 Unrestricted Resource Consumption](https://owasp.org/API-Security/editions/2023/en/0xa4-unrestricted-resource-consumption/); [CWE-799: Improper Control of Interaction Frequency](https://cwe.mitre.org/data/definitions/799.html).

5. **Trace:**

   - A browser-controlled `Origin` enters at [auth.rs:24](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/auth.rs:24) and is canonicalized at [auth.rs:36](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/auth.rs:36).
   - An unapproved origin reaches `AuthorizationManager::request` at [auth.rs:64](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/auth.rs:64).
   - Every distinct origin below the ten-origin cap passes [authorization.rs:344](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/authorization.rs:344), is inserted as active or queued at [authorization.rs:247](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/authorization.rs:247), and returns `is_first = true` at [authorization.rs:349](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/authorization.rs:349).
   - `is_first` invokes the desktop popup callback at [auth.rs:69](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/auth.rs:69), with the UI handoff at [windows.rs:172](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/windows.rs:172).
   - Once ten origins are pending, a legitimate eleventh origin is rejected through [authorization.rs:344](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/authorization.rs:344) → [auth.rs:64](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/auth.rs:64).
   - Denial removes all history for that origin at [authorization.rs:274](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/authorization.rs:274), allowing the attacker to immediately recreate it.

6. **Missing control:** There is no global popup-creation rate limit, recently-denied-origin cooldown, or retry budget across resolved requests.

7. **Exploit story:**

   1. A malicious page embeds frames from ten controlled origins such as `https://o0.evil.example` through `https://o9.evil.example`.
   2. Each frame submits an empty POST to `http://127.0.0.1:59833/prove`; the request reaches authorization before payload processing.
   3. All ten origins create authorization windows and occupy every pending slot.
   4. The single-active arbiter serializes them through 60-second decision periods, giving roughly ten minutes of continuous queue occupancy.
   5. Whenever an origin is denied or times out, its frame immediately resubmits, keeping the queue full indefinitely.

8. **Preconditions:** Desktop mode must be active with an `AuthorizationManager` and popup callback; the attacker must control at least one web origin for recurring prompts, or ten origins to guarantee queue starvation. The browser must be able to contact the loopback listener as assumed by the threat model.

9. **Why mitigations fail:** The Host guard passes because the request’s destination Host is legitimately `127.0.0.1:59833`. The ten-origin and sixteen-piggyback caps bound simultaneous memory use but permit exact queue saturation and reset after resolution. The 60-second auto-deny and queue backstop clean up individual requests but impose no cooldown, so the attacker can recreate them immediately. The single-active-popup arbiter prevents concurrent actionable prompts, not continuous prompt occupation.

10. **Instances:** Route exposure [server.rs:273](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server.rs:273); 60-second/queue timing [server.rs:45](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server.rs:45); popup handoff [auth.rs:64](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/auth.rs:64) and [auth.rs:69](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/auth.rs:69); queue limit [authorization.rs:209](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/authorization.rs:209); insert/remove lifecycle [authorization.rs:247](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/authorization.rs:247) and [authorization.rs:274](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/authorization.rs:274); request admission [authorization.rs:324](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/authorization.rs:324).