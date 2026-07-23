# Phase 2 — C9 authorize-popup edge cases (B → C → A)

**Local status: ✓ green on everything runnable here.** Playwright + WebDriver are CI-only (see "Validation" below).

## (B) Per-scheme extension-ID grammar (D7) — `core/src/authorization.rs`
- The `chrome-extension|moz-extension|safari-web-extension` arm now validates the ID grammar before it
  becomes a `CanonicalOrigin`: `is_chrome_extension_id` (32× `a`..=`p`) / `is_extension_uuid` (lowercase
  8-4-4-4-12 hex). Rejects bidi/zero-width/non-ASCII/wrong-length homographs (opaque-host schemes skip
  `url`'s IDNA, so this is the only gate).
- Tests: `canon_extension_accepts_valid_grammar` + `canon_extension_rejects_invalid_grammar`. Two existing
  tests used the invalid 3-char `chrome-extension://abc` (now rejected) — updated to a valid 32-char ID
  (`authorization.rs` `canon_is_idempotent`, `config.rs` `de_origins_keeps_canonical`).

## (C) Server-authoritative origin via `get_pending_auth` (D8/D15) — core + src-tauri + frontend
- `AuthorizationManager::peek(request_id) -> Option<(CanonicalOrigin, bool)>` (origin + active), non-consuming.
- `get_pending_auth(window, auth, request_id) -> Option<PendingAuthDto{origin, active}>` — guarded by the
  SAME exact-label check `respond_auth` uses (a popup peeks only ITS OWN request). Registered in `main.rs`,
  in `build.rs` `commands`, and `capabilities/authorize.json` (`allow-get-pending-auth`).
- `authorize.js` now renders a `…` placeholder, fetches origin from `get_pending_auth`, feeds the SERVER
  origin to `get_verified_info`, and applies A2 UX: `null` → close (stale/resolved); IPC error → keep Allow
  disabled + retry hint; polls every 1 s so a queued popup enables when promoted.

## (A) Click-guard + SERVER-enforced single-active-popup arbiter (D18/D19/D14)
- **Arbiter state** in `PendingState` (`active: Option<String>` + `queue: VecDeque`). `insert` returns
  is_active (slot free) vs enqueued; `remove` promotes the queue head when the active one leaves and returns
  the promoted id.
- **Server-side enforcement (D19)**: `resolve_active` (used by `respond_auth`) resolves ONLY the active
  request → `ResolveOutcome::NotActive` otherwise. `resolve` (timeout/close) resolves any + promotes. Both
  atomic under the manager lock. 4 arbiter unit tests (first-active/second-queued, promote-on-resolve,
  queued-resolve-no-promote, user-resolve-rejects-non-active).
- **Timer-on-activation (D18) — the starvation fix**: the real 60 s auto-deny is armed only when a popup is
  ACTIVE (`windows.rs` at show-if-active + `commands::arm_active_popup` on promotion), NOT at enqueue. The
  `/prove` wait (`server/auth.rs`) is now `AUTH_QUEUE_BACKSTOP` (`MAX_PENDING_ORIGINS × 60 s`), a pure
  upper bound — so a queued request is never denied 60 s-from-enqueue.
- **Close listener (D14)**: `attach_close_deny_listener` resolves-Deny + promotes on `WindowEvent::Destroyed`
  (user dismissed). Idempotent with the timer/respond_auth (they resolve-then-close; the resulting Destroyed
  is a no-op).
- **Click-steal guard (A)**: `bridge.js` `guard: true` buttons ignore activation for 700 ms after the window
  last gained focus (reset on EVERY native focus, covering promotion; gated at click ENTRY so keyboard is
  covered too). Overridable via `window.__CLICK_GUARD_MS__` for tests only (production never sets it).

### Cross-crate note (build gotcha)
`commands` is in the **lib** (`aztec_accelerator::commands`), `windows` is **bin-only** (`mod windows` in
`main.rs`). A lib fn can't call a bin fn, so the three arbiter window-helpers (`arm_active_popup`,
`spawn_active_deny_timer`, `attach_close_deny_listener`) live in `commands` (lib) and `windows` (bin) calls
them. `open_or_focus_window` now returns `Option<WebviewWindow>` so the auth caller can attach the close
listener.

## Validation
- **Local green**: `core` 178 tests (grammar + arbiter + timeout); `src-tauri` 37 tests; static
  `tauri-trust-boundary.test.ts` 12/12 (command set bumped 12→13); `cargo fmt --check`, `clippy -D`, `biome`
  all clean.
- **CI-only (could not run locally)**: **Playwright** — this box is `ubuntu26.04-x64`, which Playwright's
  browser builds don't support (`Playwright does not support chromium on ubuntu26.04-x64`); the rewritten
  `e2e/authorize.spec.ts` (server-origin, active/queued, A2) validates in the `desktop-ui` CI job.
  **WebDriver** — GUI + real app, CI-only; `auth-flow.spec.ts` updated to wait for the server origin +
  elapse the click-guard.
- **Follow-up for CI**: a two-distinct-origin WebDriver case (assert the 2nd popup is not actionable while
  the 1st is pending, and a click within the guard is ignored) is NOT yet added — writing it blind (no local
  WebDriver) is error-prone; add + debug it in the CI loop. The arbiter's server-side guarantee is already
  covered by the 4 Rust unit tests; this would add the observable end-to-end assertion.
