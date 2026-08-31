# Post-implementation quality loop

## Pre-codex review
- Plan said `/code-review max --fix`; owner killed max (10-finder fan-out) AND medium (still fans
  out) live, then picked `low --fix` from the offered options. Memory updated: for this account,
  plans should write `low --fix`; only `low` avoids agent fan-out in current builds.
- `low` result: ONE finding — dead conditional in `SparkOrbitController.angle()` (both arms
  identical). Correctly left unfixed as "unfinished intent". Implemented the intent: quadrant 3
  completes the lap (spark lands on top for the pop), quadrants 0-2 stay capped. Commit `71ae996`.

## Codex round 1 (session 01a03e8d-b507-7ed3-9b52-abbe3f3b5d54, xhigh)
Findings: 1 High, 3 Medium, 5 Low. ADOPTED (commit `3947e72`):
- H settings.html cert re-enable copy claimed a system prompt (false on Linux) → neutral
  "Re-enabling installs it again." (facts safe on all OSes; per-OS detail lives in onboarding).
- M sweep "workflows untouched" overclaimed → real `git diff --name-only origin/main` check over
  .github/workflows + .github/actions (skips when origin/main absent), test renamed honestly.
- M launcher readiness raced → require BOTH 4445 and /health after the loop, fail if the app
  process died during startup, track DBUS_SESSION_BUS_PID for cleanup.
- M OFL redistribution condition → full `OFL-1.1.txt` beside the fonts, LICENSES.md points at it.
- L dial monotonicity: `|| this.#lap > 0` made every post-lap same-quadrant phase reset easing
  (codex reproduced 387°→366°) → reset only on quadrant CHANGE + regression test.
- L `--check` doc overclaimed app-icon coverage → doc states tray-only byte-reproduction; app
  icons stay under the sha256 manifest test.
- L thumb-glow static fallbacks were light-only → dark-theme blocks with dark literals.
- L comment accuracy: font/CSP comment now names the default-src fallback; theme.spec header no
  longer claims WebDriver theme coverage; launcher/sweep comments de-referenced from plan/review.
REJECTED (with reasons):
- Exact-declaration pinning of every frozen constant in the sweep — brittle to formatting;
  tauri-identity.test.ts already pins the conf fields exactly; presence + no-Presto + the new
  workflow git-diff is the intended guarantee level.
- Launcher port-OWNERSHIP verification — over-engineering for a local dev script; the preflight
  already fails fast printing the owning pid.
- `--check` byte-comparing tauri-icon outputs — the CLI's byte determinism across versions is
  unverified; the manifest covers committed-file integrity.
- Removing landing CSS section-banner comments — they match the file's pre-existing convention.
Gates after fixes: root `bun run test` ✓ (81-test final leg incl. new sweep+dial tests),
`test:e2e:ui` 74 ✓, shellcheck ✓.

## Codex round 2 (resumed)
Three Lows, all adopted (commit `53d72c7`): ta-da now sweeps linearly to the top in 450ms and
holds (the exponential approach stalled ~40° short of the claimed "lands on top"); the rewind
regression test eases materially before sampling so the old bug actually fails it; the sweep's
workflow guard asserts git's exit code.

## Main merge 2026-08-31 (#479/#481/#482/#484 → branch) + codex audit
Five conflicts, all in files the rebrand rewrote: `windows.rs` (branch theme lookup + main's
dev-only loopback navigation exception, both kept), `landing/main.ts` (main's
`LandingDetectionController` adopted; the branch's innerHTML-free rendering kept by snapshotting
the CTA as cloned nodes), `landing/style.css` (the Presto rewrite had dropped `.notice`,
`.btn-outline`, `.hidden` that #484's new permission notice relies on: re-added in Presto tokens),
`playground/main.ts` (main's `AcceleratorStatusController` + the branch's `SparkOrbitController`),
`index.md`. #484's new `accelerator-status.ts` view copy and the notice markup were re-voiced into
branch vocabulary ("running", "in-browser", brand-gold tokens) with the pinned e2e assertions
updated; `lna.real.spec.ts` needs a real-Chromium LNA harness not runnable here (mocked spec 12/12).
Lesson: the sweep's `merge-base` guard held (main's `_e2e-webdriver.yml`/`app.yml` edits did not
trip it), and `--features webdriver` churn in `gen/schemas` was restored before committing.
Codex (fresh session `01a0582e-76c6-7670-9d59-4c4f50d6c33b`, xhigh): "merge needs fixes, but only
low-severity copy cleanup; nothing material in behavior, security, or identity." Two Lows adopted:
#484's residual "Accelerator status refresh failed; WASM fallback remains available" log, and the
pre-existing em dashes in the landing/playground `<title>`/og/twitter copy.

## Codex round 3 (resumed) — CONVERGED
One Low, adopted verbatim (commit `b44b122`): `git diff origin/main...` with no second ref stops
at HEAD (missing staged/unstaged edits); `git diff --merge-base origin/main` diffs merge-base →
working tree with the same upstream-change immunity. Codex verdict, quoted: "No other new
material findings remain."

## Post-QA round (local build + appearance control)

Owner ran the branch on a real Mac. Findings and the codex loop that followed.

### From the visual QA pass
- **OS dark mode DOES reach the WKWebView.** Queried the live webview of a `--features webdriver`
  build both directions (`-NSRequiresAquaSystemAppearance YES` forces light for one process without
  touching the system setting). The plan's Rust `data-theme` fallback is therefore NOT needed on
  macOS. Linux/Windows still unverified.
- **`inner_size` is not the viewport.** A window built 500x600 gives the page 568 on macOS. The
  desktop-ui specs sized pages to the WINDOW, so they proved a fit the user never gets. Switching to
  `VIEWPORT_SIZES` immediately failed twice: onboarding over by 5px, renewal by 51px with its consent
  buttons unreachable and no scroll. Both were pre-existing.
- Tray spark vanished for 3 of 24 frames (bolt tail sits 19.6 from center against an orbit of 20);
  fixed with a mask that punches a gap rather than moving the orbit.

### Loop notes worth keeping
- **`codex login status` lies.** It reported "Logged in using ChatGPT" while the token was expired
  (`401 token_expired` in the log). Check the log, not the status command.
- **Falsify every regression test.** Three of the tests added here were run against the un-fixed
  code first. One that was NOT falsifiable got rewritten: a "survives a reload" E2E would have
  passed with or without the localStorage fix, because the post-load re-assert produces the same end
  state. It is now split, with the cache assertion as the real check.
- **Playwright does not report a `<fieldset>` element as disabled**, though it does resolve disabled
  through an ancestor fieldset. A false failure here nearly sent a correct implementation back for
  "fixing"; a MutationObserver probe settled it. Assert on a child input.
- **Do not build with `--features webdriver` before committing** — `gen/schemas` picks up
  `webdriver:default` churn. Regenerate from a plain `cargo build`.
- **The WDIO runner cannot start a session on this machine** (`UND_ERR_INVALID_ARG` from its bundled
  undici, all six spec files identically, pre-existing). New E2E assertions were validated by driving
  the same steps over raw WebDriver; the spec file itself runs only in CI.
- `:has()` is Safari 15.4+ and this app's floor is macOS 10.15 WebKit. The style sheet says so in its
  own header, and it still got used once. `fieldset[disabled]` + `:disabled` was the portable answer.

### Codex rounds (session 01a03fef-d797-7260-961f-928f3117acde, xhigh)
R1: 4 should-fix (re-navigation flash only half-fixed, hydration race, discarded `eval` errors, the
32px overclaim). R2: 2 (hydration race still open via a stalled `get_system_info`; non-atomic cache
write — documented, not fixed). R3: 3 (`:has()` floor break, E2E waited on existence not hydration,
fixed 400ms sleeps). R4: 3 (recovery path re-opened the race, `!undefined` truthiness bug, schema
churn). R5: **"Converged. I found no remaining merge-blocking issues."**

Rejected with reasons: a revision marker for the cache/config write gap (self-healing transient, more
machinery than the defect); per-platform chrome constants (never measured — documented the gap
instead of guessing).
