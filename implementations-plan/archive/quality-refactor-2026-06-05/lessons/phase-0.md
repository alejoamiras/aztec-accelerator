# Phase 0 — characterization harness (lessons)

## PR 1 — Rust hot-path characterization (branch `refactor/phase0-rust-characterization`)
Three pins landed, all test-only, full suite 125 passed:
- **`0622670` — Q8 error wire contract.** `/prove` error responses are `{error,message}` JSON-shaped but served as **`text/plain`** (via `(StatusCode, String)`, not `axum::Json`). Verified against real behavior. Pins status + error-id + Content-Type for invalid_version / invalid_origin / origin_denied. This is the Q8 regression guard: the SDK's `ky` keys `HTTPError.data` on Content-Type, so a switch to `axum::Json` would silently change SDK behavior.
- **`427bc45` — Q3 eviction edges.** `versions_to_evict` empty / only-bundled / under-limit — the AztecVersion refactor (signature `&[String]`→`&[AztecVersion]`) must preserve these.
- **`d4d766b` — Q2 ordering + Q10 status, via fake `bb`.** Added `serial_test` (dev-dep) to make `BB_BINARY_PATH` env tests race-safe (no serial_test existed; the existing `find_bb` env tests are now `#[serial]`). A fake `bb` shell script (writes a `proof` file to `-o`, exit 0) pins the SUCCESS path: 200 + `{proof}` base64 + `x-prove-duration-ms`, and the on_status sequence `["Status: Proving...", "Status: Idle"]`. Exercises auth→body→semaphore→status→resolve→bb end-to-end.

## Key findings / decisions
- **Confirmed `/prove` ordering** (codex's critical correction): auth → body-buffer → **semaphore → resolve_version/download** → bb (server.rs:493-537). The semaphore is acquired BEFORE resolve, so the download is already race-protected — the audit's worry was that the PLAN mis-stated it; the harness now pins the real order so Q2 can't regress it.
- **Success path is already `application/json`** (`axum::Json`); only the ERROR paths are `text/plain`. Q8's text/plain constraint applies to errors only — confirmed.
- **`StatusGuard::drop` emits `"Status: Idle"`** — the Q10 enum must reproduce that exact reset.

## Deferred (principled, not skipped)
- **`download_bb` atomic-rename-cleanup** → deferred to **Q11**: `download_bb` always hits the network with no injection seam; Q11 extracts `download_tarball`/`install_version_dir`, which makes the cleanup unit-testable. The extract+cleanup logic itself is already covered (extract_bb_* tests).
- **`find_bb` search-chain order beyond env** → not pinned: the non-env steps depend on real fs locations (current_exe dir, ~/.aztec-accelerator, ~/.bb, PATH) with no clean injection; env-based order is racy. The env-precedence is covered by the (now `#[serial]`) existing tests.
- **crash-recovery disarm→install→rearm sequence** → **Windows-only** (`#[cfg(windows)]`, updater.rs:86-119); the macOS dev box + macOS rc exercise a no-op `disable()`. Per opus: the real proof gate is the **Windows `_e2e-updater-windows.yml` rc run**, not a macOS unit test.

## Next
PR 1 → CI → merge. Then PR 2 (SDK contract characterization: phase sequences + status field-combinations). Then Phase 1 refactors begin.
