# Phase 1 lessons — docs-truth PR (2026-08-26)

**Outcome: PR #479, codex loop converged (reject → reject → conditional → approve).**

## The finding worth remembering: immutable docs need version-free prose
The first draft "fixed" the stale callout by naming the new line — "`latest` tracks the current
stable Aztec line (`5.2.x`)". Codex killed it: `packages/sdk/README.md` is packed into the tarball,
and at the moment 5.2.0 publishes, `latest` is still 5.0.1. The sentence would ship permanently
false, become true only after the promote, and go false again after any rollback. **A doc that
ships inside an artifact must not assert anything about state that changes after the artifact is
built.** The fix is to state the invariant instead — SDK `X.Y.Z` targets Aztec `X.Y.Z`, which holds
because the publish version is derived from the pinned `@aztec/stdlib` — and describe dist-tags by
role, never by value.

## Two mechanical traps the review caught
1. **`prepare-sdk-publish.ts`'s manifest argument is RELATIVE** (`process.argv[3] ?? "package.json"`).
   Documenting the local verification as "run it, then `npm pack`" from the repo root would rewrite
   the ROOT manifest while the cleanup restored the SDK's. The recipe must `cd packages/sdk` exactly
   like CI. Corollary caught in the same loop: a bare `npm pack` (no rewrite) ships `0.0.0` with
   source `exports` — it can pass while the real published shape is broken, so it proves nothing.
2. **A pasted `&&` chain has no cleanup.** `set -e` doesn't apply to an interactive shell and an
   `EXIT` trap fires only when that shell closes — so a mid-way failure leaves the manifest
   rewritten. Recipes that mutate tracked files must be a self-contained subshell:
   `( set -euo pipefail; <clean-check>; trap '<restore>' EXIT; … )`.

## Failure-triage asymmetry (publish vs promote)
"Read the registry before classifying" was written for publishes (does the version exist?) and
silently didn't cover promotes, where the question is different: did the TAG move? A promote can
move `latest` and still go red on a trailing step. Correct triage compares the observed `latest`
against the requested version — equal = landed, don't re-dispatch; old = didn't land; anything else
= stop. Same principle, different observable.

## CI trigger gotcha: push + `gh pr create` in quick succession → ZERO runs
The branch push and PR creation landed seconds apart and GitHub registered **no** workflow runs at
all (`gh pr checks` → "no checks reported", `check-runs` total_count 0, while other branches were
triggering runs normally, so Actions was healthy). An empty `chore: trigger CI` commit produced the
full fleet immediately. Distinct from the known unmergeable-PR case (this PR was MERGEABLE) — when
a fresh PR shows zero checks, push an empty commit before investigating anything deeper.

## Smaller corrections
- `bunfig.toml`'s `minimumReleaseAgeExcludes` is a list of EXACT package names, not an `@aztec/*`
  glob — so "never widen" was wrong advice: a bump pulling in a new `@aztec/*` package legitimately
  needs its exact name added.
- Revision suffixes have two forms: `-revision.N` (stable base) and `.N` (prerelease base).
- Publishing does not make range consumers resolve the new version "immediately" — it makes it
  ELIGIBLE for fresh/unlocked installs; existing lockfiles keep their resolution.

LESSONS_FILE=implementations-plan/sdk-5-2-0-release/lessons/phase-1.md
