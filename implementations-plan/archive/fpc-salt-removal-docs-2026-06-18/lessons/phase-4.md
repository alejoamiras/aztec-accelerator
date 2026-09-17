# Phase 4 — Land the PR (2026-06-19)

**PR #366** → squash-merged to `main` as `ba3aec0`. All gates green (lint, typecheck, unit, **salt-less** SDK+App local-network e2e, WebDriver macos/linux/windows, Rust, Clippy, actionlint, smokes). The salt-less e2e passing is the live proof the removal is a true no-op for CI.

## Gotchas caught during P4 (the real work of this phase)

- **Scope contamination:** my P2 commit used `git add -A`, which swept the untracked `audit/security/**` (8 files, ~900 lines, unrelated security-audit reports from session start) into the commit. Caught it pre-push via `git diff --stat`. Excised cleanly with `reset --hard <base>` + `cherry-pick P1`, `cherry-pick P2 → git rm -r --cached audit/security/ → commit --amend` (P2 was HEAD at that moment, so the amend was exact), `cherry-pick P3`. Result: 3 clean commits, net 24 files / +274−65, no `audit/`. **Lesson: never `git add -A` with untracked unrelated dirs present — stage by path.**
- **Stale `origin/main` ref:** the local `origin/main` tracking ref was 3 commits behind real main (still at #363). Root cause (again): `git fetch <URL> main` updates **`FETCH_HEAD` only**, NOT `refs/remotes/origin/main`. Verified real main via `git ls-remote` + `gh pr view 364/365 --json state` (both MERGED). The branch base was actually correct (built on real main `0fc6ffd`); the alarm was purely the stale ref. Fixed with `git update-ref refs/remotes/origin/main <FETCH_HEAD>`. **Lesson: after `fetch <URL> <branch>`, the truth is `FETCH_HEAD`; `ls-remote` is the authoritative cross-check.**

## Post-impl review
- `/code-review max --fix` on the #366 diff: **clean, no findings.** Reviewed all angles directly (diff is ~9 lines of logic + YAML deletions + docs — disproportionate to fan out 10 agents): explicit `new Fr(0)` (no falsy-zero), `Fr`/`salt` still used everywhere, `execSync` removed cleanly, reusable-workflow `secrets:` removed symmetrically (decl + callers), no `/Users/` paths in docs.
- Codex post-impl audit (xhigh, adversarial) — verdict "safe for currently verified targets":
  - **Med (fixed):** `RELEASE_RUNBOOK` pre-flight said `bun run --cwd packages/sdk test`, but `packages/sdk` has no `test` script (only `test:lint`/`test:unit`) → would hard-fail. Corrected to the real scripts.
  - **Low (fixed):** `packages/sdk/README.md` bare `npm install` gave no hint that the 5.0 line is on the `testnet` dist-tag → added a one-line note (root README already had it).
  - **Med (accepted, NOT reverted):** removing the salt override means a *future* custom `AZTEC_NODE_URL` without a funded salt=0 FPC would fail sponsored txs with no escape hatch. This is the **approved scope** ("always salt=0" — every targeted network has salt=0 funded). Per the standing rule, codex is advisory and can't override approved plan scope; surfaced + documented, not actioned. If a non-salt=0 network is ever targeted, re-introduce a config knob then.
  - **Confirmed fine by codex:** no dangling workflow/secret breakage (symmetric removal); security posture improved (smaller trust surface); the headless README "no Tauri/WebKit" claim verified against `server/Cargo.toml` → `accelerator-core`.

LESSONS_FILE=implementations-plan/fpc-salt-removal-docs-2026-06-18/lessons/phase-4.md
