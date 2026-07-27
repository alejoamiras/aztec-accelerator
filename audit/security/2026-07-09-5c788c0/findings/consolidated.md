# Consolidated Findings (Phase 3 reduce)

Run: 2026-07-09-5c788c0 · Focus: security (privacy leaks / crypto / witness risk) · Effort: high
Coordinator: Fable (main). Dedupe key: root cause + sink + impacted boundary. Cross-model attribution kept as a confidence signal (convergence = strongest evidence).

## Raw-input tally (10 clusters × Claude + Codex)

| Cluster | Claude | Codex | Convergence |
|---|---|---|---|
| sdk-witness-transport | 1 (unauth accel → witness harvest) | 1 (same) | **CONVERGENT** |
| server-ingress-host | 1 (spoofable probe → self-exit) | 1 (same, impostor keeps port) | **CONVERGENT** |
| origin-authz-config | 0 | 1 (trailing-dot collapse) | divergent (Codex only) |
| prove-witness-bb | 1 (witness temp file perms) | 1 (semaphore-after-buffer DoS) | divergent (different roots) |
| crypto-tls-updater | 2 (updater rollback; CA key not zeroized) | 1 (updater rollback) | rollback **CONVERGENT** |
| supplychain-download | 2 (win pin TOFU; download-bb no digest) | 2 (same two) | **CONVERGENT** ×2 |
| tauri-ipc-app | 1 (systemd unit injection) | 2 (global IPC scope; systemd injection) | systemd **CONVERGENT** |
| frontend-trust-ui | 2 (popup overflow; displayName no ASCII) | 0 | divergent (Claude only) |
| headless-server | 1 (localhost auto-approve) | 0 | divergent (Claude only) |
| ci-infra-supplychain | 3 (mutable-tag pin; whole-bucket IAM; OIDC wildcard) | 2 (OIDC wildcard→role; _publish-sdk injection) | OIDC/IAM **CONVERGENT**; injection Codex-only |

## Deduped findings (root cause) → severity assigned at reduce

- **F-001 [HIGH]** SDK sends the private witness to an unauthenticated local server (no server-identity check). *cluster 1; both; high.* Coupled → F-002.
- **F-002 [MEDIUM]** Spoofable `/health` incumbent-probe evicts the real accelerator (Windows `exit(0)`), letting an impostor keep the port. *cluster 2; both; high.* Coupled → F-001.
- **F-003 [MEDIUM]** Private witness written world-readable to a temp file (`tempdir()` = umask-default ~0o755, file ~0o644). *cluster 4 Claude; verified; high.*
- **F-004 [HIGH]** Updater trusts feed-declared `version` unbound from the signed artifact → rollback/downgrade to old signed-but-vulnerable build. *cluster 5; both; high.* Coupled → F-005.
- **F-005 [MEDIUM]** Over-broad deploy trust: wildcard OIDC `sub` (`chore/aztec-*-*`) + unprotected `nightlies` + whole-bucket S3 write shared across 4 pipelines → any deploy-path compromise reaches `releases/latest.json`. *cluster 10; both; high.* Coupled → F-004.
- **F-006 [MEDIUM]** `_publish-sdk.yml` interpolates `dist_tag` dispatch input into `run:` with NPM/GH tokens → command injection / token exfil. *cluster 10 Codex; verified; high.*
- **F-007 [MEDIUM]** `download-bb.ts` populates the runtime bb cache with no integrity check; runtime trusts a pre-existing cache entry forever. *cluster 6; both; high.*
- **F-008 [MEDIUM]** Windows `bb.exe` checksum pin is trust-on-first-use, auto-written by the update script, merged with 0 required reviews. *cluster 6; both; high.*
- **F-009 [MEDIUM]** `/prove` buffers the full 50MB body before acquiring the concurrency semaphore → unbounded concurrent memory (DoS). *cluster 4 Codex; verified; high.*
- **F-010 [LOW]** Linux crash-recovery writes unescaped `current_exe()` into a systemd unit → directive injection (Windows path is escaped; Linux is not). *cluster 7; both; moderate.*
- **F-011 [LOW]** Trailing-dot origin canonicalization collapses `https://x.` into approved `https://x`. *cluster 3 Codex; verified; low exploitability.*
- **F-012 [LOW]** Global Tauri IPC handler not window-scoped + no CSP → any future webview injection reaches trust-changing commands (defense-in-depth; no current sink). *cluster 7/8; Codex; low.*
- **F-013 [LOW]** Headless mode auto-approves every localhost origin regardless of `ALLOWED_ORIGINS` (documented; residual gap on multi-tenant hosts). *cluster 9 Claude; verified; low.*
- **F-014 [LOW]** Authorize popup: unbounded-length origin overflows the fixed-height, non-scrolling popup (word-break wraps → vertical clip) + "Remember" pre-checked. *cluster 8 Claude; verified; low.*
- **F-015 [LOW]** Mutable major-tag pinning of third-party Actions (incl. the AWS-cred action that mints the deploy session) while one action is SHA-pinned. *cluster 10 Claude; low.*
- **F-016 [LOW]** CA signing key not explicitly zeroized (rcgen `zeroize` feature enabled but never invoked; residual in-memory copy). *cluster 5 Claude; moderate confidence; requires local memory read.*

## Dropped / not pursued during reduce

- Classic IDN/punycode homograph of the origin (cluster 8): ruled out — `canonicalize_origin` normalizes via `url`/`idna` before display (pinned test proves no collision).
- Host/`:authority` DNS-rebinding bypass (cluster 2): guard held against absolute-form, authority-disagreement, IPv6 zone, userinfo, trailing-dot, case, layer-order — no bypass.
- `respond_auth` origin self-approval via HTTP (cluster 7): request_id is UUIDv4 and never disclosed outside its own popup window — no bypass today.
- bb subprocess argv/env/stderr witness leak, TOCTOU/symlink on temp path (cluster 4): ruled out — witness travels by file content only; client gets a generic error.
- `ALLOWED_ORIGINS` parse widening (cluster 9): fail-closed on empty/whitespace/wildcard/malformed; `--allow-all` mutually exclusive.
- `verified-sites.json` displayName missing ASCII guard (cluster 8): folded to cross-cutting — ground-truth origin always shown; curator-only vector.
- config persistence TOCTOU, macOS `security` argv injection, version-string path traversal, tar bomb / path traversal in extraction, digest fail-open: all investigated, no concrete trace.
