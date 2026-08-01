## 1. User-writable cache markers allow forged prover binaries to be trusted and executed

1. **Title:** A same-user local process can self-author both a cached `bb` binary and its integrity marker, bypassing download verification.

2. **Impact factors:**

   - **Properties violated:** Confidentiality, integrity, authorization, and potentially availability.
   - **Blast radius:** Every proof using the poisoned version. The executable runs as the accelerator user.
   - **Data sensitivity:** Private transaction witnesses are supplied to the malicious prover.
   - **Attack vector:** Local filesystem access.
   - **Attack complexity:** Low.
   - **Privileges required:** Ability to write as the desktop user, or otherwise write the user’s version-cache directory.
   - **User interaction:** None if the attacker waits for a legitimate proof or triggers an otherwise-authorized localhost request.

3. **Evidence confidence:** High.

4. **OWASP / CWE mapping:** OWASP A08:2021 Software and Data Integrity Failures; CWE-345, Insufficient Verification of Data Authenticity; CWE-494, Download of Code Without Integrity Check.

5. **Trace:**

   - Attacker-controlled marker content is read at [cache_layout.rs:161](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/versions/cache_layout.rs:161).
   - Only public schema, version, platform, and hexadecimal formatting are checked at [cache_layout.rs:165](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/versions/cache_layout.rs:165); the attacker-supplied binary digest is returned at [cache_layout.rs:180](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/versions/cache_layout.rs:180).
   - The attacker-controlled binary is hashed at [cache_layout.rs:197](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/versions/cache_layout.rs:197) and compared only with that attacker-controlled digest at [cache_layout.rs:198](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/versions/cache_layout.rs:198).
   - The resulting pathname is declared verified and returned at [cache_layout.rs:216](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/versions/cache_layout.rs:216).
   - A cache hit bypasses the GitHub download/digest path at [downloader.rs:29](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/versions/downloader.rs:29).
   - The production execution lookup consumes this path at `packages/accelerator/core/src/bb.rs:38`; `tokio::process::Command` executes it at `bb.rs:206` with the private proving workspace/witness prepared by `bb.rs:158,190+`.

6. **Missing control:** The marker has no signature or other authenticity binding unavailable to a hostile local process. Its `archive_sha256` is merely checked for hexadecimal syntax and is never tied back to an authenticated release during cache validation. The cache also provides no process-level isolation from another process running under the same OS identity.

7. **Exploit story:**

   1. A local malicious process chooses a canonical version such as `5.0.0`.
   2. It writes a malicious executable to `~/.aztec-accelerator/versions/5.0.0/bb`.
   3. It computes that file’s SHA-256 and writes `bb.sha256.json` containing the public schema, expected platform, any 64-character lowercase `archive_sha256`, and the malicious binary’s digest.
   4. A proof request selects that version, or a legitimate application later uses it.
   5. Both files agree, so `verify_cached_bb` accepts them and the remote release verification is skipped.
   6. The malicious prover is spawned and can read/exfiltrate the witness, forge or corrupt results, or terminate the accelerator.

8. **Preconditions:** The attacker must be able to modify the cache as the same desktop user, or through an equivalent ACL exposure. The poisoned version must subsequently be selected for proving.

9. **Why mitigations fail:**

   - `0700` directories and `0600` markers isolate other OS accounts, not hostile processes using the same account.
   - SHA-256 establishes consistency between two attacker-writable files, not publisher authenticity.
   - The regular-file check accepts malicious regular files.
   - macOS ad-hoc signing occurs only during the legitimate installation path; a forged cache hit bypasses it.
   - This is independent of accepted SEC-02: no GitHub compromise or network interception is required.

10. **Instances:** `cache_layout.rs:17-18,117-141,149-180,185-218`; cache-verification short circuit at `downloader.rs:27-31`.

## 2. Hash-then-execute pathname race permits post-verification binary substitution

1. **Title:** A hostile local process can replace `bb` after it is hashed but before the returned pathname is executed.

2. **Impact factors:**

   - **Properties violated:** Confidentiality, integrity, authorization, and potentially availability.
   - **Blast radius:** The raced proof request and code executing with the accelerator user’s privileges.
   - **Data sensitivity:** The replacement process receives access to the private witness workspace.
   - **Attack vector:** Local filesystem race.
   - **Attack complexity:** Elevated; requires monitoring and timing an atomic replacement.
   - **Privileges required:** Write access to the cache as the desktop user or an equivalent principal.
   - **User interaction:** None when racing an existing legitimate proof.

3. **Evidence confidence:** Moderate. The check/use gap is explicit; reliability depends on platform timing and filesystem behavior.

4. **OWASP / CWE mapping:** OWASP A08:2021 Software and Data Integrity Failures; CWE-367, Time-of-check Time-of-use Race Condition.

5. **Trace:**

   - The cache pathname is checked as a regular file at [cache_layout.rs:191](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/versions/cache_layout.rs:191).
   - `sha256_file` separately opens and reads that pathname at [cache_layout.rs:102](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/versions/cache_layout.rs:102) and [cache_layout.rs:197](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/versions/cache_layout.rs:197).
   - After the hash matches, the file handle is discarded and only the pathname is returned at [cache_layout.rs:218](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/versions/cache_layout.rs:218).
   - `packages/accelerator/core/src/bb.rs:38` accepts that pathname.
   - A local attacker atomically replaces the pathname.
   - `tokio::process::Command::new` opens and executes the replacement at `packages/accelerator/core/src/bb.rs:206`.

6. **Missing control:** Verification is not bound to the inode or file handle ultimately executed. There is no cache lease preventing replacement and no platform-specific execute-from-verified-handle or equivalent protected-copy mechanism.

7. **Exploit story:**

   1. The attacker leaves a legitimate cached binary and marker in place.
   2. A filesystem watcher observes the verifier reading or closing `bb`.
   3. After the final hash succeeds, the attacker atomically renames a malicious regular file over `bb`.
   4. The verifier returns the unchanged pathname; it does not revalidate its identity.
   5. The proving code opens the new file and executes it with the private witness.
   6. The attacker can restore the legitimate binary afterward to evade later validation.

8. **Preconditions:** The attacker needs write access under the same OS identity, a cached version selected by a proof request, and sufficiently precise filesystem monitoring or repeated attempts.

9. **Why mitigations fail:**

   - `symlink_metadata` and SHA-256 validate only the object present during the check.
   - Returning a `PathBuf` loses the verified file’s identity.
   - Atomic staging protects legitimate publishers from partial installs but does not prevent a hostile writer from performing its own atomic rename.
   - A second verification in `find_bb` still ends before `Command` opens the path; the attacker races the last check.
   - This is distinct from the documented cache-eviction TOCTOU at `downloader.rs:209-214`.

10. **Instances:** The exploitable verification/execution separation is at `cache_layout.rs:191-218`, consumed by `packages/accelerator/core/src/bb.rs:38,206`. Bare post-verification paths are also returned by `downloader.rs:29-31,58`; those APIs similarly provide no identity binding to subsequent consumers.