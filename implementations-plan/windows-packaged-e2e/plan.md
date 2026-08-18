# Windows packaged-E2E legs — composed proof + full uninstall

**Tier**: `/blueprint light` (bounded surface: two CI jobs in one reusable workflow + one contract-test count).
**Branch/worktree**: `windows-packaged-e2e` off `main` @ `b98352b` (source already at `2.0.1-rc.1`).
**Recon**: [recon.md](recon.md) — read it first; every design choice below cites it.

## Phase 0 answers (derived from the goal, not re-asked)

| Question | Answer |
|---|---|
| Success criterion | Both Windows legs green on a REAL draft's installers in a full rc dispatch, alongside the existing 4 legs. |
| In scope | "Packaged E2E (windows)" composed proof; "Full uninstall (windows)" (absorbs backlog #61). |
| Out of scope | The Windows stateful upgrade + config-migration leg. Updater smokes already cover Windows update mechanics against the real 1.0.7 N-1 (positive+negative); migration code is platform-shared and E2E'd on Linux; Windows deltas (DACL, paths, legacy-exe prune) are unit-tested. |
| Quality bar | Production release gate — these legs become release-blocking the moment they merge (recon §2). |
| Validation layers | Local `bun run test` + `bun run lint:actions`; live proof via `2.0.1-rc.N` `mode=publish` dispatches (rc tags are cheap, append-only, fix-forward). |
| Escalate vs decide | Any change to production trust semantics escalates to the owner. Harness-only choices are mine. |

## The fork (Phase 2 is gated on this)

Recon §0 is the headline: on Windows the SHIPPED app launch-gates HTTPS on its own trust predicate
(`main.rs:121-129` -> `trust/windows.rs:155-177`), which queries **CurrentUser `Root`** — a store that cannot
be written without the OS consent dialog on a hosted runner (`tests/trust_windows.rs:1-9`). The goal requires
testing the shipped artifact, which rules out the previously-recommended test-only `e2e-trust` cargo feature
(a specially-featured build is not what users install). This exact wall is already recorded as an
owner-blocked decision (`v2-release-train/b4-impl.md:215-216`).

Candidate resolutions, to be decided with codex's design round + the owner:

- **(A) Extend the production predicate** to also accept the anchor in `LocalMachine\Root`. CI can seed that
  store silently today (`updater-smoke-windows.ps1:156-163`, runner is admin) and Chromium chains to it, so
  the proof stays real. Cost: a security-sensitive widening of what the app accepts as trusted.
- **(B) Presence-satisfying seed + browser SPKI pin** — IF `certutil -user -store Root` enumerates a
  raw-written store entry (the app's gate is a PRESENCE query, not a chain build), pair it with Chromium's
  `--ignore-certificate-errors-spki-list=<one key>`. Zero production change, real TLS, one pinned key.
  Hinges on two empirical facts to verify on a runner.
- **(C) Descope the composed proof to a documented manual/self-hosted check** and ship only the uninstall leg.
  The goal forbids silently choosing this — it is an explicit owner call.

**Phase 1 does not depend on this.** The uninstall leg needs no HTTPS and no chain trust: removal is
delete-only and headless-safe (`trust/windows.rs:189-209`, `hooks.nsi:388`).

## Architecture & Implementation (light tier)

**Shape**: two additional STANDALONE named jobs in `_e2e-packaged.yml` (never a `strategy: matrix` — matrix
interpolation renames the check names branch protection may pin, recon §7), each `needs: stage-installers`,
each `permissions: contents: read`, unconditional at job level with the `release_tag != ''` gate applied
per-step (existing convention). No caller edits: `packaged-e2e-on-draft` gates on the whole called workflow's
result, so the new jobs fold into `tag`/`finalize` automatically — and become release-blocking (recon §2).

**Reused as-is** (recon §1): `.github/scripts/packaged-e2e-verify-manifest.sh` and `packaged-e2e-swap-sdk.sh`
are OS-agnostic (awk/sha256sum/tar/npm all present in Git Bash) — no OS branch, no new script for them.

**Changed once, shared**: `stage-installers` release-asset mode gained `--pattern '*Windows-x86_64-setup.exe'`
(build-artifact mode's `accelerator-*x86_64` already matched `accelerator-windows-x86_64` — verified against
`release-accelerator.yml:746,880,898`).

**Windows-specific deltas** (no Linux/macOS analogue, recon §5):
1. Defender exclusion BEFORE touching the unsigned installer (`Add-MpPreference -ExclusionPath`).
2. `Start-Process ... "/S" -PassThru` + `WaitForExit(timeout)` + `Kill()` — NSIS `/S` is async; a non-silent
   prompt would otherwise hang to the job timeout.
3. Binary located under `%LOCALAPPDATA%\Aztec Accelerator\` (no PATH registration) — address it by name, the
   `b823374` lesson (a bare `find|head -1` grabs the bundled `bb` sidecar).
4. `AZTEC_ACCEL_NO_UPDATE=1` so the launched app never polls the prod feed.
5. **Disarm crash-recovery BEFORE any forced kill** — else Task Scheduler treats it as a crash and relaunches
   mid-assertion (`accelerator.yml:723-729`).
6. Cleanup mirrors the macOS hardening: `if: always()` + `timeout-minutes` + `continue-on-error` (a hung
   trust/cleanup call once burned a PASSING run to the job timeout, `def1cbe`).
7. Shell split: bash (Git Bash) for portable glue; `pwsh` ONLY for cert-store / registry / schtasks steps,
   with a trailing `exit 0` where a native tool leaves a stale non-zero code.
8. No xvfb/tray/dbus needed — WebView2 runs unattended on the Windows runner session (`accelerator.yml:635-790`).

**File-level change map**
- `.github/workflows/_e2e-packaged.yml` — +Windows download pattern (done); +`uninstall-windows` job (P1);
  +`packaged-e2e-windows` job (P2).
- `packages/accelerator/scripts/release-contract.test.ts` — write-isolation reads `4 -> 6` (only after BOTH
  jobs land; `-> 5` in the interim if P2 is deferred), + name/behaviour pins, mutation-proven.
- `implementations-plan/windows-packaged-e2e/` — this plan, `recon.md`, `lessons/`.

**Trade-off taken**: assertion depth. The linux uninstall leg deliberately asserts only "flow succeeded +
binary gone", pushing per-store detail to unit tests (`_e2e-packaged.yml:460-462`). The Windows leg asserts
much deeper (Run value, scheduled task, install dir, processes, certs-dir removal vs config retention) —
because that breadth is exactly the still-open #61 item (`v2-release-train/lessons/b5.md:100-105`) and because
Windows is the only OS with a native uninstall hook whose ownership logic has never run end-to-end.

## Security & Adversarial considerations

- **Privilege isolation must not regress**: `stage-installers` stays the ONLY `contents: write`; both new jobs
  execute downloaded installer code and therefore stay `contents: read`. The contract test's count guard is
  what enforces this — updating it is part of the change, not an afterthought (recon §3).
- **Trust predicate**: option (A) widens what the shipped app accepts as a trust anchor. Threat: anyone who
  can write `LocalMachine\Root` (local admin / admin-level malware) could make the app serve HTTPS with an
  anchor the user never approved. Counter-argument: an attacker with admin already owns the box AND browsers
  already trust that anchor, so the app refusing is a false negative. This is precisely why it escalates.
- **Uninstall ownership**: the leg must not "prove" removal by running as the only install — the foreign-owner
  path (`uninstall.rs:157-163`, `ForeignOrUncertain` -> leave everything, exit 0) is the security-relevant
  branch. Where cheap, assert the preserve-foreign case too (that is half of #61's scoped harness).
- **Unsigned installer**: Defender exclusion is scoped to the install/staging paths only — never a blanket
  real-time-protection disable.
- **No new secrets, no new permissions, no network egress** beyond the existing staged-artifact download.

## Phases

- **P0 — DONE**: recon + this plan + `stage-installers` Windows pattern.
- **P1 — DONE + PROVEN**: `uninstall-windows` leg. Green three times on a hosted runner against the REAL
  published 2.0.0 installer (as first written, after the codex round-2 hardening, and after round 3).
  Absorbs backlog #61. Contract-pinned and mutation-proven; write-isolation now 1 write / 5 reads.
- **P2 — BLOCKED (owner), infeasibility now fully measured**: `packaged-e2e-windows` composed-proof leg.
  BOTH claims behind the blocker are first-hand measurements against the shipped 2.0.0 binary on the CI
  runner image, not inherited from prior art:
  1. `certutil -user -store Root` does NOT see a LocalMachine-imported anchor (though
     `Cert:\CurrentUser\Root` does) — so an LM seed leaves the gate closed, verified end-to-end.
  2. NO headless path seeds CurrentUser\Root: `certutil -addstore` hangs, `.NET X509Store.Add()` hangs, and
     `Import-Certificate` *succeeds without landing* in the store the predicate reads (protected-root
     filtering). A green `Import-Certificate` is not evidence of a seed.
  Needs decision (A) / (B) / (C). (A) is a change to a security-sensitive PRODUCTION predicate, which is
  outside this arc's scope ("two CI legs, not a framework") and outside the agent's authority — hence
  STOP-and-surface rather than a quiet descope to a weaker proof.

  **The other P2 unknown is RETIRED (spike 3)**: Playwright/Chromium had never run on a Windows runner in
  this repo, and if a browser could not reach the app there, no trust fix would have been sufficient.
  Measured: chromium installs and launches on `windows-latest`, and both `page.request.get` and `page.goto`
  reach `http://127.0.0.1:59833/health` (200, real body). Chrome's Local Network Access gating does not bite
  because the page origin IS loopback. So after (A) or (B), the composed leg contains **no unknown
  mechanism** — it is the ordinary harness (install → seed trust → launch → swap packed SDK → existing spec).

  **Scoping (A) correctly** (from reading the call sites, `windows.rs`): `live_present` has FOUR callers, and
  two of them — `install()` :214 and `trust_new_anchor()` :283 — verify the app's OWN write to CurrentUser.
  Widening those would let a FAILED per-user add report success whenever a machine-wide copy exists. So (A)
  is "add a wider predicate used ONLY by the launch gate (`is_ca_trusted`)", not "widen `live_present`".
  Second consequence: `remove()` is `-user -delstore` only, so an LM anchor survives uninstall — the
  uninstaller must report that rather than claim complete removal.
- **P3 — DONE for P1** (reads 4 → 5); goes to 6 with P2.
- **P4 — codex loop closed for P1** (rounds 1-4); live rc validation of the leg inside a real draft happens
  on the next genuine release dispatch, since the pipeline refuses non-main refs — the
  `smoke-uninstall-windows.yml` manual smoke covers it in the meantime by running the identical script.

## Validation route (revised — the original plan was wrong)

The plan assumed live proof would come from `2.0.1-rc.N` dispatches. It cannot, on a branch:
`release-accelerator.yml` asserts a main ref before doing anything (F-005), and it is the ONLY caller of
`_e2e-packaged.yml`. So a new leg there is unrunnable until merged — at which point it is already
release-blocking, which is precisely the risk the live proof was meant to retire. Resolution: the leg body
lives in a script, and `smoke-uninstall-windows.yml` (manual, `workflow_dispatch`) runs that same script
against a published installer. That is what produced the three green proofs. The rc dispatch remains the
final in-situ check, but it is no longer the ONLY way to learn whether the leg works.

## Codex loop log

Rounds are logged in [lessons/phase-1.md](lessons/phase-1.md) — round, question, verdict, folded vs rejected
(with the rejection rationale, since "reject over-engineering" is an explicit goal constraint).
