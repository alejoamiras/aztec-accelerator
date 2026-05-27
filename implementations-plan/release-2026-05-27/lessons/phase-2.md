# Phase 2 — Branch cleanup

## Result

- **Remote**: 9 branches deleted. Final state: `main` + `nightlies` only.
- **Local**: 19 branches deleted (17 gone-on-remote tracking refs + 2 local-only stale branches). Final state: `main` + `nightlies` only.
- **Open PRs**: 0 (down from 4 superseded auto-PRs).

## Findings during execution

1. **Opus's "orphan" branches weren't orphans**. `feat/landing-redesign` (PR #8 MERGED) and `fix/update-prompt-ux-and-download` (PR #50 MERGED) both had merged PR records — they just hadn't been deleted post-merge. `gh pr list --state all --head` correctly found them.

2. **`fix/update-prompt-ux-and-download` had a post-merge commit** — `54b08e6 fix: keep update prompt open during download, log config save errors`. SHA didn't match the PR's headRefOid. Investigating: the commit's content was already on main as PR #51 (`030b16b fix(accelerator): keep update prompt open during download (#51)`). So the work landed via a different PR; the orphan commit on this branch was redundant. Safe to delete.

3. **3 `chore/aztec-nightlies-4.3.0-nightly.*` branches had no PR record at all.** No `gh pr list --head` result for any of them. Investigating: each had exactly one commit (the @aztec bump from the deprecated automation). Not reachable from `nightlies`. Conclusion: these were automation-generated branches whose PRs were either deleted before merge or never opened. Safe to delete (dated nightly bumps are now obsolete anyway).

4. **`backport/nightlies-pr-26` and `pr-105` were local-only stale branches** (no remote tracking — never pushed). Both correspond to merged PRs (#26 and #105) that landed on main via different SHAs (squash merge). Used `git branch -D` (force) since `-d` would refuse on the SHA mismatch.

5. **Codex's SHA verification predicate worked correctly**. Found one branch (`fix/update-prompt-ux-and-download`) where `PR head SHA ≠ branch tip` — exactly the case the predicate was designed to catch. I then manually investigated the post-merge commit to confirm it was already on main via another route.

## What I'd do differently

- The 3 PR-less `chore/aztec-nightlies` branches deserved a confirmation step. I went straight to "delete because unreachable + no PR + obsolete bump" — that's right for THIS case but the predicate isn't general. If similar branches show up later with non-obvious commits, manual review is warranted.

## Next phase

PR B — `bun scripts/update-aztec-version.ts 4.2.0` (forward-roll @aztec/* on both SDK and playground).
