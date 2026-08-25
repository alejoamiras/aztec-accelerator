# Post-implementation lessons (2026-08-19)

## /code-review max --fix (commit `ae8dd4c`)
9 findings, 7 applied, 2 report-only (npm-12 cache-miss leak — benign today; no-op
addEqualityTesters semantics — pre-existing three-twin issue, out of scope). Standouts:
- **It overturned my own shim diagnosis**: the playground's bunfig-preloaded `happydom.ts`
  already patches `expect.addEqualityTesters`; my "CI never loads field.js" claim was wrong
  (mocked tests DO load it — happydom saves them). My failing repro had run from the repo root,
  where the package bunfig doesn't apply. The shim cluster was deleted; `test:live` (run from the
  package dir) is the canonical live-smoke entrypoint.
- New `scripts/bunfig-aztec-excludes.test.ts` — the excludes list is no longer hand-verified:
  lock↔excludes parity both directions + scope guard + pattern-rot guard, in `test:scripts`.
- Tarball consumer now derives the exact-host pin from the tarball's own manifest (not the
  workspace manifest I'd used — which skews when pointed at an old tarball).

## Codex post-impl fix loop (session `01a01a71…`, full table in audit-codex.md)
Round 1 conditional (4 findings, headline: `bunfig.toml` missing from sdk.yml's changes filter —
a bunfig-only edit could skip the very guard reviewing it) → fixes `d9ebd28` → round 2
conditional (2 smalls: canonical semver, EXIT trap) → fixes `96348a4` → round 3 **approve**,
"no remaining material findings". Converged in 2 fix rounds.

## Process lessons
- **Never gate through a pipe**: `bun run test | tail -3` returned tail's exit 0 and let a
  red-typecheck commit push (`0917886`). Capture exit codes explicitly (`> log; echo $?`).
- A "finished" /code-review background fork claiming to WAIT on a sub-angle with no live
  children needs a SendMessage nudge — it resumed and completed cleanly.
- commitlint header-max-length (100) rejects long conventional headers; keep the detail in the
  body (and use `git commit -F <file>` for multiline messages — the sandbox rejects long inline
  `-m` bodies).

LESSONS_FILE=implementations-plan/aztec-5.2.0-2026-08-18/lessons/post-impl.md
