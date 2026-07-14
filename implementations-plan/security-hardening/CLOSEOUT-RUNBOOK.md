# Security-Hardening Campaign — Human-Gated Closeout Runbook

**Status at hand-off (2026-07-14):** every in-scope cluster (C0–C10) is blueprinted → implemented →
post-impl-audited → local + CI green → **merged into `security-hardening`**. The branch is green and
internally consistent. **Nothing below has touched live AWS, live GitHub rulesets/secrets, or `main`.**
"Source + CI complete" is NOT "operationally remediated" — the steps here are what a trusted human
(owner/admin creds) must run to finish the job.

Claude cannot do any of this: it requires owner AWS credentials, GitHub ruleset/secret admin, one-off
auth, and (for F-002) an upstream contract that does not exist yet. Do a brief **deploy/merge freeze**
for the duration of Section A.

Fill in once: `REPO=alejoamiras/aztec-accelerator`, AWS `ACCOUNT_ID` (`aws sts get-caller-identity`),
`RULESET_ID` (captured in A2).

---

## Order of operations (do NOT reorder — each step fails CLOSED, never silently open)

1. **A. Apply F-005 infra** on `security-hardening`'s current source — additive IAM/S3, hardened `main`
   ruleset, three deploy secrets, per-pipeline smoke + the mandatory negative cross-role test. Legacy
   role retired LAST, in a separate post-smoke PR.
2. **B. Read back the live ruleset** (F-008 fail-closed dependency) and learn the Windows-bb-pin
   procedure for future bb bumps.
3. **C. Integrate `security-hardening` → `main`** via a reviewed PR that ALSO reverts the two temporary
   scaffolds (the `security-hardening` CI trigger and any temporary ruleset relaxation).
4. **D. F-002 / C11** stays BLOCKED until F-001's team ships the `InstallationIdentity` contract.

---

## A. F-005 (C5) — infra apply · **full procedure: `clusters/C5-runbook.md`**

That runbook is authoritative and command-complete. Summary of its gates so you know the shape and the
traps before you start:

- **A0 Preflight (read-only):** `tofu init && tofu plan` (expect only the C5 additions, no replace of
  bucket/distribution); confirm squash-or-rebase merge is enabled (linear history needs it); capture
  `RULESET_ID`; check whether `landing/releases/latest.json` still exists (a latent sync-`--delete` bug
  may have removed it); confirm no Org SCP / permission boundary narrows the new roles.
- **A1 Apply additive IAM + S3** (3 roles + policies + lifecycle + versioning; legacy `aws_iam_role.ci`
  trust NARROWED to `main`, not deleted). 0 destroys. Legacy role still backs live deploys → nothing
  breaks yet.
- **A2 Apply the hardened `main` ruleset EARLY** (`gh api --method PUT … --input
  infra/rulesets/main-branch-protection.json`), after backing up the current ruleset JSON. Expect rules:
  deletion, non_fast_forward, required_linear_history, pull_request, required_status_checks; enforcement
  `active`. Never test force-push/deletion against `main`.
- **A3 Wire the three secrets** (`AWS_ROLE_ARN_LANDING`, `AWS_ROLE_ARN_RELEASE`,
  `AWS_ROLE_ARN_PLAYGROUND`) from `tofu output`.
- **A4 Cut the workflow role references over on `main`** — only takes effect once the C5 workflow
  changes reach `main` (that's Section C). Until then live main uses the legacy role. This is WHY the
  legacy role is retired last.
- **A5 Smoke each pipeline** against its new role (landing real; playground real with SDK skipped;
  release via `auth_probe=true` — no tag/release side effects) + IAM `simulate-principal-policy` checks.
- **A5b MANDATORY negative cross-role AssumeRole test (D1):** from a scratch workflow on `main` that is
  NOT "Release Accelerator", attempt to assume the release role ARN and confirm it is **DENIED**. This is
  the ONLY gate that catches a mis-bound `workflow` name-claim — positive smokes + policy simulation both
  pass silently on a wrong/dropped claim. If it SUCCEEDS, STOP and fix before A6.
- **A6 Retire the legacy role** in a SEPARATE post-smoke PR (2 destroys, 0 add), only after A5 + A5b pass
  and no old-role deploy is in flight; then `gh secret delete AWS_ROLE_ARN`.

**Accepted residuals** (documented, not bugs): CloudFront invalidation is distribution-wide (cache-bust
only); the release role can still OVERWRITE (not delete) `latest.json` with garbage/replay — F-004's
client-side manifest verification is the backstop; owner/admin compromise can rewrite rulesets/secrets
(no second human authority in a solo repo — hardware-key 2FA is the out-of-repo lever).

---

## B. F-008 (C7) — Windows bb pin provenance + ruleset readback · **full procedure: `clusters/C7-runbook.md`**

Two human-gated procedures; C7's fail-closed guarantee on `main` depends on BOTH.

1. **Ruleset bypass readback (do this right after A2).** Read back the live "Main branch protection"
   ruleset and confirm: `enforcement=active`, target `refs/heads/main`, `strict=true`, required checks
   include **`Accelerator Status`** (it aggregates the Windows Prebuild/Build Smoke jobs), and
   `bypass_actors` is **empty** (the release-bot App id must NOT appear). If the bot can bypass, a red
   Windows gate could be merged past — remove it before trusting the guarantee.
2. **Adding a Windows bb pin (only when a future bb bump needs one).** Pins are never auto-generated. On
   a bump to an unpinned version the Windows smoke gate goes RED and the bump PR is left OPEN. A human
   downloads `barretenberg-amd64-windows.tar.gz` from the matching upstream release, does a manual review
   (no upstream attestation exists today — `gh attestation verify` 404s), `sha256sum`s it, and adds a
   `{sha256, provenance:"manual-review", note:"…"}` entry to `WINDOWS_BB_CHECKSUMS` in
   `packages/accelerator/scripts/copy-bb.ts`. `manual-review` is a change-detector, NOT proof against a
   compromised publisher. When AztecProtocol adopts `attest-build-provenance`, add the `attestation`
   code path and flip entries; until that verifier exists, an `attestation` entry FAILS CLOSED.

---

## C. `security-hardening` → `main` integration

The campaign built on a temporary integration branch with two scaffolds that must be reverted as part of
the final `main` PR:

1. **Temporary CI trigger:** the gate workflows were made to run on PRs INTO `security-hardening` (C0,
   PR #377). Remove that trigger so the workflows return to their normal `main`/PR-gate triggers.
2. **Any temporary ruleset relaxation** used to allow the campaign's fast-follow merges — restore the
   hardened `main` ruleset (already applied in A2; confirm the final state matches
   `infra/rulesets/main-branch-protection.json`).

Open ONE reviewed PR `security-hardening → main`, squash or merge per the linear-history rule, with the
two reverts included. After it lands, A4's workflow role references become live on `main` — run the A5
landing/playground/release smokes against `main` if you deferred them.

---

## D. F-002 (C11) — BLOCKED on F-001

C11 cannot be built until F-001's team exposes the `InstallationIdentity` contract:

```
InstallationIdentity
  expected_identity() -> trusted local identity/key id
  answer_challenge(nonce, context) -> authenticated response
  verify_challenge(nonce, context, response) -> verified/rejected
```

Protocol (for whoever implements C11 later): fresh 32-byte nonce; domain-separated context
`aztec-accelerator/incumbent/v1`; bind the response to nonce + api_version + port; verify against the
identity from the trusted LOCAL provider (never a key supplied only by the response);
legacy/missing/malformed/replayed ⇒ treat as a "foreign process" (stay resident, surface port-in-use);
only a verified incumbent may permit the Windows `exit(0)` single-instance handoff. F-002 must NOT read
F-001's files or assume their crypto directly. **F-002 cannot be claimed closed until this ships.**

Out of scope throughout, by standing directive: **F-001** (owned elsewhere) and **F-013** (accepted).

---

## Deferred follow-ups (tracked, not blocking closeout)

- **F-003 Windows DACL** (from C2 R2 audit): apply an explicit owner-only Windows ACL to the prove
  workspace + an effective-ACL test (needs Windows CI). The confirmed F-003 vuln is already resolved on
  all platforms by `0o700`/`0o600`-at-creation; this hardens against an out-of-threat-model attacker who
  already controls the per-user data dir. See `lessons/phase-C2.md`.
- **C9 authorize-popup** (codex-ranked): focus-swap stacked-prompt click-steal (MED), extension-scheme
  host validation (LOW), `get_pending_auth` server-authoritative display binding (LOW). See `clusters/C9-plan.md`.
- **C8 desktop secrets**: `enable()→Result` rollback (documented residual); per-platform auto-launch
  formatting is a 3rd-party residual. See `clusters/C8-plan.md`.
- **C2 crash-leftover** prove-workspace cleanup (ACCEPTED residual). See `lessons/phase-C2.md`.

---

## Definition of done (campaign)

- [x] C0–C10 blueprinted → implemented → post-impl-audited → local + CI green → merged into `security-hardening`
- [x] All in-scope findings landed: F-003, F-004, F-005, F-006, F-007, F-008, F-009, F-010, F-011, F-012, F-014, F-015, F-016
- [x] F-001 (owned elsewhere) and F-013 (accepted) never touched
- [x] C11 / F-002 explicitly BLOCKED on F-001, with the contract + protocol captured for later
- [x] Human-gated closeout runbook posted (this file)
- [ ] **Human:** Section A applied + smoked (incl. A5b negative test) — *operational remediation of F-005*
- [ ] **Human:** Section B ruleset readback confirms fail-closed for F-008
- [ ] **Human:** Section C `security-hardening → main` integration PR merged (scaffolds reverted)
- [ ] **Later:** F-002 built + landed once F-001 ships `InstallationIdentity`
