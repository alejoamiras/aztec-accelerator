# Release 1.0.7 — driving to production

**Goal:** `security-hardening` merged to `main`; `@aztec` 5.0.1 carried through; `1.0.7-rc.1`
released + verified; `1.0.7` stable released with `latest.json` live and a real v1.0.6 → v1.0.7
auto-update confirmed; legacy IAM role retired.

**Status: 5 / 9 goal items COMPLETE. Items 4–7 BLOCKED on an Apple account action.**

## Progress

```
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 0 — INFRA CUTOVER                                    COMPLETE │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [✓] R1  tofu apply ........................ 8 add/1 chg/0 destroy   │
  │  [✓] R3  3 role-ARN secrets wired                                    │
  │  [✓] ..  IAM boundaries proven ............. 8/8 policy simulations  │
  │  [✓] R2  hardened main ruleset ............. 2 rules → 5 rules       │
  └──────────────────────────────────────────────────────────────────────┘
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 1 — SYNC BRANCH WITH MAIN (@aztec 5.0.1)             COMPLETE │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [✓] 1.1  PR #403 (survived a GitHub PR-service outage)              │
  │  [✓] 1.2  CI green on the COMBINATION ...... 32 pass / 0 fail        │
  │  [✓] 1.3  merged + ancestry repaired ....... 5.0.1 parity verified   │
  └──────────────────────────────────────────────────────────────────────┘
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 2 — LAND SECURITY WORK ON MAIN                       COMPLETE │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [✓] 2.1  PR #404 .......................... 183 files               │
  │  [✓] 2.2  CI green ......................... 33 pass / 0 fail        │
  │  [✓] 2.3  MERGED ........................... feed --exclude fix LIVE │
  └──────────────────────────────────────────────────────────────────────┘
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 3 — DE-RISK THE NEW IAM ROLES (H4)                   COMPLETE │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [✓] 3.1  deploy-landing → ci-landing ASSUMED                        │
  │  [✓] 3.2  release auth_probe → preflight PASSED                      │
  │  ★ H4 DISPROVEN: AWS DOES evaluate the `workflow` OIDC claim.        │
  └──────────────────────────────────────────────────────────────────────┘
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 4 — DRY RUN 1.0.7-rc.1                       ⛔ BLOCKED       │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [✓] linux + windows + 4 headless + WebDriver gate: PASS             │
  │  [✗] macOS x2: notarization HTTP 403 — APPLE AGREEMENT EXPIRED       │
  │  [✓] fail-closed: no tag, no release, latest.json untouched          │
  │  ⛔ confirmed twice — runs 30125371494 and 30128083494               │
  └──────────────────────────────────────────────────────────────────────┘
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 5 — SHIP 1.0.7 STABLE                        ⛔ gated on P4   │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [ ] latest.json → 200 (currently 403)                               │
  │  [ ] real v1.0.6 → v1.0.7 auto-update confirmed                      │
  └──────────────────────────────────────────────────────────────────────┘
  ┌──────────────────────────────────────────────────────────────────────┐
  │  PHASE 6 — CLOSE OUT                          COMPLETE (except 6.3)  │
  ├──────────────────────────────────────────────────────────────────────┤
  │  [✓] 6.0  R5 smoke: all 3 roles exercised for real on main           │
  │  [✓] 6.1  R5b isolation PROVEN (absent key ⇒ StringEquals false)     │
  │  [✓] 6.2  R6 legacy role DELETED (#406) + AWS_ROLE_ARN dropped       │
  │  [ ] 6.3  ⚠️ OWNER: delete the AWS ROOT access key                    │
  └──────────────────────────────────────────────────────────────────────┘
```

## ⛔ THE ONLY BLOCKER — OWNER ACTION

Apple refuses notarization:
`failed to notarize app: HTTP 403. A required agreement is missing or has expired.`

Both macOS architectures, two independent runs ⇒ **account-level**, not code. Code signing itself
succeeded (cert found, binaries signed); only Apple's notary service rejects.

**Fix:** sign in at **developer.apple.com as the Account Holder** → accept the pending Apple
Developer Program License Agreement (under *Membership* or *Agreements, Tax, and Banking*).

**There is no in-repo workaround.** The only technical bypass is disabling notarization, which
ships macOS builds that Gatekeeper blocks on users' machines — deliberately NOT done.

## RESUME PATH (after the agreement is accepted — no code changes needed)

```bash
# 1. dry run (prerelease: latest.json untouched)
gh workflow run release-accelerator.yml --ref main -f version=1.0.7-rc.1
# 2. verify fully green, THEN ship stable
gh workflow run release-accelerator.yml --ref main -f version=1.0.7
# 3. verify the feed is restored
curl -sI https://aztec-accelerator.dev/releases/latest.json   # expect 200
# 4. confirm a real v1.0.6 -> v1.0.7 auto-update
```

## Key findings from this campaign
- **Dead update feed root cause**: `deploy-landing`'s `s3 sync --delete` had no
  `--exclude "releases*"`, so every landing deploy wiped `landing/releases/latest.json`. Fixed on
  main, and the landing IAM role is now explicitly DENIED write to the release feed.
- **H4 disproven**: the audit predicted AWS would ignore the `workflow` OIDC claim, leaving the new
  roles unassumable. Both roles assumed successfully ⇒ the claim IS evaluated. Isolation kept.
- **H2 closed**: legacy role (trusted every workflow on main, whole-bucket write) deleted.
- **`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is intentionally unset** — v1.0.6 signed fine without it;
  the key is passphrase-less. Setting it would BREAK signing.
- Merge hazards caught by verifying rather than trusting: squash-only policy silently broke merge
  ancestry; git auto-merge produced a duplicated `permissions:` block in `sdk.yml`; 4 real type
  errors in branch scripts, one of which could have made `rmSync` delete the whole versions dir.
