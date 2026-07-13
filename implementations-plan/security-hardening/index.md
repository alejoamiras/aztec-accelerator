# Security-Hardening Ledger

Loop source-of-truth. Status: PENDING · IN-PROGRESS · DONE · BLOCKED. Pick the lowest-numbered PENDING whose deps are met. See `plan.md` for tiers, gates, and non-negotiable impl details.

| # | Cluster / branch | Findings | Tier | Deps | Status | PR | Notes |
|---|---|---|---|---|---|---|---|
| C0 | `sechard/ci-integration-gates` | (bootstrap) | light | — | PENDING | — | add `security-hardening` to gate `pull_request.branches`; dispatch all 4 workflows |
| C1 | `sechard/workflow-input-hardening` | F-006 | light | C0 | PENDING | — | validate `dist_tag` + env-quote before token steps |
| C2 | `sechard/core-request-safety` | F-003, F-009, F-011 | mid | C0 | PENDING | — | perms-at-creation; permit-before-body+timeout; reject trailing-dot |
| C3 | `sechard/action-pinning` | F-015 | mid | C0 | PENDING | — | SHA-pin all `uses:` incl GitHub-owned; pin actionlint dl; kill mutable bun/rust |
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

## Lessons
Per-cluster debugging logs land in `lessons/phase-<cluster>.md` (Codex consults logged there per the AFK protocol).
