# Merge mechanics — gotchas landing a `gh stack` under a strict ruleset

Landing the six-PR stack hit two mechanical walls that have nothing to do with the code. Recorded so
the next stack does not rediscover them.

## 1. `gh stack submit` creates PRs as DRAFTS

The atomic `gh stack merge` refuses a stack that contains any draft ("nothing to merge: pull request
#N is a draft"). Mark them ready first (`gh pr ready <n>` for each), and give the base PR a real
title — `gh stack submit` auto-titles it from the branch slug ("worktree audit ux neutral fixes"),
and a squash merge uses that as the commit subject.

## 2. Re-running a failed required check POISONS the atomic stack merge

This is the subtle one, and it cost the most time.

`main` here carries a ruleset with `strict_required_status_checks_policy: true` requiring four
aggregate contexts: `App Status`, `SDK Status`, `Accelerator Status`, `Actionlint Status`.

- **GitHub-proper** evaluates a required context by its *latest* check-run. So after a flake-then-pass,
  each PR shows `mergeStateStatus: CLEAN` and looks perfectly mergeable.
- **The atomic stack-merge API** (which GitHub *forces* for stacked PRs — individual `gh pr merge` is
  refused with "must be merged using the asynchronous merge REST API") is stricter: a lingering
  **failed** check-run for a required context on a PR head makes it report that context as "failing"
  and abort the whole all-or-nothing merge — even though the newest run for that context is green.

The failed runs got there from two self-inflicted sources:
- marking six PRs ready-for-review at once fired a thundering herd; some runs flaked/cancelled;
- **the CI babysitter's `gh run rerun --failed`** then produced a *second* run per context, leaving a
  `failure`+`success` pair on every head.

### The rule that follows

On a stack destined for an atomic merge under a strict ruleset, **do not `rerun --failed`**. A rerun
leaves the failed run attached to the head forever. Instead, react to a red required check by pushing
a **fresh commit** (new head SHA → clean single run per context). The failure runs stay behind on the
abandoned SHA and never touch the merge.

Fastest clean path when heads are already poisoned: one real commit on the base branch, `gh stack
sync` to cascade fresh SHAs up the stack, then let each PR settle to a single green run before merging.
