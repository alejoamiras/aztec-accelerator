# Security-Hardening Ledger

Loop source-of-truth. Status: PENDING · IN-PROGRESS · DONE · BLOCKED. Pick the lowest-numbered PENDING whose deps are met. See `plan.md` for tiers, gates, and non-negotiable impl details.

| # | Cluster / branch | Findings | Tier | Deps | Status | PR | Notes |
|---|---|---|---|---|---|---|---|
| C0 | `sechard/ci-integration-gates` | (bootstrap) | light | — | DONE | #377 | gates now run on PRs into security-hardening; 3/4 dispatch green, actionlint dispatch failed on a PRE-EXISTING `shellcheck infra/*.sh` glob bug (no `.sh` in infra/; only bites on dispatch, skips on real PRs) → fold fix into C3 |
| C1 | `sechard/workflow-input-hardening` | F-006 | light | C0 | IN-PROGRESS | — | validate `dist_tag` + env-quote before token steps. GATE 1 /blueprint light in progress |
| C2 | `sechard/core-request-safety` | F-003, F-009, F-011 | mid | C0 | DONE | #379 | MERGED e4e791b. perms-at-creation; permit-before-body+30s timeout + waiter-cap(429); reject trailing-dot. Post-impl audit R1 CHANGES→5 folded; R2 waiter-cap+dotted RESOLVED, #3/#5 folded, #2-Windows-DACL DEFERRED + crash-leftover ACCEPTED (lessons/phase-C2.md). Note: blueprint GATE waived for this retrofit cluster per user direction; C1+ blueprint-first |
| C3 | `sechard/action-pinning` | F-015 | mid | C0 | PENDING | — | SHA-pin all `uses:` incl GitHub-owned; pin actionlint dl; kill mutable bun/rust; **+ fix `shellcheck infra/*.sh` glob (nullglob/guard) discovered in C0** |
| C4 | `sechard/updater-rollback` | F-004 | deep | C0 | PENDING | — | signed manifest envelope in latest.json + monotonic floor |
| C5 | `sechard/infra-deploy-authz` | F-005 | deep | C0 | PENDING | — | 4 scoped roles; landing `--delete` exclude; drop `chore/*` OIDC; human applies |
| C6 | `sechard/bb-cache-integrity` | F-007 | mid | C0 | PENDING | — | staging + digest marker + runtime rehash; fail-closed legacy |
| C7 | `sechard/bb-windows-provenance` | F-008 | mid | C0, C6 | PENDING | — | remove auto-pin; independent provenance; fix `_aztec-update.yml` immediate-merge |
| C8 | `sechard/desktop-platform-secrets` | F-010, F-016 | mid | C0 | PENDING | — | systemd-escape + `systemd-analyze verify`; `Zeroizing<KeyPair>` + early drop |
| C9 | `sechard/authorize-popup-safety` | F-014 | light | C0 | PENDING | — | PSL middle-ellipsis; scrollable; Remember unchecked |
| C10 | `sechard/tauri-trust-boundary` | F-012 | deep | C0, C9 | PENDING | — | externalize scripts; withGlobalTauri:false; CSP; per-window caps |
| C11 | `sechard/incumbent-identity` | F-002 | deep | C0, **F-001 contract** | BLOCKED | — | needs F-001 `InstallationIdentity`; do not build 2nd identity system |

**Sequencing:** C0 first (unblocks CI on PRs into security-hardening). C1–C10 then proceed (cut each branch AFTER the prior merges). C11 stays BLOCKED until F-001's team ships the identity contract.

**Human-gated closeout:** F-005 (`tofu apply` + ruleset API apply + secret cutover + read-back), the temporary `security-hardening` CI trigger/ruleset removal in the final `main` integration PR, and F-002 (F-001 dependency). See `plan.md` → "Human-gated closeout".

**Deferred follow-ups (tracked):**
- **F-003 Windows DACL** — apply an explicit owner-only Windows ACL to the prove workspace + an effective-ACL test (needs Windows CI). Deferred from C2's R2 audit: the confirmed F-003 vuln is resolved on all platforms by `0o700`/`0o600`-at-creation; this hardens against an out-of-threat-model attacker who already controls the per-user data dir. See lessons/phase-C2.md.

## Lessons
Per-cluster debugging logs land in `lessons/phase-<cluster>.md` (Codex consults logged there per the AFK protocol).
