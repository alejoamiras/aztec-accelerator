# PR-4 — pre-flight updater size cap (SEC-03) — #341

Branch `sec/pr4-updater-cap` off main (independent of PR-3). Commit 43bd5ab.

## What shipped (the R3 downgrade)
Kept the plugin's verified `update.download()` intact; added a **pre-flight** reject when the feed's
advertised `size` exceeds a 500 MB ceiling, before the plugin buffers the artifact. `size_from_feed`
(pure, 2 tests) reads it from `Update.raw_json` by matching `download_url`. latest.json generation now
emits `size` per platform (asserted present). NO reqwest rewrite / re-implemented minisign — that was
the net-negative both auditors flagged (a verify bug there = signature bypass).

## Lessons
- `tauri_plugin_updater::Update` exposes `download_url: Url` + `raw_json: Value` (verified in the
  plugin source) — extra latest.json fields survive in raw_json, so adding `size` is safe + readable.
- The progress callback canNOT abort the buffering loop (opus/codex), so a callback-based cap is
  useless; the pre-flight on advertised size is the only true cap that keeps the verified path.
- Residual (noted in-code): a feed omitting `size` skips the cap — but that implies feed compromise,
  which the signature check (not the cap) is the control for.
LESSONS_FILE=implementations-plan/security-hardening-2026-06-09/lessons/phase-4.md
