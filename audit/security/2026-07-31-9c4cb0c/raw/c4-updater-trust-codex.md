## F-01 — F-004 remains incomplete: historical signed releases can be replayed as current

1. **Title.** An attacker controlling the update feed—but not the signing key—can replay an exact, previously signed release whenever its version remains above the victim’s installed floor. This is distinct from the fixed high-version/old-artifact splice: the entire historical envelope and artifact are replayed unchanged.

2. **Impact factors.**

   - **Integrity:** a withdrawn, vulnerable, or alternate-channel signed build can be installed despite no longer being the currently authorized release.
   - **Authorization:** feed-write access effectively grants authority to select any historical signed release, although it should not grant release-signing authority.
   - **Availability:** the attacker can withhold the current security release and keep affected clients on the replayed build.
   - **Confidentiality:** not directly violated by replay alone, but a replayed vulnerable build regains access to the application’s private transaction witnesses and its CA, persistence, and executable-management privileges.
   - **Blast radius:** every installation consuming the compromised feed whose `current`, `floor`, and `pending` values remain below the replayed version; the entire desktop application on each affected host.
   - **Exploitability:** remote supply-chain/feed attack; low complexity once an old signed feed and artifact are available; requires feed/CDN publication control but **not** the updater private key or local privileges. No user interaction when auto-update is enabled; otherwise the user must accept the normal update prompt. The attacker cannot introduce arbitrary bytes—only a legitimately signed historical build.

3. **Evidence confidence.** **High.** The signed date is checked only for equality and then discarded, while the persistent gate compares only SemVer against versions already run or committed.

4. **OWASP / CWE mapping.** [OWASP A08:2025 — Software or Data Integrity Failures](https://owasp.org/Top10/2025/A08_2025-Software_or_Data_Integrity_Failures/); [CWE-345 — Insufficient Verification of Data Authenticity](https://cwe.mitre.org/data/definitions/345.html), specifically missing freshness/currentness verification. No more-specific CWE from the current Top 25 fits signed-metadata replay.

5. **Trace.**

   1. Untrusted feed data enters through `updater.check().await` at [updater.rs:220](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/updater.rs:220). The scoped code states that the plugin decides whether an update is newer using `version` at [update_manifest.rs:3](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/update_manifest.rs:3).

   2. The returned release is passed into `verify_and_gate` at [updater.rs:239](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/updater.rs:239), which forwards its raw feed, version, URL, and signature at [updater.rs:169](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/updater.rs:169).

   3. The historical manifest and signature are extracted and decoded at [update_manifest.rs:132](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/update_manifest.rs:132) and cryptographically verified at [update_manifest.rs:160](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/update_manifest.rs:160). An unmodified historical signature therefore succeeds.

   4. `pub_date` is only compared with the equally historical outer value at [update_manifest.rs:185](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/update_manifest.rs:185); it is never parsed, compared with an expiry/current time, or checked against persisted metadata. The returned result contains only version and size at [update_manifest.rs:220](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/update_manifest.rs:220).

   5. Layer B is invoked at [updater.rs:184](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/updater.rs:184). Its decision at [updater_state.rs:118](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/updater_state.rs:118) accepts any candidate strictly above `current` and `floor` and not below `pending`; a missing state explicitly accepts it at [updater_state.rs:124](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/updater_state.rs:124).

   6. A `VerifiedUpdate` is created at [updater.rs:189](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/updater.rs:189) and automatically handed to installation at [updater.rs:244](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/updater.rs:244).

   7. The install-time check at [updater.rs:304](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/updater.rs:304) repeats the same non-freshness-aware Layer B policy. The authentic historical artifact is downloaded at [updater.rs:338](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/updater.rs:338), recorded as pending at [updater.rs:394](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/updater.rs:394), and installed at the security-impacting sink [updater.rs:533](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/updater.rs:533).

6. **Missing control.** There is no trusted freshness policy: no signed expiry, monotonically increasing repository/snapshot sequence, persisted highest-seen signed release, or stable/prerelease channel binding. Preventing first-contact and long-term replay generally requires signed short-lived timestamp/snapshot metadata plus persistent rollback state, rather than merely signing `pub_date`.

7. **Exploit story.**

   1. Version `1.0.8` is legitimately signed and released, but is later withdrawn because of a security regression fixed in `1.0.9`. Its feed and artifact remain retrievable or were previously captured.
   2. A victim still runs `1.0.7`, with `floor = 1.0.7` and no pending update.
   3. An attacker with write access to `latest.json` replaces it with the exact signed `1.0.8` envelope, platform entry, and artifact signature.
   4. `1.0.8 > 1.0.7`, so the plugin reports an update. Every signature and outer-envelope comparison passes because nothing was modified.
   5. Both version-floor checks pass, and auto-update installs the genuine but withdrawn `1.0.8` binary.
   6. By continuing to replay that feed, the attacker prevents delivery of `1.0.9` and leaves the host on the vulnerable build.

8. **Preconditions.**

   - A historical signed release above the victim’s current/floor version exists and is withdrawn, vulnerable, or unauthorized for that client’s channel.
   - Its signed metadata and artifact remain available or were captured.
   - The attacker controls the update feed response or publication path, as assumed by F-004’s feed-writer threat model.
   - Auto-update is enabled, or the user accepts the apparently legitimate update prompt.

9. **Why mitigations fail.**

   - Minisign proves the historical envelope and artifact were once authorized; it does not prove they are current.
   - Exact outer-envelope binding defeats field splicing but not byte-for-byte replay.
   - The signed `pub_date` has no freshness semantics and is discarded after equality checking.
   - `floor` and `pending` cover releases already run or committed, not a newer security release that the victim has never observed.
   - Artifact signature and size checks correctly accept the authentic historical artifact.

10. **Instances.**

   - Temporal metadata accepted without freshness enforcement and omitted from the verification result: [update_manifest.rs:38](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/update_manifest.rs:38), [update_manifest.rs:54](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/update_manifest.rs:54), [update_manifest.rs:185](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/update_manifest.rs:185), [update_manifest.rs:220](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/update_manifest.rs:220).
   - Persistent state and acceptance policy track only `floor` and `pending`: [updater_state.rs:42](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/updater_state.rs:42), [updater_state.rs:118](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/core/src/updater_state.rs:118).
   - The same incomplete policy is used at check time and install time: [updater.rs:169](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/updater.rs:169), [updater.rs:184](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/updater.rs:184), [updater.rs:304](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-1/.claude/worktrees/harden-security-bugs/packages/accelerator/src-tauri/src/updater.rs:304).