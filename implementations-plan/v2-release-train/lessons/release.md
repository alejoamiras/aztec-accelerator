# Release phase — lessons

## 2026-08-17 — RC blocked 3× by an unretried N-1 download during a GitHub major incident

**Symptom.** Three `2.0.0-rc.1` publish dispatches failed at the SAME step — "Download latest stable (N-1)
[AppImage/DMG]" — in 2–3 of the 7 updater-smoke jobs each. Coincided with a GitHub major incident (Actions
major-outage; archive/release-asset downloads ~50% error rate for ~4h).

**Why it kept happening.** The N-1 download (`gh release download` in `_e2e-updater.yml` +
`_e2e-updater-linux.yml`) had NO retry. With 7 unretried downloads over a ~30-min run and an elevated
background error rate, losing ≥1 was near-certain. Probing the asset path healthy right before a dispatch was
NOT sufficient — the point-in-time sample doesn't cover the whole run window. Windows didn't flake only
because its download was wrapped in an `if !` guard (still no true retry — luck).

**3-strike protocol (brief).** After the 3rd identical failure I stopped the blind re-dispatch loop, logged
this lesson, and consulted codex before attempt 4 (session resumed from the release-readiness review).

**Fix (this branch).**
- Retry-with-backoff (5 attempts, linear 10/20/30/40s, partials cleared between tries) around the N-1
  `gh release download` in both the linux + darwin legs, failing closed after exhaustion.
- Integrity check per success: `gh` streams to the final path with `O_TRUNC` and verifies neither length nor
  digest, so a transport error exits non-zero (→ retry) but a premature "clean" close could exit 0 with a
  truncated file. Each success is verified against the asset's API sha256 `digest` (exact byte `size` as a
  fallback for a release without one). Written `set -e`/`pipefail`-safe (GitHub's default shell), so a false
  integrity test can't spuriously exit the step.
- Folded in codex release-readiness blocker 2 (E2E-pin): the `_e2e-packaged.yml` caller now passes
  `ref: ${{ github.sha }}` so the draft gate's harness/SDK is pinned to the dispatched commit, not moving
  `refs/heads/main`.
- `release-contract.test.ts` gained 2 guards (retry+digest present in both legs; the SHA pin on the caller),
  mutation-proven (revert → named test fails → restore): 10 pass, 8-pass/2-fail under mutation.

**Deferred (codex-agreed), NOT in this branch.**
- Release-readiness blocker 1 (promote `PUT`+CloudFront-invalidation not failure-atomic; `verify-live-feed`
  skipped if invalidation fails post-PUT). Only affects the `promote-only` dispatch — a hard prerequisite of
  the FIRST real promote (rollback testing included), landed in a pre-promote PR. The publish/RC path has no
  S3 write, so it can't affect RC or stable publication.
- Blocker 3 (finalize digest-read→publish + failed-gate mutable draft) — the previously-accepted B4 residual
  (privileged-writer / operator-error, seconds-wide window).
- Blocker 4 (withdrawn — stale local git state; the fix WAS merged). Blocker 5 (GitHub "Latest" stale — the
  intentional, contract-tested `--latest=false` design; the signed S3 feed is the sole source of truth).

**Takeaway.** Any CI step that fetches from GitHub's asset/codeload CDN must retry — the CDN throttles/errors
transiently even outside named incidents. "Probe healthy then dispatch" is not a substitute for in-run retry.
