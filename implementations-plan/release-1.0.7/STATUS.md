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
  │  [✓] 2.1  PR #404 security-hardening → main  183 files               │
  │  [✓] 2.2  CI green ......................... 33 pass / 0 fail        │
  │  [✓] 2.3  MERGED ........................... --exclude fix LIVE      │
  └──────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 3 — DE-RISK THE NEW IAM ROLES  (H4 open question)             │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [✓] 3.0  deploy-landing dispatched → ci-landing role ASSUMED       │
  │  [✓] 3.1  release-accelerator auth_probe=true (no side effects)      │
  │  [✓] 3.2  AWS trust preflight PASSED                                 │
  │  ★ H4 DISPROVEN: AWS DOES evaluate the `workflow` OIDC claim.        │
  │    Per-pipeline isolation works as designed. No iam.tf change needed.│
  └──────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 4 — DRY RUN: RELEASE 1.0.7-rc.1   (feed NOT touched)          │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [~] 4.1  dispatched (run 30125371494)                               │
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
- 2026-07-24 — Phases 1-3 complete. Two real problems caught by verifying rather than trusting:
  (a) the repo is squash-only, so #403's sync never made main's commits ANCESTORS — #404 went
  CONFLICTING even though content was correct. Fixed with a real merge commit whose tree was
  verified byte-identical to the branch (pure ancestry repair, pushed direct to
  security-hardening since another squash would recreate the break). (b) git's auto-merge
  produced a DUPLICATED `permissions:` block in sdk.yml (invalid YAML) — caught by diffing the
  merge result instead of trusting "no conflicts".
  ★ H4 RESOLVED THE OTHER WAY: codex predicted AWS would ignore the `workflow` OIDC claim,
  making the new roles unassumable. Both roles assumed successfully (deploy-landing + release
  preflight). AWS DOES support it; the claim stays. Testing beat the prediction.
