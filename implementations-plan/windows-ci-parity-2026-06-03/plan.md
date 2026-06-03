# Windows CI Parity — post-rc follow-ups (#95, #96, P5)

**Tier:** `/plan mid` (codex + opus dual audit — both completed, consolidated below).
**Status:** revised post-audit → final fresh-codex pass pending → approval gate.

## North star
Windows must reach the **same assurance** and **behave the same** as Linux/Mac in CI and the
release pipeline (owner's explicit framing). Where Windows carries a risk the others don't (the
repeating-trigger crash-recovery install-race), parity means *verifying the guard* — and where the
Windows test is *weaker* than mac/Linux (it validates a synthetic artifact, not the prod-signed
one), parity means *closing that gap*.

## Context
P0–P4 + P6 DONE on `main`. `1.0.4-rc.1` Windows rc dry-run GREEN (built the `-setup.exe`, proved
click-free minisign-verified N-1→N auto-update both legs, mac/linux gates green; tagged as a
**prerelease**). 3 follow-ups remain. **Sequence: #95 → #96 → P5.**

---

## Audit consolidation (codex = REWORK, opus = ship-with-changes)
Both audits verified against the files. Two of my original premises were factually wrong; one P5
gap was deeper than drafted. Adopted/rejected ledger:

**ADOPTED:**
- **#95 targets the wrong lock (codex+opus).** `src-tauri/Cargo.lock` is already synced
  (`1.0.4-rc.1`); the stale one is **`server/Cargo.lock` = 1.0.2-rc.1** (vs `server/Cargo.toml`
  1.0.4-rc.1). Retargeted below.
- **#96 arming was a no-op (codex+opus).** No `autostart` field in `Config`; arming is the
  tauri-plugin-autostart Registry Run key. Retargeted to the Run key / `set_autostart`.
- **P5 flips a *synthetic* test, not a parity one (codex).** The Windows smoke builds N-1 AND N
  in-job with one ephemeral key — it never exercises the **prod-signed** Windows artifact that
  mac/Linux smokes validate. This reframes P5 (see the fork).
- **Timeout-tighten is necessary-but-insufficient + promotes a privileged job (codex+opus).** Add
  a documented revert runbook (like the linux flip) + real headroom; weigh the privileged-job risk.
- **Level-2 SYSTEM spike safety (codex+opus):** GH-hosted-only, verified cleanup, task name
  provably disjoint from the `"Aztec Accelerator Crash Recovery"` constant, NOT also autostart-armed.
- **#95 via `sed`, not cargo (opus):** `bump-source` has no Rust toolchain.

**REJECTED / DEFERRED (with reason):**
- **Full structural-convergence to a real *N-1* (opus).** Rejected for P5 scope: the N-1 resolver
  excludes prereleases (_e2e-updater.yml:68) and there's no Windows *stable* yet, so a real-N-1
  smoke would wedge the first stable Windows release. Deferred to post-first-stable-release. (Note:
  using the real prod-signed *N* — below — does NOT have this problem; only real *N-1* does.)

**FINAL FRESH-CODEX PASS (round 2 — verdict: B; all adopted):**
- `server/Cargo.lock` has **two** stale stanzas (accelerator-server + the aztec-accelerator path-dep),
  not one — #95 patches both.
- #96 arming via a raw `Run\aztec-accelerator` value is a no-op (plugin uses `productName`
  "Aztec Accelerator" + `is_enabled()` also reads `StartupApproved\Run`) — drive `set_autostart(true)`
  in-harness instead.
- Option B must NOT keep a synthetic fallback once blocking (a missing real artifact must fail).
- #96 needs explicit disarm + verified task-deletion in cleanup.
- Roll Option B out STAGED (rework → advisory rc → then blocking). B's trust-chain confirmed sound.

---

## Phase 1 — #95: server-lock parity + bump-source fix
**Problem (verified Fact):** `packages/accelerator/server/Cargo.lock` pins `accelerator-server` at
`1.0.2-rc.1` while `server/Cargo.toml` is `1.0.4-rc.1`. The release version-patch step seds both
`Cargo.toml`s (release-accelerator.yml:280-ish) but neither lock; `bump-source` (≈841) bumps the
manifests only. `src-tauri/Cargo.lock` happens to be synced (incidental local build). Cosmetic —
the server build is `cargo build --release` with **no `--locked`** (release-accelerator.yml:287
comment), so the stale lock doesn't break anything; this is correctness-of-record.

**Steps (corrected by final codex — TWO stale stanzas):**
1. `server/Cargo.lock` has TWO stale local stanzas (server depends on src-tauri as a path-dep):
   `accelerator-server` (line 7) AND `aztec-accelerator` (line 259), both `1.0.2-rc.1`. `sed` BOTH
   version lines → `1.0.4-rc.1`; commit. (`src-tauri/Cargo.lock` is already synced.)
2. Patch `bump-source` to update the version line of EVERY local-package stanza in BOTH locks after
   it seds the `Cargo.toml`s — each `version = "<old>"` sitting under a `name = "<local-crate>"`
   (accelerator-server, aztec-accelerator). A targeted `sed`/awk keyed on the local crate names (no
   Rust toolchain, no transitive-dep churn). Same `chore/bump-…` PR.
3. Verify: a dry bump leaves all local-package version lines in both locks aligned with the toml.

---

## Phase 2 — #96: crash-recovery-armed updater-smoke (Level 1 + spike Level 2)
**Goal:** the Windows updater-smoke exercises an update **while crash-recovery is armed**, so the
disarm-before-install guard (updater.rs:64-119, hardened across 5 codex rounds) is under test —
the one update-interaction that is Windows-specific.

**Why parity (verified):** linux/mac recovery keys on exit code (crash_recovery.rs:188-190), so a
clean-exit update can't trigger a relaunch — no install-race, nothing to test. Windows' repeating
`TimeTrigger`+`IgnoreNew` relaunches anything not running, so the update must disarm before NSIS.

**Level 1 (committed; arming + cleanup corrected by final codex):**
1. Arm crash-recovery by driving the app's own `set_autostart(true)` command (commands.rs:31)
   in-harness — NOT raw registry poking: the plugin keys autostart on `productName`
   "Aztec Accelerator" (not the crate name), and `is_enabled()` also reads `StartupApproved\Run`, so
   a hand-written `Run\aztec-accelerator` value is a no-op. Letting the app register it guarantees
   `is_enabled()` is true so N-1's startup `enable_crash_recovery()` (main.rs:260-265) creates the task.
2. Run the positive update. Assert: (a) update SUCCEEDS (/health==N); (b) the guard ran — disarm
   during install + re-arm after (assert via `schtasks /Query` after, and/or the app-log
   "disarm/re-arm" lines; the in-install-window absence may be unobservable — assert what's reliable).
3. **Cleanup (final codex):** the smoke's `finally` must also `set_autostart(false)` + verify-delete
   the crash-recovery task (`schtasks /Query` returns absent). The current cleanup
   (updater-smoke-windows.ps1:57) does neither — an armed task/autostart must not leak even on an
   ephemeral runner.

**Level 2 (spike, may be documented-gap):**
3. Prove a tick mid-install would corrupt WITHOUT the guard, via a **SYSTEM-principal** variant
   (the crash-recovery integration test proved SYSTEM tasks fire — but on `windows-2025`; the smoke
   runs `windows-latest`, so re-confirm on the smoke runner). HARD SAFETY REQUIREMENTS: task name
   provably disjoint from `"Aztec Accelerator Crash Recovery"`; **GH-hosted runners only**; cleanup
   verified (query-after-delete), not just `finally`; the spike must NOT also arm autostart. If it
   can't be made reliable → document the gap (Level 1 + the codex-hardened guard + the crash-recovery
   integration test already give strong assurance).

---

## Phase 3 — P5: real-artifact parity + flip-to-blocking + bound-the-hang
**The reframed core (codex):** today's Windows smoke is **fully synthetic** — it builds N-1 and N
in-job with an **ephemeral** key (_e2e-updater-windows.yml:5,50,63-89). mac/Linux smokes validate
the **real prod-signed** artifact + prod pubkey. So flipping the synthetic smoke to blocking gates
releases on something that never touches the shipped artifact or the prod-signing path. To make
P5's blocking gate *mean* what mac/Linux's means, the smoke must test the **real prod-signed N**.

### THE KEY DECISION (Ask — approval gate)
- **Option A — lean:** flip the synthetic smoke to blocking now (a *mechanism* gate) + revert
  runbook + timeout headroom; rework-to-real-artifact as a tracked fast-follow. Faster; but
  P5-blocking gates a synthetic test (codex's exact objection) — weaker than mac/Linux.
- **Option B — true parity (RECOMMENDED by all three audits, matches the north-star):** first rework
  the Windows smoke to use the **real prod-signed N** = the `build` job's Windows `-setup.nsis.zip`/
  `.sig` (signed with the real `TAURI_SIGNING_PRIVATE_KEY`), with a **synthetic N-1 embedding the
  real prod pubkey** (public, from tauri.conf.json — bootstrap-SAFE, no prerelease-exclusion problem).
  That proves the *shipped* Windows artifact installs + verifies against the *prod* pubkey, like
  mac/Linux. Final codex confirmed the trust-chain claim is sound (build emits prod-signed artifacts
  at release-accelerator.yml:154; updater verifies the embedded pubkey at updater.rs:61).
  - **Roll it out STAGED (final codex's refinement):** rework → run it **advisory** on the next rc
    dry-run to prove the real-artifact path green → THEN flip to blocking. Don't make a freshly
    reworked gate blocking on its first run.

(Full real-*N-1* convergence stays deferred either way — it breaks the first-stable bootstrap.)

### Steps (after the Option decision)
1. (Option B) Rework `_e2e-updater-windows.yml`: consume the `build` job's Windows artifact as N
   (add the dependency); synthesize N-1 embedding the real prod pubkey. **No synthetic fallback once
   blocking (final codex):** a missing/empty build artifact must FAIL the gate, not silently fall
   back to an ephemeral build — that would weaken the gate exactly when the shipped artifact is broken.
2. Add `update-smoke-windows` + `-negative` to `tag.needs` + `release.needs` (matching
   `update-smoke`/`update-smoke-linux`).
3. **Bound-the-hang:** tighten `_e2e-updater-windows.yml` timeout with REAL headroom over healthy
   runtime (confirm the rc's actual smoke duration first; if Option B drops the in-job N build,
   runtime falls and the timeout can approach the linux/mac 30 naturally).
4. **Revert runbook (in the PR):** document drop-from-needs + restore-advisory, mirroring the linux
   flip (_e2e-updater-linux.yml:115-120). Treat the first post-flip real release as the true proof.
5. **Privileged-job note:** the smoke mutates `LocalMachine\Root`/hosts/Defender/Task Scheduler. As
   a release-critical gate it must clean up deterministically (already does — Remove-MpPreference,
   CA + hosts in `finally`); keep it GH-hosted-only; re-affirm no secret/privilege escalation path.
6. Owner-gated rc dry-run to confirm the now-blocking legs pass and don't wedge the pipeline.

---

## Security & Adversarial Considerations
- **Update trust chain:** minisign sig verified against the embedded pubkey. Option B *strengthens*
  this (it now tests the **prod** pubkey path, not just an ephemeral one). The negative leg
  (tamper→reject, fail-closed) stays load-bearing (updater-smoke-windows.ps1:93,169-174).
- **Ephemeral vs prod key:** ephemeral smoke key is generated in-job, isolated (separate job from
  `build`'s `TAURI_SIGNING_PRIVATE_KEY`). Option B must consume the prod-signed *artifact*, never
  the prod *private key* (the artifact + its `.sig` are public outputs of `build`). No key crossing.
- **Least privilege / privileged-job-in-release-boundary (codex):** making a job that mutates
  `LocalMachine\Root`, hosts, Defender, Task Scheduler release-CRITICAL raises the stakes on its
  cleanup + isolation. Mitigations: GH-hosted ephemeral runners only (VM torn down), verified
  cleanup in `finally`, run-unique CA/task names, Defender exclusion scoped + removed. The Level-2
  SYSTEM variant is the sharpest edge — GH-hosted-only + disjoint task name + verified delete.
- **Blocking-flip DoS-on-releases (codex+opus):** a flaky-but-fast red self-DoSes releases via the
  single-slot `cancel-in-progress:false` concurrency. Timeout bounds *hangs*; the **revert runbook**
  bounds *flakes*. Negative leg must fail closed, never open.
- **Supply chain:** no new deps. #95 keeps both Cargo.locks honest (defense-in-depth; no `--locked`).

## Assumptions
### Facts (verified against files)
- `server/Cargo.lock` `accelerator-server` = 1.0.2-rc.1; `server/Cargo.toml` = 1.0.4-rc.1; `src-tauri`
  lock + toml both 1.0.4-rc.1 (synced). Server build = `cargo build --release`, no `--locked`
  (release-accelerator.yml:287 comment).
- `Config` has no `autostart` field (config.rs:45-54); arming = Run key via `is_enabled()`
  (main.rs:260-265) / `set_autostart` (commands.rs:31-41).
- Windows smoke builds N-1 AND N in-job, ephemeral key (_e2e-updater-windows.yml:5,50,63-89);
  advisory `needs:[validate]`, absent from tag/release needs. mac/Linux smokes use real artifacts +
  real N-1, ARE in tag/release needs, timeout 30 (_e2e-updater.yml:46) vs Windows 60 (:35).
- N-1 resolver excludes prereleases (_e2e-updater.yml:68); `1.0.4-rc.1` is a prerelease; no Windows
  stable exists → real-N-1 convergence breaks the first stable release.
- The updater guard (download-first, refuse-install-if-disarm-unverified, re-arm-everywhere) is
  well-defended (updater.rs:64-119) — both audits' "looks fine".
### Inferences (attack these)
- Option B's synthetic-N-1-with-prod-pubkey verifies the real prod-signed N — UNVERIFIED until built
  (the prod pubkey is public in tauri.conf.json; the build artifact is prod-signed → should verify).
- `bump-source`'s `sed` on the lock is safe (single `version=` line per crate) — confirm the lock
  format has exactly one such line under each `name=`.
- Run-key pre-creation makes `is_enabled()` return true on the smoke runner — confirm the plugin
  reads HKCU at startup, not a cached value.
### Asks (surface)
- **P5 Option A (lean, fast-follow) vs Option B (real-artifact parity now).** ← the main fork.
- `accelerator-v1.0.4-rc.1` prerelease: keep (becomes a future N-1 baseline) or delete?
- Re-enable commit signing going forward (AFK commits unsigned, moot via squash).

## Seeds
### /goal
```
/goal All 3 phases (#95, #96, P5) ✓ in the plan checklist, each a merged PR with green CI; per phase the agent printed `LESSONS_FILE=implementations-plan/windows-ci-parity-2026-06-03/lessons/phase-N.md`; #96 Level-1 assertions green AND (P5) the chosen Option's Windows update-smoke is in tag/release needs and proven by a GREEN owner-dispatched rc dry-run that isn't wedged; `/code-review max --fix` applied + committed; codex post-impl audit clean (or high/critical addressed); `bun run test` + `bun run lint:actions` exit 0 in transcript. No STABLE release — rc dry-runs only, owner-gated.
```
### /loop
```
/loop Each turn, priority order: (1) read implementations-plan/windows-ci-parity-2026-06-03/plan.md + lessons/ as source of truth; `git status`; open PR? `gh pr view --json statusCheckRollup` (no --watch). (2) CI on HEAD? `gh run watch <id>` ≤10min. (3) Failed? triage+fix, `/codex xhigh` if non-trivial, commit (small, conventional)+push; stop+reassess after 5 fails on one step. (4) Phase green? mark ✓ in plan.md, file lessons/phase-N.md, print LESSONS_FILE=..., advance (sequence #95 → #96 → P5). (5) Nothing in flight? next pending step (edit → `bun run lint:actions`/`bun run test` → commit → push). (6) All ✓? `/code-review max --fix` → commit → codex post-impl audit (`/codex xhigh`, adversarial+security) → address high/critical → stop. Repo artifacts authoritative; codex on any scope/risk fork; NEVER merge to main or cut a stable release autonomously; rc dry-runs owner-gated — surface+stop when a release dispatch is next.
```
