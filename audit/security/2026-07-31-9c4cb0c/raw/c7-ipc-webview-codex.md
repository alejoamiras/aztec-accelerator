Two findings met the reporting bar.

## F-1 — Public `/health` responses are treated as authenticated Accelerator identity

1. **Title.** A hostile loopback listener can impersonate the Accelerator, suppress the Windows instance, and falsely satisfy the updater launch-floor check.

2. **Impact factors.**

   - **Availability:** On Windows, the legitimate Accelerator exits cleanly while the attacker retains port `59833`.
   - **Integrity/authorization:** An unauthenticated process can cause the app to attest that a newly installed version launched successfully and advance the rollback-prevention floor.
   - **Confidentiality context:** The provided SDK map confirms that a recognized health response causes complete private transaction witnesses to be posted to the same endpoint ([sdk.md:69](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/audit/security/2026-07-31-9c4cb0c/raw/repo-map/sdk.md:69>), [sdk.md:99](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/audit/security/2026-07-31-9c4cb0c/raw/repo-map/sdk.md:99>)). Thus the impersonator is positioned to receive privacy-sensitive witnesses. This contextual client path was not independently audited.
   - **Blast radius:** The logged-in user’s Accelerator service, proof requests, and persistent updater state.
   - **Exploitability:** Local loopback attack; low complexity; no administrator privilege or app IPC access; no user interaction beyond launching/using the app. The current version and health schema are public.

3. **Evidence confidence.** High.

4. **OWASP / CWE mapping.** [OWASP A07:2025 Authentication Failures](https://owasp.org/Top10/2025/A07_2025-Authentication_Failures/); CWE-306, Missing Authentication for Critical Function, which is #21 in the [2025 CWE Top 25](https://cwe.mitre.org/top25/archive/2025/2025_cwe_top25.html). CWE-940, Improper Verification of Source of a Communication Channel, is also directly descriptive.

5. **Trace.**

   **Windows instance suppression:**

   - The legitimate server attempts to bind through `server::start` at [main.rs:280](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:280>).
   - An attacker already owning `59833` causes `AddrInUse`, classified at [main.rs:285](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:285>).
   - The attacker-controlled HTTP response enters the scoped code as the Boolean result of `healthy_aztec_on_port()` at [main.rs:294](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:294>) and [main.rs:296](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:296>); the dependency signature is [probe.rs:24](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:24>).
   - That unauthenticated result directly reaches `app_handle.exit(0)` at [main.rs:301](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:301>).

   **False updater-floor attestation:**

   - The tracker accepts a loopback-supplied version from `healthy_aztec_version_on_port()` at [main.rs:347](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:347>)–[main.rs:350](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:350>); dependency signature: [probe.rs:54](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/server/probe.rs:54>).
   - Three matching public strings increment the counter at [main.rs:352](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:352>).
   - The attacker-controlled assertion reaches the persistent security-state sink `commit_launch_floor()` at [main.rs:354](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:354>).

6. **Missing control.** No process/service ownership authentication binds the health response to this application. The floor tracker also lacks an internal readiness signal from the server task that actually performed the successful bind.

7. **Exploit story.**

   1. An unprivileged local process binds `127.0.0.1:59833` before the Accelerator.
   2. It serves a copied health response; for the floor path it supplies the publicly known current application version.
   3. On Windows, the real app sees `AddrInUse`, classifies the attacker as a healthy incumbent, and exits with code zero instead of surfacing the port conflict.
   4. The attacker continues presenting the fixed Accelerator endpoint. A browser client recognizing that endpoint can subsequently send it private witnesses.
   5. Independently, during the first launch of a new version on a platform where the app remains resident after bind failure, three matching responses cause the monotonic floor to be committed even though this build’s own server never became healthy.
   6. A later attempt to return to the previous signed build can then be rejected by rollback protection.

8. **Preconditions.**

   - The attacker can run a local process capable of binding loopback port `59833`.
   - The attacker wins the bind race before application startup.
   - Windows is required for the clean-exit path.
   - The false-floor path requires update polling to be enabled and a build whose floor has not already been committed.
   - Witness exposure additionally requires a browser client to use the fixed endpoint.

9. **Why mitigations fail.** This is not a re-report of the accepted unauthenticated `/health` endpoint. The defect is using that deliberately public signal as authentication for security-critical decisions. Host/Origin controls protect requests received by the genuine server; they do not authenticate the process answering a client probe. Matching `CARGO_PKG_VERSION` and repeating the probe three times add consistency, not authenticity.

10. **Instances.**

   - Windows incumbent decision and clean exit: [main.rs:280](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:280>), [main.rs:285](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:285>), [main.rs:294](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:294>), [main.rs:296](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:296>), [main.rs:301](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:301>).
   - Launch-floor attestation: [main.rs:340](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:340>), [main.rs:347](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:347>), [main.rs:350](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:350>), [main.rs:352](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:352>), [main.rs:354](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:354>).

## F-2 — Update approval is not bound to the version displayed by the prompt

1. **Title.** A later update check can replace the pending artifact while an older version remains displayed, causing the user to install a different executable than the one presented for approval.

2. **Impact factors.**

   - **Integrity/authorization:** Manual update consent is applied to a different `VerifiedUpdate` than the one whose version was displayed.
   - **Blast radius:** The installed native application and everything it can access as the user, including future private witnesses, configuration, persistence entries, and certificate-management operations.
   - **Data sensitivity:** No data is directly disclosed by the race, but the substituted executable inherits the app’s sensitive local role.
   - **Exploitability:** Remote update channel; high timing complexity because the prompt must remain open across another 12-hour poll; requires control of valid signed update publication or two legitimate releases during that interval; one normal user click is required.

3. **Evidence confidence.** High.

4. **OWASP / CWE mapping.** [OWASP A08:2025 Software or Data Integrity Failures](https://owasp.org/Top10/2025/A08_2025-Software_or_Data_Integrity_Failures/); CWE-863, Incorrect Authorization, #17 in the [2025 CWE Top 25](https://cwe.mitre.org/top25/archive/2025/2025_cwe_top25.html). The state-replacement mechanics also match CWE-367, Time-of-check Time-of-use Race Condition.

5. **Trace.**

   - A signed but externally supplied `VerifiedUpdate` enters at [main.rs:237](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:237>).
   - Its version is captured for display at [main.rs:239](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:239>)–[main.rs:240](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:240>), while the object is stored in the singleton `PendingUpdate` at [main.rs:243](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:243>)–[main.rs:245](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:245>).
   - The first version is encoded into the prompt URL at [windows.rs:246](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/windows.rs:246>)–[windows.rs:250](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/windows.rs:250>) and rendered at [update-prompt.js:3](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/frontend-src/update-prompt.js:3>)–[update-prompt.js:6](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/frontend-src/update-prompt.js:6>).
   - The poller repeats at [main.rs:318](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:318>)–[main.rs:324](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:324>). A newer update overwrites the same pending slot at `main.rs:243-245`.
   - The prompt uses the static label `update-prompt` at [windows.rs:255](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/windows.rs:255>). Because that window already exists, `open_or_focus_window` returns without replacing its URL at [windows.rs:65](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/windows.rs:65>)–[windows.rs:69](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/windows.rs:69>).
   - Clicking the old prompt sends only `"update"` and `autoUpdate`, with no version or update token, at [update-prompt.js:8](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/frontend-src/update-prompt.js:8>)–[update-prompt.js:14](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/frontend-src/update-prompt.js:14>).
   - `respond_update_prompt` takes whichever object is currently in the singleton at [commands.rs:731](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/commands.rs:731>)–[commands.rs:734](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/commands.rs:734>) and reaches the install sink at [commands.rs:742](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/commands.rs:742>); dependency signature: [updater.rs:264](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/updater.rs:264>).

6. **Missing control.** The user action carries no immutable prompt/update identifier, and the backend never verifies that the consumed `VerifiedUpdate.version()` equals the version displayed by that window. The pending slot should either remain immutable while its prompt exists or the prompt and response must be rebound to a new unguessable token/version whenever it is replaced.

7. **Exploit story.**

   1. Version `2.0.0` becomes available. The app stores its `VerifiedUpdate` and displays “current → 2.0.0.”
   2. The user leaves the prompt open.
   3. At the next poll, signed version `2.1.0` becomes available. The singleton is overwritten with `2.1.0`.
   4. The existing window is retained and still displays `2.0.0`.
   5. The user clicks “Update.”
   6. The command takes and installs `2.1.0`, despite that artifact never being presented in the approval UI. An over-privileged release actor can use this window to substitute a different signed native payload.

8. **Preconditions.**

   - `auto_update` is `None` or `Some(false)`, so a manual prompt is shown.
   - The prompt remains open across another update check, normally 12 hours.
   - A newer validly signed update becomes available.
   - The user clicks Update on the stale prompt.
   - Malicious exploitation requires update-signing/publication authority; the mismatch can also occur accidentally during consecutive legitimate releases.

9. **Why mitigations fail.** Signature and manifest verification prove both artifacts came from the authorized release channel; they do not prove the installed artifact is the one the user saw. The mutex prevents simultaneous mutation but deliberately permits sequential replacement. The static window label deduplicates prompts without refreshing their displayed state, and the updater floor accepts the newer version rather than detecting the consent mismatch.

10. **Instances.**

   - Singleton pending state: [commands.rs:34](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/commands.rs:34>), [main.rs:568](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:568>).
   - Unconditional replacement: [main.rs:243](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:243>)–[main.rs:245](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/main.rs:245>).
   - Stale static prompt: [windows.rs:65](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/windows.rs:65>)–[windows.rs:69](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/windows.rs:69>), [windows.rs:255](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/windows.rs:255>).
   - Unbound response and latest-object consumption: [update-prompt.js:13](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/frontend-src/update-prompt.js:13>), [commands.rs:716](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/commands.rs:716>)–[commands.rs:717](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/commands.rs:717>), [commands.rs:732](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/commands.rs:732>), [commands.rs:742](</home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/commands.rs:742>).