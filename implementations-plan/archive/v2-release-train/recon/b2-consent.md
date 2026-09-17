# Recon: B2 — consent popups + click-steal guard

Agent: sonnet Explore, 2026-08-16, tree @ 0c351bc.

## Popup inventory

Frontend: `src-tauri/frontend-src/*.js` → bundled by `scripts/build-frontend.ts` into gitignored
`frontend/assets/` (`build.rs` fails on stale bundle; Playwright serves `frontend/` directly —
**staleness trap**: rebuild before testing). Window creation centralized in `windows.rs`; IPC in
`commands.rs`; arbiter in `core/src/authorization.rs`.

| Popup | JS | show fn (windows.rs) | IPC (commands.rs) | guard today | focus_on_create |
|---|---|---|---|---|---|
| Authorize | authorize.js (109 ln) | :172-239 | respond_auth :147-192, get_pending_auth :203-218, get_verified_info :134-144 | YES (:107-108, only site) | false (:207) + post-build arm_active_popup :226-233 |
| Onboarding | onboarding.js | :122-146 | get_onboarding_state :535-543, complete_onboarding :566-616 | no (:86) | true (:143) |
| Renewal | renewal.js (17 ln) | :151-166 (macos/win) | renew_cert :625-667, record_renewal_prompt :671-685 | no (:5-16) | true (:163) |
| Update | update-prompt.js (21 ln) | :246-265 (not webdriver) | respond_update_prompt :711-762 | no (:8-20) | true (:262) |
| Settings (out of stated scope) | settings.js | :102-118 | many | **bypasses wireButton entirely** (raw onclick :94-101, :241-243) | true (:115) |

## Guard engine (bridge.js)

- :15-42 engine: `DEFAULT_GUARD_MS=700` (:19); `guardMs()` reads `window.__CLICK_GUARD_MS__`
  dynamically (:20-26); `inputArmedAt` set at module-eval (:30); re-arm on native focus/pageshow
  (:34-37) via PRIVATE `rearmInputGuard` (:31-33, not exported); `isClickGuardActive()` (:40-42).
- :104-126 `wireButton()` — gate :108 `if (opts.guard && isClickGuardActive()) return;` — OPT-IN.
- e2e/tauri-mock.js:14 sets `__CLICK_GUARD_MS__ = 0` globally for all Playwright specs.

## "Became actionable" per popup

- Authorize: queued→active via arbiter; JS polls `get_pending_auth` every 1s (authorize.js:84-87),
  `setControlsEnabled(info.active)` (:68). Guard rearm relies on the NATIVE focus event from Rust's
  `set_focus()` — two independent paths; nothing rearms on the poll's active-flip. THE F-10 GAP.
- Onboarding/Renewal/Update: synchronous, focus_on_create:true — module-eval time ≈ actionable;
  simply passing `guard:true` suffices (no new plumbing).

## Update display↔decision UNBOUND — concretely reachable

- `respond_update_prompt` (commands.rs:711-762) takes NO version arg; `pending.lock().take()`
  (:732) on singleton `PendingUpdate` (commands.rs:34; no per-request id).
- main.rs:346-349 reruns update check every 12h; :243-245 unconditionally overwrites PendingUpdate
  BEFORE show_update_prompt_window (:254) which no-ops if already open (windows.rs:65-70 dedup;
  focus_if_open:false :261 — no re-navigate). Popup shows X; background swaps to Y; user's
  "Update Now" on X installs Y. Cheap fix (a): update-prompt.js sends displayed `version`; Rust
  compares to `VerifiedUpdate::version()` (updater.rs:129,137-139) before take(); reject on
  mismatch. Fuller (b): get_pending_update command + capability change — but singleton/no-id means
  the auth SEC-06 pattern does NOT port verbatim.

## Adapt list

1. Export a rearm fn from bridge.js; call in authorize.js `refreshPending()` on queued→active
   TRANSITION (needs was-active latch to avoid pushing arm-time forward every tick).
2. `guard: true` at onboarding.js:86, renewal.js:5+12, update-prompt.js:8+17 (5 sites, 3 files).
3. Update version binding per (a) above.
4. Deny-cooldown: new instance state beside PendingState (authorization.rs:229-240); const joins
   MAX_PENDING_ORIGINS neighborhood (:206-218). CHECK near server/auth.rs:53-55; RECORD in
   resolve/resolve_active (:352-385) — single choke point for all three deny paths (click, 60s
   timeout commands.rs:238-253, window-close :258-274). NOTE is_approved/is_auto_approved are
   static fns — cooldown is structurally closer to resolve/peek (lock self.state).
5. Queue fairness: strict FIFO (queue.pop_front() :278). NO existing policy; AND the
   AUTH_QUEUE_BACKSTOP bound (server.rs:58-62 = 60s × (MAX_PENDING_ORIGINS+1) = 660s) ASSUMES
   strict FIFO — reordering invalidates the stated bound. NOT "genuinely small" → recommend DEFER
   per brief.

## Test infrastructure

- Playwright mocked (test:e2e:ui): authorize/onboarding/update-prompt/settings specs; tauri-mock
  zeroes guard → Playwright CAN test guard via addInitScript non-zero override (e.g. 50ms): click
  immediately → assert IPC NOT in `__TAURI_MOCK__.calls`; wait past; click; assert fires. Fast,
  headless, no real 700ms.
- WebDriverIO real webview (test:e2e:webdriver, port 4445, wdio.conf.ts order :18-24): only auth
  has a spec; `waitForActivePopup()` (auth-flow.spec.ts:132-143) waits PAST the guard (pause 900)
  but never asserts a click WITHIN it is ignored. clickBy WebKitGTK workaround :26-52.
- Rust arbiter tests: authorization.rs:431-919 — closest template for cooldown tests
  (arbiter_* :574-650, co() helper, #[tokio::test]).
- No unit test of bridge.js guard logic exists anywhere (test:unit covers scripts/ only).

## Collisions

- settings.js consequential actions (origin Remove, cert-trust Remove) have zero guard and bypass
  wireButton — same threat class, outside stated scope; surface as small fold-in or explicit defer.
- focus_on_create asymmetry: JS-only fix leaves the "does set_focus() fire a JS focus event on
  WebKitGTK/macOS/Windows" question moot for authorize only if arming moves to the poll transition.
- Frontend build staleness (above). tauri-mock zeroing (above).
- Backstop bound vs fairness (above).

## Governing docs found in-tree

brief.md B2 (:43-50) matches; inputs/codex-consolidated.md:7 (F-10), :21 (F-07 grouping);
implementations-plan/security-hardening/clusters/C9-plan.md — built the existing guard/arbiter
(D1-D19 ids in code comments); its Deferred section :35-46 pre-flagged the queue work.
