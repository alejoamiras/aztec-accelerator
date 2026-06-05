# Phase 4 — `AztecVersion` value object (Q3) — lessons

Shipped as PR #300 (two commits: type-first checkpoint, then sink threading), rebased onto the
Q10-main (#299) before merge.

## Design — validation-as-constructor + Deref bounds the blast radius
`AztecVersion { raw, tier, sort_key }` with `parse()` running the exact `is_valid_version` predicate.
The key decision that kept this a *small* behavior-preserving PR: implementing `Deref<Target = str>`
(+ `AsRef<str>` + `Display`). With Deref, `&AztecVersion` **auto-coerces to `&str`** at every
`&str`-taking call site — so `version_bb_path`, `find_bb`, and all of `bb.rs` stayed **untouched**
(they accept `&av` for free). Only the signatures where the type adds real enforcement changed.

## The #99 traversal guard became *structural*
`download_bb(&AztecVersion)` is the win: a value of this type can only have been built by `parse`
(which validated), so an unsafe version literally cannot reach the `remove_dir_all` sink — the bypass
the old sink-side recheck defended against is now impossible to express. Inside `download_bb`, a single
`let version: &str = version;` deref-shadow kept the entire body (8 tracing sites + path/URL calls)
byte-identical, so the signature change was the only diff. The old `download_bb_rejects_unsafe_version_at_sink`
test (which fed `download_bb` invalid strings) became un-expressible — **retargeted** to the ctor as
`unsafe_version_cannot_be_constructed_for_download_sink`, preserving the threat-model intent.

`resolve_version` constructs the `AztecVersion` ONCE at the HTTP ingress (replacing the bare
`is_valid_version` call). Pre-existing char tests `resolve_version_rejects_invalid_version` +
`_passes_valid_version` stayed green through the new parse path — proof the 400-on-invalid contract is
identical.

## Deferred (kept this PR tight)
The plan folds `versions_to_evict` re-parse subsumption (`&[String]` → `&[AztecVersion]`, using the
precomputed `tier()`/`sort_key()`) + the `bb_asset_name` minor into Q3. Split to a follow-up to keep
the sink-threading PR small and behavior-preserving. `tier()`/`sort_key()` are `pub` so they're not
dead-code while unused internally (the char test exercises them).

## Rebase gotchas
1. **Q3↔Q10 `resolve_version` overlap auto-merged cleanly.** Both PRs edit the same function's download
   block (Q10: `cb(ServerStatus::Downloading)`; Q3: `version_bb_path(&version)` / `download_bb(&version)`),
   but on non-adjacent lines, so git's 3-way merge combined them with no conflict. **Auto-merged adjacent
   changes in one function are where silent breakage hides** — verified by re-running the full suite
   (126/126, incl. both `prove_success_path_and_status_sequence` and the Q3 tests together) + eyeballing
   the merged function, NOT by trusting the clean rebase.
2. **1Password commit *signing* failed mid-rebase** ("failed to write commit object / 1Password: failed
   to fill whole buffer") — distinct from the earlier SSH-*transport* outage (that was port 22; this is
   the signing agent). Continued with `git -c commit.gpgsign=false rebase main` per standing guidance.
   The signing-vs-transport distinction matters: same vendor, two independent failure modes this session.

## Next
Phase 4 (Q3) merges → **Phase 5 / Q11** (`download_bb` split — Extract Method, operates on the new
`AztecVersion`; char test the atomic-rename-cleanup first). Then the Q3 follow-up (`versions_to_evict`)
can ride alongside or precede it.
LESSONS_FILE=implementations-plan/quality-refactor-2026-06-05/lessons/phase-4.md
