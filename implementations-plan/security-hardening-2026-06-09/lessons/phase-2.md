# PR-2 — origin-gate hardening (SEC-01b/c, SEC-04, SEC-05) — #339

Branch `sec/pr2-origin-gate`, rebased onto main after PR-1 (#338) merged. Commits: cb97990 (SEC-01c),
2a73ab4 (SEC-01b), 0bc4bff (SEC-05), 92dcfa5 (SEC-04), a47edf2 (docs).

## What shipped
- **SEC-01c** headless 3-mode `resolve_gating()` (pure helper, 6 tests) — deny-by-default; `--allow-all`/`ACCEL_ALLOW_ALL` opt-in (⊥ `ALLOWED_ORIGINS`); module doc + README rewritten.
- **SEC-01b** renamed `prove_allows_no_origin_only_with_trusted_loopback_host` — the Host guard, not Origin omission, is the no-Origin boundary now.
- **SEC-05** two-tier `/health` via `health_is_detailed()` (Origin absent-or-approved → detailed; present-and-unapproved → minimal). Keeps the no-Origin CI probes on the full body (R11 — the corrected predicate keys on Origin, NOT Host, since post-PR-1 every caller is loopback-Host).
- **SEC-04** (audit C1) `is_approved(origin, approved, auto_approve_localhost)`; config field default **false** (desktop prompt-once) / headless **true**. `is_approved_checks_both` pins flag-off-denies-localhost.

## Gotchas / lessons
1. **SEC-04 rippled the `is_approved` signature** → broke 2 server tests that encoded the OLD localhost-auto-approve (`prove_auto_approves_localhost_origin` + my own SEC-05 test). Both are the INTENDED behavior change: fixed by enabling the flag in their state (the flag-OFF deny is pinned by the `is_approved_checks_both` unit test). Clean separation: unit test = gate logic; integration test = the flag-on path.
2. **`/health` detail tier must key on Origin, not loopback-Host** (the final-codex Critical R11): after PR-1 every request has a loopback Host, so a Host-keyed predicate would leak detail to everyone. Origin (absent-or-approved) is the right discriminant.
3. **Stacked-PR rebase**: PR-2 was branched off PR-1's pre-squash commit. After PR-1's squash-merge, `git rebase --onto main c575913 sec/pr2-origin-gate` cleanly dropped the duplicated PR-1 commit → PR-2 shows only its own diff. Needed a plain `--force` push (the gh-one-shot pushes don't update local tracking, so `--force-with-lease` saw stale info). Own un-shared branch → fine.
4. **SSH still down** — all pushes via the gh-credential HTTPS one-shot; commits unsigned.

## Deferred (noted)
- Desktop **Settings toggle** to opt `auto_approve_localhost` back to silent (the secure default ships; the toggle is power-user convenience). Small follow-up.
- The **SDK minimal-`/health` test** (R11 belt-and-suspenders: minimal body → `available:true` + prove still routes) — the core behavior is server-tested; the SDK graceful-degradation assertion is a small add.

## Status
PR-2 #339 in CI. Mark SEC-01b/c+04+05 ✓ in plan.md + merge when green.
LESSONS_FILE=implementations-plan/security-hardening-2026-06-09/lessons/phase-2.md
