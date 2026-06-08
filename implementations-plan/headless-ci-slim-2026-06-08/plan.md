# Headless CI Slim — core-extraction Phase 3b

**Tier:** `/blueprint mid`. Follow-up to `core-extraction-2026-06-07`.
**Dual audit done: codex + opus BOTH → Approach A, "holds-with-changes" (every defect folded below).**

## Summary
Now that the headless `accelerator-server` builds Tauri-free (−56% deps), slim the **bb-less** headless CI legs
(PR `Smoke` + `Release Smoke`) so they stop installing desktop GUI libraries (WebKit/GTK) and stop running the
full `copy-bb.ts` desktop prebuild. Plus the two codex post-impl findings: keep `/health.aztec_version` truthful on
the slimmed `Smoke` job (via a version-only bb-version resolution), extend the `AZTEC_BB_VERSION` hook to the e2e
leg (a `/prove` fast-path fix), and add a `/health.version` shape-guard. **e2e setup untouched.** Validated by
PR-gate CI (no rc). No `/harden`.

## Verified facts (recon + audit)
- `Smoke`: builds + runs the server + asserts `/health.status==ok` + `.aztec_version != "unknown"`. No prove →
  needs the bb **version** for `/health`, NOT the bb binary.
- `Release Smoke`: builds the RELEASE server + `tar` + `sha256`. No `/health`, no prove → needs NEITHER bb nor the
  version. **Archive-SHAPE parity only** — NOT true release parity (the real `release-accelerator.yml` patches
  versions + asserts `--version`; codex). Don't oversell what it proves.
- Both use the shared `setup-accelerator` composite (host-assert + Bun + cache + `--frozen-lockfile` + rust +
  unconditional Linux WebKit/GTK + the `copy-bb.ts` prebuild).
- `copy-bb.ts` resolves the version from `@aztec/bb.js/package.json .version` (144–150) + writes it to
  `packages/accelerator/src-tauri/AZTEC_VERSION` (181). The pinned SHA-256 gate is **Windows-ONLY** (56–159);
  on Linux/macOS the script just copies bb from installed `@aztec/bb.js` → cross-platform the trust anchor is
  `bun install --frozen-lockfile`.
- `_e2e.yml` **inlines its own setup** (apt + prebuild, NOT the composite) and launches the server in a SEPARATE
  step from setup (`:80/:85`, repo-root-relative paths).
- **Out of scope (per "no rc"):** `release-accelerator.yml`'s `build-headless` shares the waste but touches the
  release pipeline → needs an rc → deferred.

## Decision ledger
- **Approach A (boolean inputs on the shared composite) — CHOSEN; both auditors.** B (a separate
  `setup-accelerator-headless` composite) forks ALL the shared setup (host-assert, Bun, bun-cache,
  `--frozen-lockfile`, rust-toolchain + cache) to guard just **2 lines** (the apt line + the prebuild step) →
  classic drift bait, and it would split the `--frozen-lockfile` supply-chain gate + the cross-warming
  `shared-key`. A's 2 flags are `default: true` → not "sprawl." B only wins if headless setup is expected to
  diverge materially beyond this PR — it isn't. **Rejected: B.**
- **Required A fix (codex):** gate the **host/target assertion** (`action.yml:42`) on `run-prebuild` — that guard
  exists for *sidecar selection* (the prebuild picks bb by host arch), NOT for pure headless builds. Skip it when
  `run-prebuild: false`.
- **No unresolved disputes.**

## Approach A — boolean inputs on the shared composite (audit-fixed)
Two `inputs` on `setup-accelerator` (default `true` → zero change for existing callers):
- `install-tauri-system-deps` (default true): when false, install **`libssl-dev` ONLY** (headless's reqwest) —
  skip WebKit/GTK/appindicator/rsvg/patchelf.
- `run-prebuild` (default true): when false, skip the `copy-bb.ts` prebuild **AND the host/target assert**.
Extract `export function resolveAztecBbVersion()` from `copy-bb.ts` + a `prebuild:version` package script.

## Phases (small PRs; each green on the PR gate)
**Phase 1 — ✓ DONE — version-only resolver.** Extract `resolveAztecBb()` (lift 144–150; `main()` MUST call it, not
re-inline — anti-drift). Add a `prebuild:version` script. **Add a NEW unit test for the resolver** — both auditors
verified `copy-bb.test.ts` only covers the Windows tag/checksum/SHA surface, NOT this resolution path (it is NOT a
freebie). `bun run test`.
**Phase 2 — ✓ DONE — composite inputs.** Add the two boolean inputs (default true → no behavior change for any current
caller); gate the host-assert + system-deps + prebuild steps; false-branch installs `libssl-dev` only.
**Every `if:` gate MUST use explicit string comparison** (`inputs.run-prebuild != 'false'` /
`== 'true'`) — GHA inputs are STRINGS, so a bare `if: inputs.run-prebuild` is always truthy and a `false` caller
would silently still run the prebuild/full-apt path (final codex). `bun run lint:actions`.
**Phase 3 — ✓ DONE — slim the bb-less legs + hooks.**
- `Smoke`: inputs both false. **DELETE the `cat AZTEC_VERSION` line** (its file is gone post-slim → `|| echo
  unknown` would fire → the `.aztec_version != "unknown"` assert FAILS) and replace with
  `AZTEC_BB_VERSION="$(bun run --cwd "$GITHUB_WORKSPACE/packages/accelerator" prebuild:version)"` — **pin the cwd**
  (Smoke runs from `src-tauri`, the script lives in `packages/accelerator`; final codex). **Expected: `/health.bb_available` flips to `false`** (no bb
  copied) — do NOT assert `bb_available`; `status==ok` + `.aztec_version != "unknown"` still hold. Add `.version`
  as a SHAPE guard only (low semantic signal — core+server share the version today).
- `Release Smoke`: inputs both false. Nothing else (no `/health`).
- `e2e` (setup untouched): set `AZTEC_BB_VERSION` from `packages/accelerator/src-tauri/AZTEC_VERSION` **in the
  launch step itself OR via `$GITHUB_ENV`** (codex: a plain `export` in an earlier step won't persist). This is a
  **`/prove` fast-path** correctness fix (`prove.rs:59–64`), NOT a `/health` fix — frame it so a reviewer doesn't
  drop it.
`bun run lint:actions`; the PR gate exercises `Smoke` / `Release Smoke` / `e2e` on this very PR → green proves it.
**Phase 4 — ✓ DONE — docs.** Release-tarball runtime note; mark Phase 3b done. Note the apt list now lives in 3 places
(composite true-branch, false-branch, `_e2e.yml:49`) + that `_e2e.yml`'s WebKit/GTK is ALSO waste → future cleanup.

## Security & Adversarial Considerations (corrected by audit)
- **Supply chain:** the version-only resolver only READS `@aztec/bb.js/package.json .version` — no fetch/exec.
  **Correction (codex): the pinned SHA-256 gate is WINDOWS-ONLY** (`copy-bb.ts:56–159`); on Linux/macOS the script
  just copies bb from the installed `@aztec/bb.js`, so cross-platform the trust anchor is
  **`bun install --frozen-lockfile`**. The extraction touches NEITHER (resolver is read-only; `--frozen-lockfile`
  stays in the composite; the Windows checksum path is untouched).
- **Least privilege:** slimmed legs install STRICTLY LESS (drop 5 apt packages) → net-narrower CI surface. No new
  secrets/permissions.
- **Release path:** the real `release-accelerator.yml` is a SEPARATE file, out of scope; `Release Smoke` proves
  archive-shape only.

## Assumptions
**Facts (verified):** Smoke runs /health (no prove); Release-Smoke build+package only (archive-shape parity);
composite installs WebKit/GTK + prebuild on Linux; copy-bb.ts resolves the version from `@aztec/bb.js/package.json`
(144–150) + writes `packages/accelerator/src-tauri/AZTEC_VERSION` (181); headless needs `libssl-dev`; the
pinned-checksum gate is Windows-ONLY; `_e2e.yml` inlines its own setup + launches in a separate step.
**Inferences (attack):** dropping WebKit/GTK doesn't break the headless `cargo build` (HIGH — both auditors: server
depends only on accelerator-core + tokio/tracing, no GUI tree); `/health.bb_available` flips to false on slimmed
Smoke (expected, harmless — no assert depends on it); the e2e `AZTEC_VERSION` path stays stable (its prebuild still
writes it immediately before build).
**Asks (RESOLVED):** scope = bb-less legs only (e2e untouched); validation = PR-gate CI; /harden = no. None open.
**Corrected:** the earlier "Fact" that copy-bb's gate already exercises the resolver was FALSE (both auditors) →
Phase 1's resolver test is a genuine new test.

## Audit trail
- **Codex (xhigh):** `A; holds-with-changes` — host-assert gating, `/health.bb_available` flip, Windows-only
  checksum correction, e2e `$GITHUB_ENV` persistence, the false test claim, Release-Smoke parity caveat. ALL folded.
- **Opus subagent:** `A; holds-with-changes` — `cat AZTEC_VERSION` deletion ordering, explicit e2e path, the false
  test claim, `/prove`-fast-path framing, 3-copy apt drift. ALL folded.
- **Final fresh-context codex (session 019ea784):** `conditional approve` — "everything else in the folded plan
  matches the live repo." 2 conditions, BOTH folded: (1) composite `if:` gates use explicit string comparison
  (Phase 2); (2) Smoke's `prebuild:version` pinned to the `packages/accelerator` cwd (Phase 3). Explicitly
  validated: host-assert gating, `cat AZTEC_VERSION` deletion, version-only resolver, `bb_available`-not-asserted,
  e2e `/prove`-fast-path framing + `$GITHUB_ENV` persistence, `/health.version` shape-guard, Windows-only checksum,
  per-PR merge-green sequencing.

## Seeds
**Recommended — `/goal`** (the PR's own green CI is the proof):
```
/goal All phases marked ✓ in implementations-plan/headless-ci-slim-2026-06-08/plan.md; for each, the agent printed `LESSONS_FILE=implementations-plan/headless-ci-slim-2026-06-08/lessons/phase-N.md` in the transcript; `bun run test` + `bun run lint:actions` report exit 0; the PR's CI shows Smoke, Release-Smoke, and the e2e all green (the slim is self-proving on its own PR); `/code-review max --fix` applied+committed and codex post-impl audit clean (or high/critical addressed). Never merge to main without green CI.
```
**Alternative — `/loop`** (self-paced):
```
/loop Each turn: read implementations-plan/headless-ci-slim-2026-06-08/plan.md + lessons/ for phase status; `git status`; if a PR is open, `gh pr view --json statusCheckRollup` (no --watch). CI in flight? watch up to 10 min. Failed check (esp. the slimmed Smoke/Release-Smoke/e2e)? triage+fix, `/codex xhigh` if non-trivial, commit small+push (stop after 5 fails on one step). Phase green? mark ✓ in plan.md, file lessons/phase-N.md, print LESSONS_FILE=…, advance. Nothing in flight? next pending step (edit → bun run test/lint:actions → commit → push). All phases ✓? `/code-review max --fix` → commit → codex post-impl audit → address high/critical, then stop + surface. Never merge to main without green CI.
```
_Use exactly one per session — they don't compose._
