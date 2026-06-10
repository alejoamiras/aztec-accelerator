# PR-3 — per-request opaque auth id (SEC-06) — #340

Branch `sec/pr3-auth-id`, rebased onto main after PR-2 merged. Single commit fed755e.

## What shipped
`AuthorizationManager` dual-map resolving by an opaque UUID (`uuid` v1, `v4` feature):
`origin→request_id` (piggyback) + `request_id→{origin,senders}` (resolution). `request()` →
`(rx, request_id, is_first)`; `resolve(request_id, …)`. Cross-package: `ShowAuthPopupCallback`
gains `request_id`; auth.rs gate + windows.rs timeout + the desktop wiring + `respond_auth` all
resolve by id; `show_auth_popup_window` + `authorize.html` thread `requestId` via the popup URL.
`respond_auth` drops the F-02 non-canonical-origin workaround (moot once id-keyed).

## Gotchas / lessons
1. **The signature change is an all-or-nothing compile unit** — `request`/`resolve`/the callback all
   change together, rippling to auth.rs, server.rs (callback type + test helper), commands.rs,
   windows.rs, the desktop wiring (main.rs), and the Rust tests. Can't land the manager in isolation.
2. **The popup-resolving server tests broke** (3) because they `resolve(origin, …)` to unblock the
   auth flow — now a no-op (resolve needs the id). Fix: the test popup-callback forwards `(origin,
   request_id)` (tuple channel), and the tests capture the id from `popup_rx` + resolve by it (via
   `spawn_blocking` so the std::mpsc recv doesn't block the current-thread tokio runtime). The
   `bad_cast` stderr noise is the fake-msgpack prove body — harmless, the test result is `ok`.
3. **Tauri v2 arg convention**: JS `invoke("respond_auth", { requestId })` (camelCase) maps to the
   Rust `request_id` param (snake_case). Confirmed against `update-prompt.html`'s `autoUpdate`→`auto_update`.
4. **The Edit tool needs a Read-TOOL call** before editing — a `sed`/`grep` (Bash) read doesn't
   register, so edits fail "modified since read". Read via the Read tool right before editing.
5. **Stacked rebase again**: PR-3 branched off PR-2's tip (a47edf2); after PR-2 squash-merged,
   `git rebase --onto main a47edf2 sec/pr3-auth-id` cleanly reparented PR-3's single commit. The
   `uuid` v4 feature also propagated a `+ "uuid"` line into src-tauri/Cargo.lock — amended in.

## Status
PR-3 #340 in CI (Rust + Playwright + the WebDriver auth-flow end-to-end). Merge when green.
LESSONS_FILE=implementations-plan/security-hardening-2026-06-09/lessons/phase-3.md
