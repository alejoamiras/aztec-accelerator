# Release 1.0.7 — driving to production

**Goal:** `security-hardening` merged to `main`; `@aztec` bumped `5.0.0-rc.2` → `5.0.1`;
`1.0.7-rc.1` released + verified; `1.0.7` stable released with `latest.json` live and a real
v1.0.6 → v1.0.7 auto-update confirmed.

**Driving mode:** autonomous (owner-authorised 2026-07-24, present + explicit). Stop-and-ask only on
genuine failure or ambiguity — not on routine gates.

## Progress

```
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 0 — INFRA CUTOVER                                             │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [✓] R1  tofu apply ........................ 8 add/1 chg/0 destroy   │
  │  [✓] R3  3 role-ARN secrets wired .......... verified in repo        │
  │  [✓] ..  IAM boundaries proven ............. 8/8 simulations pass    │
  │  [✓] R2  hardened main ruleset ............. 2 rules → 5 rules       │
  └──────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 1 — SYNC THE BRANCH  (main ALREADY has @aztec 5.0.1)          │
  │  ⚠ REVISED: main is 11 ahead (#395-#398 = the 5.0.1 cycle).          │
  │    Branch is on 5.0.0-rc.2. Prove security-code + 5.0.1 TOGETHER     │
  │    on the branch BEFORE it reaches main.                             │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [✓] 1.0  merge resolved ................... 5 conflicts, by hand    │
  │  [✓] 1.0b local gate green ................. bun test 0 + rust 184   │
  │  [~] 1.1  PR main → security-hardening ..... ⛔ GitHub PR OUTAGE     │
  │  [ ] 1.2  full CI green on the COMBINATION                           │
  │  [ ] 1.3  merge the sync                                             │
  └──────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 2 — LAND THE SECURITY WORK ON MAIN                            │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [ ] 2.1  PR security-hardening → main ..... 24+ commits             │
  │  [ ] 2.2  CI green                                                   │
  │  [ ] 2.3  merge .......................... unlocks --exclude fix     │
  └──────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 3 — DE-RISK THE NEW IAM ROLES  (H4 open question)             │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [ ] 3.1  dispatch release-accelerator auth_probe=true               │
  │  [ ] 3.2  confirm AWS trust preflight PASSES                         │
  │           └─ FAIL ⇒ drop the `workflow` claim from iam.tf, re-apply  │
  └──────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 4 — DRY RUN: RELEASE 1.0.7-rc.1   (feed NOT touched)          │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [ ] 4.1  dispatch release-accelerator version=1.0.7-rc.1            │
  │  [ ] 4.2  build 3 Tauri + 4 headless, sign, smoke, tag, release      │
  │  [ ] 4.3  verify: prerelease marked, latest.json NOT uploaded        │
  └──────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 5 — SHIP: RELEASE 1.0.7 STABLE                                │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [ ] 5.1  dispatch release-accelerator version=1.0.7                 │
  │  [ ] 5.2  latest.json uploaded to S3 (new release role)              │
  │  [ ] 5.3  https://aztec-accelerator.dev/releases/latest.json = 200   │
  │  [ ] 5.4  confirm a real v1.0.6 → v1.0.7 auto-update                 │
  └──────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 6 — CLOSE OUT                                                 │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [ ] 6.1  R5b negative cross-role AssumeRole test                    │
  │  [ ] 6.2  R6 retire legacy role (separate PR) + drop AWS_ROLE_ARN    │
  │  [ ] 6.3  ⚠️ OWNER: delete the AWS ROOT access key                    │
  └──────────────────────────────────────────────────────────────────────┘
```

Legend: `[✓]` done · `[~]` in progress · `[ ]` pending · `[✗]` failed/blocked

## Hard rules while driving
- Never force-push `main` or rewrite published history.
- `1.0.7` stable is dispatched ONLY after `1.0.7-rc.1` is fully green (technical gate).
- Any unexpected failure, ambiguity, or a security-relevant surprise ⇒ STOP and report.
- No secret creation/rotation. Root-key deletion is the owner's action.

## Log
- 2026-07-24 — Phase 0: R1 applied (8 added, 1 changed, 0 destroyed); R3 secrets set; IAM
  boundaries verified 8/8. Root cause of dead update feed confirmed: `deploy-landing`'s
  `s3 sync --delete` without `--exclude "releases*"` on `main` (fixed in security-hardening).
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` confirmed NOT needed (v1.0.6 signed without it).
- 2026-07-24 — Phase 1: merged `main` into the branch. NOT a formality: 5 conflicts, all on
  security-sensitive files where main and the branch had diverged in OPPOSITE directions. Resolved
  by hand (ported main's 5.0.0/5.0.1 Windows pins into the branch's F-008 provenance format; kept
  the branch's removal of circular auto-pinning; took main's promote-latest design for
  `_publish-sdk.yml` but re-applied SHA pins + the F2 tag check that main had lost/never had).
  main's new `typecheck:scripts` then caught 4 real type errors in branch scripts — fixed, not
  suppressed. Local gate green. PR creation BLOCKED by a GitHub "Pull Requests: major_outage";
  branch pushed, retry armed.
- 2026-07-24 — Phase 6 (except the owner's root-key deletion) COMPLETE while Phase 4/5 stay blocked
  on Apple. R5 smoke finished: publish-testnet exercised ci-playground-testnet for real (success),
  joining deploy-landing (ci-landing) and the release auth preflight (ci-release-feed). With R5 +
  R5b both satisfied, R6 executed: PR #406 removed aws_iam_role.ci / aws_iam_role_policy.ci /
  ci_role_arn; `tofu apply` destroyed exactly 2 resources (0 add/change); AWS confirms
  NoSuchEntity for aztec-accelerator-ci-github; the three per-pipeline roles remain; the legacy
  AWS_ROLE_ARN GitHub secret is deleted. The audit's H2 finding (legacy role trusted every workflow
  on main with whole-bucket write, bypassing the landing role's release-feed Deny) is now CLOSED.
- 2026-07-24 — Retry (run 30128083494) to test whether the agreement had been accepted in the
  meantime: IDENTICAL failure. `Build Tauri bundle` -> `failed to notarize app: HTTP 403 - A
  required agreement is missing or has expired`. Cancelled on detection to cap macOS runner spend
  (10x billing); verified again no partial state (0 tags, 0 releases for 1.0.7). Two independent
  runs is conclusive — no further retries until the owner confirms the agreement is accepted.
  STATUS: 5/9 goal items complete. Items 4-7 (rc.1 green -> 1.0.7 stable -> latest.json 200 ->
  auto-update verified) are gated on an Apple-account action with no in-repo workaround. The only
  technical bypass would be disabling notarization, which ships macOS builds Gatekeeper blocks on
  users' machines — deliberately NOT done.
