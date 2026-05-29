# WebDriver Linux flake — root cause (CONFIRMED)

**Date**: 2026-05-29
**Symptom**: `E2E WebDriver (linux) / WebDriver E2E (dev)` started failing on every PR from ~2026-05-29 morning. `settings.spec.ts` fails all 3 tests with `element ("#speed-label") still not existing after 5000ms` / `WebDriverError: null is not an object (evaluating 'el.value="2"')`. `smoke.spec.ts` and `auth-flow.spec.ts` pass.

## Decisive evidence

`#speed-label` is a **static** element in `settings.html:43` (`<span class="speed-value" id="speed-label">Full</span>`). It exists at parse time, before any JS/IPC. `waitForExist` failing ⇒ WebDriver's active window is NOT the Settings window.

`smoke.spec` checks the same `#speed-label` and PASSES; `settings.spec` (runs next, per `wdio.conf.ts:16` explicit order `smoke → settings → auth-flow`) FAILS. So the active window changes between them.

Failing-run `tauri.log` (artifact `webdriver-e2e-logs-Linux-dev`, run 26636261687):

```
12:21:27.499 updater: Update available current=1.0.2-rc.1 new=1.0.2
12:21:27.500 updater: Auto-update preference auto_update_pref=None
12:21:27.500 Showing update prompt ... version=1.0.2
12:21:30.115 ERROR tauri::manager: asset not found: settings.html
```

## Root cause

`main.rs:345-354` spawns an **ungated** background update check: sleeps 5s, calls `run_update_check`, which polls the production updater feed. It runs even under `--features webdriver` / debug builds.

1. The dev/webdriver build's source version is `1.0.2-rc.1`.
2. Until we shipped **1.0.2 stable** (latest.json pub_date `2026-05-28T20:56:44Z`), the feed's newest was `1.0.1` (< `1.0.2-rc.1`) ⇒ no update ⇒ tests green. PR #234's WebDriver ran `20:57:26Z`, ~40s after publish but before the CloudFront `max-age=300` feed propagated — so it still saw no update and passed (last green run).
3. Once 1.0.2 propagated, every dev build (`1.0.2-rc.1`) detects `→ 1.0.2 available`. With no config file in CI, `auto_update_pref=None` ⇒ it **opens the update-prompt window** ~5s after launch.
4. The new window steals WebDriver's active browsing context. `smoke` runs in the first ~3s (window still Settings → passes); `settings` runs after the 5s prompt (active window is now update-prompt → static `#speed-label` not found).

This is **self-inflicted by the 1.0.2 release** and will recur on every dev/CI build whose version is ≤ the latest published release. It is a real product bug, not merely a test bug: a dev/CI build polls the prod updater and interrupts with a modal.

## Why "same runner image, same WebKitGTK, identical UI code, opposite result"

Not an environment regression (verified: both #234 and #235 used `ubuntu24/20260525.161` + libwebkit2gtk `2.52.3-0ubuntu0.24.04.1`). The only thing that changed between the last green run and the first red run was **external state**: 1.0.2 appeared on the updater feed.

## Fix directions (for the plan)

1. **Primary (product)**: gate the background update check so it does not run under `#[cfg(feature = "webdriver")]` (and reconsider debug/dev + a `CI`/test env override). A test/dev build must never poll the prod updater or pop a modal.
2. **Defense-in-depth (test)**: make `smoke.spec` + `settings.spec` self-anchor to the Settings window in a `before` hook (capture/known handle, dismiss stray windows, `waitForExist`) instead of trusting ambient active-window state. A stray window then can't break an assumed-active-window assertion.
3. **Note the circular trap**: merging bump-source #234 (→ `1.0.3-rc.1` > `1.0.2`) would incidentally mask the flake (dev version then exceeds latest published), but that is luck, not a fix — the next stable release reintroduces it. Gate the check.
