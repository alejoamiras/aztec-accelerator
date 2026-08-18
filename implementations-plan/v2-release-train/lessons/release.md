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

## 2026-08-18 — 2.0.0-rc.2 PUBLISHED; blocker 1 (promote failure-atomicity) fixed (PR #463)

**RC published.** After ~11 machinery bugs fixed across 15 publish dispatches (rc.1 attempts 1–14 burned that
tag at pre-fix SHAs; append-only ⇒ rc.2), `2.0.0-rc.2` published clean: all builds, 3-OS pre-release gate,
7 updater smokes, draft + staged installers, ALL 4 packaged-E2E legs (both composed native-bb-over-HTTPS
proofs, 1.0.7 upgrade+migration, uninstall), tag, finalize. The finalize fix (#462) — resolve the DRAFT
release by `databaseId` (a draft 404s on `GET /releases/tags/{tag}`) — was the last-uncovered step.

**Blocker 1 fixed (pre-promote, #463).** The promote flip did the S3 PUT and CloudFront invalidation in ONE
`set -e` step; a transient invalidation error post-PUT exited RED, skipping `verify-live-feed` while the feed
was live. Fix: split into an authoritative PUT (retry 5×, fatal on exhaustion — safe, feed unchanged, nothing
downstream ran) and a best-effort invalidation (retry 3×, then warn + continue, never fatal). Codex
release-readiness re-review (session 01a0143a) then raised a residual HIGH: `aws s3 cp` can error/time-out
AFTER S3 commits, so the PUT has the SAME skip-ambiguity. Folded: `verify-live-feed` now runs on its OWN
status function (`always() && !cancelled() && validate-green && promote-only && !dry_run`), making the LIVE
FEED the single source of truth after any attempted promote — a PUT that truly didn't land polls ~600s and
fails RED; one that landed despite a RED `promote` is confirmed GREEN. release-contract.test.ts +1 test
(non-fatal-invalidation + own-always-verify), both halves mutation-proven.

**Deferred (codex-agreed, documented — NOT blocking GA).**
- `stale-if-error=0` on the feed's Cache-Control: hardens rollback (don't serve stale-bad during an S3
  outage) but HURTS the common case (turns a stale-but-valid serve into an error). Net-negative for a feed
  that's ~always serving good bytes; the updater already fails safe on a fetch error. Not adopted.
- Single-PoP verification: `verify-live-feed` checks one edge; other PoPs + browser caches hold their own
  ≤300s TTL. Inherent to any single-runner CDN check; invalidation (when it fires) clears edges globally.
- Sub-5min rollback SLA via captured invalidation ID + `aws cloudfront wait invalidation-completed`: no such
  SLA is specified; ~300s max-age propagation is the accepted rollback bound, and the always() live-verify
  already confirms it. Revisit if a tighter rollback SLA is ever required.

**Prior blocker status.** Blocker 2 (E2E SHA-pin) folded earlier. Blocker 3 (finalize digest-read→publish
window) — accepted B4 residual (privileged-writer, seconds-wide). Blocker 4 withdrawn. Blocker 5 (GitHub
"Latest" stale) — intentional `--latest=false`; the signed S3 feed is the sole source of truth.
