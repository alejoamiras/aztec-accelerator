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
| E1 | tauri | High | **REAL-BOUNDED → DOCUMENTED-ACCEPTED** | queued auth requests each build a webview + 1s poller. Bounded to MAX_PENDING_ORIGINS (10 windows), non-crashing, only for UNAPPROVED origins. Proper fix (defer webview to promotion) re-architects the 5-round-converged C9 arbiter across the lib/bin boundary (`arm_active_popup` would need to build windows + reconcile double-arming) — disproportionate + regression-risky for a bounded annoyance. **Recommend NOT fixing in code**; owner may opt into the refactor. See rationale below |
| G2 | scripts | High | **SUPPLY-CHAIN-INHERENT** | bb archive + its digest both come from the same (mutable) Aztec release. Inherent trust-in-Aztec; `immutable:false` in fixture never checked. Cheap partial: require immutable releases |
| H3 | infra | Med | **REAL — FIX (tofu, human-applies)** | S3 versioning w/o `noncurrent_version_expiration` → storage-cost exhaustion by a compromised deploy job |
| H4 | infra | rel-block? | **NEEDS-OWNER-VERIFY** | iam.tf conditions on the `workflow` OIDC claim; codex says AWS won't evaluate it → roles unassumable after cutover. `sub` is load-bearing regardless. Best fix ties to GitHub Environments (owner item #1). Verify at the human-gated cutover |

## What the audit did NOT find
No new Critical/High in the core prover, origin-auth, canonicalization, crypto/manifest verification, or
Windows ACL surface. The two scary auth findings (A2/A3) were false positives; chunk C was clean; the
updater's manifest binding is sound. The real signal is in the **supply-chain surface** (CI/scripts/infra)
— mostly key-hygiene + integrity items that overlap already-known owner decisions.

## E1 rationale (why documented-accepted, not code-fixed)
The proper fix — build the auth webview only when its request becomes ACTIVE, keeping queued requests as
arbiter metadata — requires the promotion path (`arm_active_popup`, in the **lib** `commands.rs`) to
BUILD a window, which lives in the **bin** (`windows.rs`) behind the lib/bin split. It would also have to
reconcile its arming with `show_auth_popup_window`'s own active-arming (double-arm risk), and it edits the
single-active-popup arbiter that took FIVE codex rounds to converge. The impact it defends is BOUNDED
(≤ `MAX_PENDING_ORIGINS` = 10 windows), non-crashing, and only for UNAPPROVED origins the user is actively
visiting (they see the popups and simply don't approve). Trading a real regression risk on converged auth
code for a bounded annoyance is the wrong call — the same proportionality that stopped the version-floor
brick. **Owner can opt into the refactor** if the popup-flood UX is deemed worth it; otherwise accepted.

## Remediation status (this PR)
- **FIXED (code/YAML):** F2 (tag `|| true`), G2 (immutable warning + doc), A1 (prove-permit decouple),
  B1/B2 (age-gated reap/eviction), H3 (S3 noncurrent-version expiration — tofu, human applies).
- **DOCUMENTED-ACCEPTED:** E1 (bounded popup flood — rationale above), D1/D2 (updater buffer #345/M6),
  H1 (0-approval branch protection = solo-dev risk-accept).
- **OWNER RUNBOOK** (`../quality-audit-2026-07-24/release-ci-owner-runbook.md`): F1/G1 (CI signing-key
  isolation), H2 (legacy-role retirement), H4 (IAM `workflow`-claim / GitHub-Environments cutover).
- **VERIFIED FALSE POSITIVES:** A2, A3. **CLEAN:** chunk C.
