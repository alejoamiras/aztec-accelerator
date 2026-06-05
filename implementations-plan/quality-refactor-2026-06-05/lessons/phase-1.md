# Phase 1 — cheap wins (lessons)

Each shipped as its own small behavior-preserving PR off main (after Phase 0's char tests merged).

## Q15 — shared AUTH_DECISION_TIMEOUT const (#295, MERGED)
The 60s authorization timeout was duplicated in server.rs (the `/prove` gate) + windows.rs (popup
auto-deny). Extracted `pub const AUTH_DECISION_TIMEOUT: Duration` in server.rs; windows.rs imports it.
**Gotcha (caught after a `--lib`-only test passed):** windows.rs is in the **bin** crate (it imports
`aztec_accelerator::authorization` = the lib by name), so it must reference `aztec_accelerator::server::
AUTH_DECISION_TIMEOUT`, NOT `crate::server::…`. `cargo test --lib` does NOT compile the bin — had to
`cargo check --bin aztec-accelerator` to catch it. Also removed the now-unused `Duration` import in
windows.rs. Behavior-preserving (same 60s); guarded by `prove_returns_403_on_authorization_timeout`.

## Q8 — typed ProveErrorBody (#296, auto-merging)
Replaced the `json!`-macro `/prove` error bodies with a `#[derive(Serialize)] struct ProveErrorBody
{error, message}` and folded the **6 inline `serde_json::to_string(&json!{}).unwrap()` sites** that
bypassed the `json_error` helper onto it. **CRITICAL (codex's final-audit catch): kept returning
`(StatusCode, String)` so the response stays `text/plain`** — NOT `axum::Json`, which would flip
Content-Type → `application/json` and change the SDK's `ky` `HTTPError.data` parsing. Field order
(`error`, `message`) preserved → byte-identical body. Guarded by the Phase-0
`prove_error_responses_stay_text_plain` golden test (the whole reason that test exists).

## Sequencing note
Q15 + Q8 both touch server.rs but in disjoint regions (const at top + L368 vs the error sites L280-410),
so they were developed in parallel and merge cleanly. The remaining Phase-1 items (Q9 commands.rs, Q10
server.rs+main.rs) serialize behind these to avoid same-file churn (codex's point).

## ⚠️ Upcoming — Q9 carries a deliberate BEHAVIOR CHANGE (owner to be told, per Ask B)
`mutate_config` will dedup the 6 lock-mutate-save sites. Five propagate the save error via `?`; the
sixth (`respond_update_prompt`, commands.rs:219) **silently swallows it** (`if let Err(e) = save {
warn!}`). Per Ask B (refactor + ship the fixes, communicate each), Q9 will make that site **propagate**
too — a real, user-visible change (a failed auto-update-pref save now surfaces instead of being lost).
This will be surfaced to the owner explicitly in the Q9 PR, as a separately-labeled commit.

## Q10 — typed ServerStatus enum (#299, auto-merging)
Replaced `StatusCallback = Arc<dyn Fn(&str)>` with `Arc<dyn Fn(ServerStatus)>`.
`enum ServerStatus { Idle, Downloading, Proving }` + `display_text()` (byte-identical to the prior
`"Status: …"` literals) + `is_busy()` (true iff `Downloading|Proving`). The 4 emit sites
(`StatusGuard` drop, download transition, two proving transitions) pass variants; the main.rs tray
consumer matches on `is_busy()` instead of `text.contains("Proving") || text.contains("Downloading")`.
Behavior-preserving: the Phase-0 `prove_success_path_and_status_sequence` test's assertion is
**unchanged** (`["Status: Proving...", "Status: Idle"]`) — now produced via `display_text()`.

**Gotcha 1 (compiler-caught):** the two `cb("Status: Proving...")` sites had **different indentation**
(12 vs 8 spaces), so an `Edit replace_all` on the 12-space literal silently missed the 8-space one.
`cargo test --lib` caught it (`E0308 expected ServerStatus, found &str` at the survivor). Lesson:
after a `replace_all` on a string that recurs at varying indent, grep the literal again to confirm zero
survivors before trusting it.

**Gotcha 2 (process, not code):** `git checkout -b refactor/phase1-q10-server-status` **failed**
("already exists" — a stale branch from a prior abandoned attempt at the pre-Q8/Q9 base), which left
HEAD on `main`. The subsequent commit landed on **local main**, and the explicit-refspec push then
shipped the *stale* branch tip, not the new commit (`gh pr create` → "No commits between"). Fix:
`branch -f <q10> <commit>`, `reset --hard <real-main>`, re-push (fast-forward, since the stale tip was
an ancestor). Lesson: when `checkout -b` can fail on a pre-existing name, verify `git branch
--show-current` before committing — don't assume the branch switched.

**Infra note:** SSH transport to github.com (port 22) was down this session while keys were loaded and
`gh` (HTTPS) worked — so pull/push were routed through `gh`'s credential helper over HTTPS via
`git -c credential.helper='!gh auth git-credential'` (no token-in-URL, no persistent config change).
Diagnosis matters: it was the transport, NOT 1Password/the agent.

## Next
Phase 1 cheap-wins COMPLETE (Q15 #295, Q8 #296, Q9 #298, Q10 #299). Next: **Phase 2 value objects** —
Q3 `AztecVersion` (versions.rs/bb.rs), Q11 `download_bb` split, Q5 SDK extraction. Characterization
tests FIRST per phase. LESSONS_FILE=implementations-plan/quality-refactor-2026-06-05/lessons/phase-1.md
