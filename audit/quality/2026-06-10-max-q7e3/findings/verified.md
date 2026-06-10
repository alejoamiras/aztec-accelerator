# Verifier notes — 2026-06-10-max-q7e3

Phase 4 run by the orchestrator (independent re-read before accepting the claim; deviation documented in report Methodology). Spot-verified the top + disputed findings against source:

- **F-01** (high): `try_start_https` (main.rs:55-94) and `enable_safari_support` (commands.rs:151-180) ARE two orderings of the same cert bring-up — confirmed; failure mode is the SEC-08 M1 bug fixed earlier this session. CONFIRMED.
- **F-03** (high): `json_error`→`String` at server.rs:326; `host.rs:69-72` independently re-read → uses `axum::Json(json!{error})` (no `message` field) = the divergent 3rd shape. CONFIRMED.
- **F-13** (cross-model disagreement, resolved): re-read all 3 sites — `mutate_config` (commands.rs:12, propagates), `auth.rs:117-126` (warns), `reset_safari_support` main.rs:98-104 (`let _ =`, swallows). Three diverged save-failure policies CONFIRMED; codex's "resolved/6 sites" was a partial view (missed that core's auth.rs can't import the src-tauri helper). Finding stands; fix = move helper to core/config.rs.
- F-02/F-04/F-05/F-06/F-07/F-08/F-09/F-10/F-11: two-model convergence with matching file:line = verification-grade; confidence high.
- F-12/F-14/F-15: single-source, concrete instances; confidence moderate.
