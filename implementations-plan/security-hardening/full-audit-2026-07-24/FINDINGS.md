# Full-branch codex audit — security-hardening (2026-07-24)

Scope: the entire `main...security-hardening` **code** delta (docs excluded), #400+#401 folded in.
8 chunks, each a codex gpt-5.6-sol xhigh adversarial pass. Every finding **verified by hand** against
the real code before disposition — the chunk boundaries produce false positives (a chunk can't see a
caller/early-return in another chunk).

## Disposition summary

| ID | Chunk | Codex sev | Verdict | Disposition |
|----|-------|-----------|---------|-------------|
| A2 | auth | Med | **FALSE POSITIVE** | headless denies immediately (`auth.rs:58-60`), never reaches the backstop wait |
| A3 | auth | Med | **FALSE POSITIVE** | `respond_auth` DOES route through `resolve_active` (`commands.rs:203`); arbiter is wired |
| C  | acl/certs/recovery | — | **CLEAN** | confirmed |
| D1,D2 | updater | Med | **KNOWN-ACCEPTED** | the #345/M6 unbounded-plugin-buffer residual; the fix (fork/hand-roll downloader) was rejected in a prior round (would make hand-written verify the sole authenticity control). Availability-only; Ed25519 integrity intact |
| H1 | infra | Crit | **OWNER-ACCEPTED** | `required_approving_review_count: 0` = the solo-dev risk-accept decision (release-env item #1) |
| H2 | infra | High | **KNOWN-OWNER** | legacy role = the legacy-role-retirement owner item (#4) |
| F1,G1 | ci/scripts | High/Crit | **PARTLY-KNOWN** | prod signing key in CI smoke-job env; overlaps owner protected-env item #1. Real key-hygiene concern; full fix = ephemeral smoke key + isolated signer |
| F2 | ci | Med | **REAL — FIX** | `_publish-sdk.yml:148-149` `|| true` swallows tag failures → a pre-claimed wrong-commit tag gets blessed. Cheap, mine |
| A1 | auth | Med | **REAL-BOUNDED** | prove permit held during body read → ≤8 slow uploaders 429 others. Bounded by MAX_INFLIGHT_PROVE + 30s timeout. Hardening |
| B1,B2 | versions | Med | **REAL-BOUNDED** | concurrent stage-reap / cleanup can evict another request's in-use version → proof failure + re-download churn. Availability; needs an approved origin |
| E1 | tauri | High | **REAL-BOUNDED** | queued auth requests each build a webview + 1s poller. Bounded to MAX_PENDING_ORIGINS (10 windows). Annoyance/resource, not crash |
| G2 | scripts | High | **SUPPLY-CHAIN-INHERENT** | bb archive + its digest both come from the same (mutable) Aztec release. Inherent trust-in-Aztec; `immutable:false` in fixture never checked. Cheap partial: require immutable releases |
| H3 | infra | Med | **REAL — FIX (tofu, human-applies)** | S3 versioning w/o `noncurrent_version_expiration` → storage-cost exhaustion by a compromised deploy job |
| H4 | infra | rel-block? | **NEEDS-OWNER-VERIFY** | iam.tf conditions on the `workflow` OIDC claim; codex says AWS won't evaluate it → roles unassumable after cutover. `sub` is load-bearing regardless. Best fix ties to GitHub Environments (owner item #1). Verify at the human-gated cutover |

## What the audit did NOT find
No new Critical/High in the core prover, origin-auth, canonicalization, crypto/manifest verification, or
Windows ACL surface. The two scary auth findings (A2/A3) were false positives; chunk C was clean; the
updater's manifest binding is sound. The real signal is in the **supply-chain surface** (CI/scripts/infra)
— mostly key-hygiene + integrity items that overlap already-known owner decisions.

## Remediation plan
- **Fix now (mine, in-code/YAML):** F2 (tag `|| true`), G2-partial (require immutable bb releases).
- **Fix now (tofu, commit+validate, human applies):** H3 (S3 noncurrent expiration).
- **Availability hardening (judgment — bounded, approved-origin-only):** A1, B1/B2, E1 — see owner steer.
- **Owner/infra (known or human-gated):** H1 (accepted), H2/H4 (legacy-role + OIDC cutover), F1/G1 (CI
  signing-key isolation — pairs with protected environments). Rolled into the release-CI owner runbook.
