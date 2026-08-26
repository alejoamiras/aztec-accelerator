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

## Codex round 2
- _pending — resumed session re-review_
