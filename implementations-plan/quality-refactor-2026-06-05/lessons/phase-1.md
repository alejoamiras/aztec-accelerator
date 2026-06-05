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

## Next
Q8 merges → Q9 (mutate_config + the swallow fix) → Q10 (ServerStatus enum, coordinated server emit +
main.rs tray consumer; the Phase-0 `prove_success_path_and_status_sequence` test pins the exact
`["Status: Proving...", "Status: Idle"]` strings the enum must reproduce). Then Phase 2+ (value objects,
splits). LESSONS_FILE=implementations-plan/quality-refactor-2026-06-05/lessons/phase-1.md
