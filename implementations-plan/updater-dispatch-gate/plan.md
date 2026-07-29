# Plan — updater dispatch gate + L8 (piece 3)

`/blueprint light`. CI-only: `.github/workflows/_e2e-updater-windows.yml`,
`.github/workflows/release-accelerator.yml` (inputs only), `packages/accelerator/scripts/
updater-smoke-windows.ps1`, plus a **build-time-injected** (never committed) sentinel in the
synthetic N−1's `hooks.nsi`. Zero product code. Settled context: piece-1 plan §6 L8 + the refuted
`/UPDATE`-skips-uninstaller claim (`autostart-self-heal/audit-codex-final.md`); piece 2 shipped the
marker this gate exists to prove.

## Facts (verified, file:line)

1. The workflow is `workflow_call`-only; `n-artifact` defaults to `accelerator-windows-x86_64`,
   **this run's `build` output** (`_e2e-updater-windows.yml:28-32`) — why dispatch was removed:
   standalone, there is nothing to install.
2. The synthetic N−1 keeps the COMMITTED PROD pubkey (`:102-104`), so the smoke's manifest must be
   signed with the prod key (`:38-44`, `:121-124`). The header's "no Windows STABLE yet" (`:9-11`)
   is false — `accelerator-v1.0.0..v1.0.7` ship setup.exe; `:14` says to switch when one exists.
3. `timeout-minutes: 40` is sized for ONE in-job build (`:50`); the dispatch path builds TWO.
4. Release callers: `release-accelerator.yml:582` (positive) and `:599` (negative), passing
   `n-version` + prod secrets.
5. The smoke pre-seeds config with the RETIRED `safari_support` key (`updater-smoke-windows.ps1:196`);
   harmless (config ignores unknown fields, `core/src/config.rs:61-64`) but stale.
6. The smoke already proves armed→disarm→re-arm via a 200ms poller (`:198-310`) and has
   positive/negative modes; `$InstallRoot = %LOCALAPPDATA%\Aztec Accelerator` (`:45`);
   `Cleanup` handles CA/hosts/task/Run-key (`:59-78`).
7. Piece 2 means an N−1 built FROM THE CURRENT REF writes the real
   `~/.aztec-accelerator/update-in-progress.json` before `install()` and the real N reconciles it
   at startup — the dispatch path exercises the production marker lifecycle end-to-end. The REAL
   v1.0.7 N−1 (call path) predates the marker: writes none; N reconciles Missing. Both fine.
8. `hooks.nsi` uses LogicLib forms in the installer context already (`${If} ${Errors}` in
   POSTINSTALL) — the injected sentinel can use them.

## Design

**Two jobs, hard event split (piece-2 ledger A3: `workflow_call.secrets` is not an isolation
boundary — separation is by JOB, and the dispatch job requests no secrets):**

- **`updater-smoke-windows`** (existing; `if: github.event_name == 'workflow_call'` is not a real
  event — gate with `if: github.event_name != 'workflow_dispatch'`). Changes:
  - **Real N−1**: `gh release download accelerator-v1.0.7` (the `-setup.exe` asset) replaces the
    synthetic build. The throwaway-key + version-patch steps go away on this path.
  - **Preflight, fail-early** (piece-1 plan §6 L8): (a) asset exists; (b) N strictly > 1.0.7
    (F-004's floor in the real N−1 rejects anything ≤ its own version — assert instead of
    discovering on release day); (c) committed updater pubkey at tag `accelerator-v1.0.7` ==
    HEAD's (via `gh api .../contents/...?ref=` + jq — the fixture is worthless if the old build
    can't verify today's signature); (d) endpoints at that tag include `aztec-accelerator.dev`
    (the host the feed impersonates).
  - **N−1 launch proof**: new optional `-N1Version` param — poll `/health == N1Version` after
    install/launch BEFORE watching for N (catches a wrong fixture; today nothing asserts N−1 ran).
- **`updater-smoke-dispatch`** (new; `if: github.event_name == 'workflow_dispatch'`;
  `permissions: contents: read`; `timeout-minutes: 70`; references NO `secrets.*`):
  - Generate ONE ephemeral key; **patch its pubkey into `tauri.conf.json` for BOTH builds**; build
    synthetic N−1 (0.0.1, current ref, sentinel injected) and N (input `n-version`, default
    `9.9.9`, current ref); sign N's artifacts + the manifest with the same ephemeral key
    (the piece-2 audit correction: swapping only the private key fails verification).
  - Inputs: `mode: choice [barrier, positive, negative]` default `barrier`.

**The L8 barrier.** Injected by the dispatch job into the SYNTHETIC N−1's
`NSIS_HOOK_POSTUNINSTALL` body (sed after the macro line, before the build; `git checkout` after —
test lever never in shipped code). Mechanics: if `$PROFILE\.aztec-accelerator\smoke-barrier-request`
exists → record whether `$INSTDIR\aztec-accelerator.exe` exists to `smoke-p-status` ("present"/
"absent") → write `smoke-barrier-ready` → LogicLib loop until `smoke-barrier-release` (500ms ×
240 = 120s timeout, then proceed — a hung barrier must not wedge the runner past the job timeout).

**Smoke `mode=barrier`** (ps1): seed `smoke-barrier-request`; run the positive flow up to launch;
wait for `smoke-barrier-ready`; then, INSIDE the held-open window:
1. **ASSERT `smoke-p-status` == "absent"** — the goal's assert-don't-assume: this is the measured
   proof that the old-uninstaller run leaves P non-resolving mid-update (and if silent installs
   ever stop running the old uninstaller, this fails loudly instead of testing nothing).
2. Assert the real marker `update-in-progress.json` is live.
3. Launch Q — a copy of N−1's installed exe taken to a SEPARATE dir before the update — and
   assert, after a bounded wait: Run value byte-unchanged (no heal), `schtasks /Query` absent
   (no rearm), marker still present (no foreign removal), feed log shows no second download
   (no new update — D22). Kill Q.
4. Write `smoke-barrier-release`; then the standard positive tail: `/health == N`, marker AND
   token AND handoff all absent (the reconcile ran), task re-armed.
Also, in ALL positive-family modes: end-state assert `update-in-progress.json` absent (real
v1.0.7 N−1 never writes one; a current-ref N−1 must have had it reconciled away).

**Hygiene riding along:** config seed `safari_support` → `https_enabled`; header comment rewritten
(the stale "no Windows STABLE" claim); `n1-version` doc updated.

## Explicitly NOT in scope
Making the dispatch job a PR gate (two Windows Tauri builds — release-checklist item, per the
piece-1 plan §10); any change to `hooks.nsi` as committed; the rename (next piece).

## Gates
1. `bun run lint:actions` + shellcheck-clean ps1 edits (pwsh — actionlint only).
2. Push branch → `gh workflow run` the dispatch path on the branch → iterate to green
   (**no local gate exists for this phase** — budgeted for wiring iterations; lessons logged).
3. The CALL path cannot be dispatch-tested by design; it is validated by review + the barrier
   scenario sharing the same ps1 code paths, and proves out on the next release. Stated, not
   hidden.
4. PR → CI green (the accelerator.yml suite is unaffected; actionlint gates the workflow) → ONE
   codex audit → fix → ONE resumed verification → merge.

## Asks
None. (Light floor: 8 verified Facts above; no silent asks.)
