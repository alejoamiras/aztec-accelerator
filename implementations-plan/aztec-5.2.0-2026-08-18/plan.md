# Plan — @aztec/* 5.0.1 → 5.2.0 bump

- **Tier**: `/blueprint light` (rubric: external coupling HIGH, everything else LOW-MOD; fourth bump
  cycle with purpose-built tooling and fail-closed CI gates makes light defensible despite the 1-HIGH
  nominally pointing at mid).
- **Success criterion** (user, 2026-08-18): bump PR merged to main with green CI, after a live
  testnet proving smoke. SDK publish, playground deploy, and accelerator release stay queued for the
  owner (consistent with the 2.0.0-GA held items).
- **Vehicle** (user): local run of the repo's own updater in this worktree — NOT the
  `aztec-stable.yml` bot path.
- **Min-age decision** (user): `@aztec/*` gets a permanent `minimumReleaseAgeExcludes` entry in
  `bunfig.toml` (5.2.0 published 2026-08-18, inside the 7-day window).
- **eli5_mode**: artifact. ELI5 source: `eli5.html` in this dir; Artifact URL recorded in Seeds
  section once published.
- **Worktree/branch**: `aztec-5.2.0-2026-08-18` / `worktree-aztec-5.2.0-2026-08-18`, base
  `b98352b` (origin/main).
- Recon: see `recon.md` (same dir). Key external facts: 5.2.0 on npm today (npm `latest`); live
  testnet already serves `5.2.0-nightly.20260815`; `@aztec-foundation/aztec-standards` has **no
  5.2.0** (latest `5.1.0-rc.1`); v5.2.0 GitHub release has the Windows bb asset.

---

## Phase 1 — Min-age exception + mechanical bump

1. Edit `bunfig.toml`: under `[install]`, add
   `minimumReleaseAgeExcludes = ["@aztec/*"]` with a comment recording the rationale (first-party
   trusted scope; the @aztec bump flow inherently targets fresh releases; decision 2026-08-18) and
   the residual risk (see Security). If Bun rejects the wildcard (support undocumented), fall back
   to the exact package list: the 14 distinct `@aztec/*` names across `packages/sdk/package.json` +
   `packages/playground/package.json`.
2. `bun run aztec:update 5.2.0` — expect the loud warn-skip for `@aztec-foundation/aztec-standards`
   (stays 5.0.1 per Ask A1 unless approval says otherwise). Verify stdout lists every `@aztec/*`
   package as updated; treat any OTHER skip as a blocker (fail-open `npm view` lesson from the
   5.0.1 cycle — re-run to distinguish 404 from network blip).
3. `bun install` (no override flag — the bunfig exclusion is the mechanism, and its success is the
   empirical wildcard test).
4. Lockfile discipline: `git diff bun.lock` — every changed resolution/integrity line must be an
   `@aztec/*` package (or its direct consequence); anything else is a stop-and-investigate.
5. `bun install --frozen-lockfile` — must pass (CI parity).
6. `bun scripts/check-windows-bb-pin.ts` — expect it to REPORT the missing 5.2.0 pin (Phase 2 fixes
   that); confirm the live-resolved `@aztec/bb.js` version string it prints (expected `5.2.0`).
7. Fix any typecheck/unit fallout from API drift (recon says SDK surface is minimal and 5.2.0
   claims no breaking TS changes; historical breaks land in playground code — fix in place, keep
   mechanical-bump vs code-fix changes in separate commits if fixes are non-trivial).

**Phase assumptions**: bunfig exclusion applies to `bun install` resolution of new pins (Fact F6
mechanism, Inference I1 for wildcard); updater's npm-view pre-check sees all @aztec packages
published at 5.2.0 (verified for aztec.js/bb.js/bb-prover — Fact F2; remainder Inference I2).

**Validation gate**
- Commands: `bun run test` && `bun run --cwd packages/playground typecheck:scripts`
- Pass: both exit 0 (biome+shell+rust lint, 3-graph typecheck, all TS+Rust unit suites green).
- Layers: lint · typecheck · unit.

## Phase 2 — Windows bb pin (manual-review provenance)

1. Confirm live bb.js version: `cat node_modules/@aztec/bb.js/package.json | grep '"version"'`
   (expected `5.2.0`; the pin key MUST be this string, not the aztec tag).
2. Download `https://github.com/AztecProtocol/aztec-packages/releases/download/v5.2.0/barretenberg-amd64-windows.tar.gz`,
   compute `sha256sum`. Sanity-check size against the 5.0.1 asset and confirm the tag/release page.
3. Add `WINDOWS_BB_CHECKSUMS["5.2.0"] = { sha256: "<hash>", provenance: "manual-review", note:
   "<release URL, fetch date, verifier>" }` in `packages/accelerator/scripts/copy-bb.ts`.
   F-008 note: the human-review half of this pin is the OWNER verifying the hash at PR review —
   the note must give them everything needed to reproduce it in one command.
4. `bun scripts/check-windows-bb-pin.ts` — now reports the pin present.
5. Optional local proof: `bun run --cwd packages/accelerator prebuild` (extracts Linux bb from the
   installed `@aztec/bb.js`, regenerates gitignored `src-tauri/AZTEC_VERSION`) — proves
   `resolveAztecBb()` resolves 5.2.0 end-to-end before CI does.

**Phase assumptions**: bb.js version string == "5.2.0" (Fact F2 for npm existence; exact-string
Inference I3, verified in step 1); Windows asset is the correct artifact for bb.js 5.2.0
(Inference I4 — historically true; CI's Windows Build Smoke is the enforcing proof).

**Validation gate**
- Commands: `bun run --cwd packages/accelerator test:unit` && `bun scripts/check-windows-bb-pin.ts` && `bun run test`
- Pass: copy-bb unit suite green; pin-check reports a reviewed pin for the live bb.js version;
  full root suite still exit 0.
- Layers: unit (+ the pin's format checks).

## Phase 3 — Commit, PR, CI green

1. Stage exactly the canonical bump set + this cycle's additions:
   `packages/sdk/package.json packages/playground/package.json bun.lock
   packages/playground/src/aztec.ts packages/accelerator/scripts/copy-bb.ts bunfig.toml
   implementations-plan/` — then verify `git status` shows nothing else touched by the updater
   (replicates `_aztec-update.yml`'s `git diff --exit-code` guard).
2. Commit `chore(aztec): bump @aztec/* 5.0.1 → 5.2.0` (separate commits for any Phase-1 code
   fixes). Pre-push local gate: `bun run test` && `bun run lint:actions`.
3. Push; `gh pr create --draft` (draft until the Phase-4 smoke passes — ruleset needs 0 approvals,
   so draft is the only pre-merge brake).
4. `gh pr checks --watch` — expect ~20–25 min, long pole Windows Build Smoke (~20 min), which is
   also the enforcing proof of the Phase-2 pin.

**Phase assumptions**: the three package workflows + actionlint all fire and aggregate into the 4
required Status checks (Fact F7); CI never hits min-age (frozen lockfile — Fact F6).

**Validation gate**
- Commands: `gh pr checks --watch` (plus the pre-push `bun run test` && `bun run lint:actions`)
- Pass: `SDK Status`, `App Status`, `Accelerator Status`, `Actionlint Status` all green — includes
  sandbox e2e with the real accelerated proving path (`_e2e.yml` `build_accelerator: true`),
  WebDriver ×4, and both Windows jobs.
- Layers: full CI — lint · typecheck · unit · integration · e2e (local sandbox + built app).

## Phase 4 — Live testnet proving smoke (pre-merge gate)

1. Node pre-flight: `AZTEC_NODE_URL=https://v5.testnet.rpc.aztec-labs.com bun test
   packages/playground/src/aztec.test.ts` — live-node block asserts reachable + nodeVersion
   defined. (Testnet already serves `5.2.0-nightly.20260815` — verified 2026-08-18.)
2. SponsoredFPC drift check: run `packages/playground/scripts/deploy-sponsored-fpc.ts --salt 0x0`
   far enough to observe the derived salt=0 address and its `node.getContract` result.
   - Address unchanged & deployed → continue.
   - Address moved / not deployed → **STOP and surface to owner**: redeploy+fund needs a funded
     Sepolia `L1_PRIVATE_KEY` (Ask A2 — never handled autonomously). The smoke is blocked until
     the FPC exists and is funded.
3. Headless accelerator (canonical CI recipe, run-isolation rules apply — claim the port in
   `~/.agents/ports.md`, spawn detached, tear down own pgid only):
   `bun run --cwd packages/accelerator prebuild` → `cargo build` in `packages/accelerator/server`
   → run `target/debug/accelerator-server` (headless auto-approves localhost origins).
4. Scriptable UI smoke: `AZTEC_NODE_URL=https://v5.testnet.rpc.aztec-labs.com
   ACCELERATOR_URL=http://127.0.0.1:59833 bun run --cwd packages/playground test:e2e:smoke` —
   Playwright `smoke` project, deploy in BOTH accelerated and WASM modes (accelerated leg
   self-skips without `ACCELERATOR_URL` — that skip is a FAIL for this gate; real proving takes
   minutes per tx, timeout budget 15 min ×2 retries).
5. SDK remote-network leg: run the SDK e2e suite with the same env — `remote-network.test.ts`
   (testnet-only block) plus connectivity; the proving phase-trail asserts (`transmit` present,
   `fallback` absent) are the discriminator that the NATIVE path worked, not WASM fallback.
6. Version-banner note: the UI's SDK-vs-node compare is string-based; `5.2.0` vs
   `5.2.0-nightly.20260815` may render amber. Informational — do not treat as failure; log what it
   shows in lessons.
7. Smoke green → mark the PR ready for review; hand to owner for merge (Ask A3).

**Phase assumptions**: testnet stays on the 5.2.0 line during the smoke (Fact F5, re-verified at
step 1); sponsored-FPC fee path is the only funding dependency (recon (c) — Fact F8); headless
server accepts localhost Playwright origins without config (Fact F9, from CLAUDE.md headless
auto-approve + CI recipe).

**Validation gate**
- Commands: steps 1, 4, 5 above.
- Pass: live-node test green; Playwright smoke green **including the accelerated leg**; SDK
  remote-network/connectivity green with native-path phase-trail asserts. If blocked at step 2:
  gate outcome is "blocked-on-owner (FPC)", explicitly logged — not a silent pass.
- Layers: e2e against live testnet (the user-mandated layer).

---

## Architecture & Implementation (compact, light tier)

- **Reuse/location**: everything rides existing tooling — `scripts/update-aztec-version.ts` (pins +
  CRS bump), `scripts/check-windows-bb-pin.ts` (pin report), `packages/accelerator/scripts/copy-bb.ts`
  (pin table — the only hand-edited code besides bunfig.toml), CI recipe for the headless smoke.
  No new modules, no Rust changes (bb cache is dynamic per `x-aztec-version`; verified no version
  floor).
- **Touched files**: `bunfig.toml`, `packages/sdk/package.json`, `packages/playground/package.json`,
  `bun.lock`, `packages/playground/src/aztec.ts` (CRS string, script-written),
  `packages/accelerator/scripts/copy-bb.ts` (one pin entry), `implementations-plan/…` (this dir),
  plus any Phase-1 drift fixes (expected: none in SDK; possible in playground).
- **Critical flow**: updater rewrites pins → bun resolves 5.2.0 (min-age exclusion) → typecheck
  arbitrates source compat → pin unlocks Windows CI → sandbox e2e proves the native proving path →
  live smoke proves it against the real network.
- **Simpler alternative considered**: dispatch `aztec-stable.yml` (bot PR). Rejected by user —
  local gives immediate control over breakage fixes in the same branch; the workflow's auto-detect
  (dist-tag `rc`) wouldn't even see 5.2.0 (npm `rc` tag currently points at 4.3.0-rc.1) so it
  would have needed the forced-version input anyway.
- **N/A**: new interfaces/types, data-flow design, algorithms — dependency bump.

## Security & Adversarial Considerations

- **Threat model**: supply chain is the whole surface. (1) A compromised @aztec npm publish inside
  the 7-day window we are now explicitly waiving for that scope; (2) a tampered Windows bb release
  asset getting pinned; (3) lockfile poisoning hidden in a large regen diff.
- **Min-age waiver residual risk**: the exclusion permanently narrows the 7-day gate for the
  `@aztec/*` scope only. Mitigations: exact-pin versions (no ranges) mean resolution only moves
  when the updater rewrites pins; the lockfile-diff discipline (Phase 1.4) reviews every moved
  line; CI stays frozen-lockfile so nothing resolves outside an authored bump PR. Residual: a
  compromised release published as 5.2.0 today would not be age-filtered — accepted by owner
  decision 2026-08-18, on the grounds that @aztec is the product's first-party upstream and bump
  PRs are human-merged.
- **Windows pin (F-008)**: never auto-pinned. The entry added here carries full provenance (URL,
  date, hash command) so the owner can independently reproduce the hash at PR review; the
  fail-closed CI gate enforces presence+format; ruleset requires 0 approvals, so the REAL control
  is that the owner is the only merger — flagged, not fixable in this plan.
- **Least privilege**: no new tokens, no workflow changes, no secrets. The only credential in play
  is the owner-held Sepolia key for the FPC contingency (Ask A2), used interactively by the owner,
  never stored.
- **Input validation**: updater's strict version regex already gates `5.2.0`; lockstep skip is
  double-checked manually (fail-open `npm view` lesson).
- **Cryptography**: none added; bb binaries verified by sha256 pin (Windows) and npm-package
  extraction (Unix), both existing mechanisms.
- **Domain risks**: no contract/protocol changes shipped by us; testnet interop verified live
  before merge (Phase 4). Frontend/XSS surface untouched.

## Assumptions

**Facts (verified)**
- F1: All `@aztec/*` pins are exactly `5.0.1` in `packages/sdk/package.json` +
  `packages/playground/package.json` on base `b98352b`; `CRS_CACHE_VERSION = "5.0.1"` at
  `packages/playground/src/aztec.ts:168`.
- F2: `@aztec/aztec.js@5.2.0` and `@aztec/bb.js@5.2.0` published 2026-08-18 (~10:1x UTC); aztec.js
  5.2.0 is npm `latest`; `@aztec/bb-prover@5.2.0` depends on `@aztec/bb.js@5.2.0` (npm view).
- F3: `@aztec-foundation/aztec-standards` has no 5.2.0 — versions end at `5.1.0-rc.1` (npm view).
- F4: `WINDOWS_BB_CHECKSUMS` in `packages/accelerator/scripts/copy-bb.ts` ends at `"5.0.1"`; both
  Windows CI jobs fail closed without a 5.2.0 entry; `barretenberg-amd64-windows.tar.gz` exists on
  the v5.2.0 GitHub release (gh release view).
- F5: Live testnet `node_getNodeInfo` (2026-08-18): `nodeVersion 5.2.0-nightly.20260815`,
  `l1ChainId 11155111`, `realProofs true`, protocol feeJuice `0x03` — matches the hardcoded
  constant in the playground scripts.
- F6: `bunfig.toml` has `minimumReleaseAge = 604800` and no exclusion list; min-age gates
  resolution only — every CI job runs `bun install --frozen-lockfile` (all three workflows +
  composite actions, verified), so CI is unaffected.
- F7: Required checks on main (GitHub ruleset): `SDK Status`, `App Status`, `Accelerator Status`,
  `Actionlint Status`; `required_approving_review_count: 0`; squash/rebase only, linear history.
- F8: The playground smoke's only funding dependency is the sponsored FPC itself (fees via
  `SponsoredFeePaymentMethod`); FPC funding needs an owner-held Sepolia L1 key.
- F9: CI's `_e2e.yml` starts `accelerator-server` bare (no origin config) and the SDK e2e's
  accelerated leg passes against it — headless auto-approves localhost.

**Inferences (attackable)**
- I1: Bun's `minimumReleaseAgeExcludes` accepts the `"@aztec/*"` scope wildcard. Undocumented —
  Phase 1.3 is the empirical test; fallback to exact names is in-plan.
- I2: Every `@aztec/*` package pinned in the two package.jsons has a 5.2.0 on npm (verified for 3;
  the updater's per-package `npm view` pre-check is the full test, and any skip is a Phase-1
  blocker, not a warn-and-continue).
- I3: The live-resolved `@aztec/bb.js` version string will be exactly `"5.2.0"` (historically the
  bb.js version == the release tag; Phase 2.1 verifies before pinning).
- I4: TS API drift for OUR surfaces is nil-to-small (5.2.0 notes claim no breaking TS changes;
  5.1.0's breaks are aztec.nr-side; SDK coupling is 5 symbols). The 3-graph typecheck + sandbox
  e2e + live smoke are the layered proof, precisely because tsc can't see the runtime-only
  contracts (base-class constructor arg, dynamic `import("@aztec/simulator/client")`,
  `node_getNodeInfo` response shape).
- I5: The SponsoredFPC salt=0 address will move (it did on all 3 prior cycles). Treated as
  expected-case in Phase 4.2, with the owner-gated funding contingency.

**Asks (for the approval gate — none silently assumed)**
- A1: `@aztec-foundation/aztec-standards` — hold at 5.0.1 (updater default; recommended, smoke
  arbitrates) or hand-pin `5.1.0-rc.1`? Recommended: hold; if the token flow fails in the smoke,
  try `5.1.0-rc.1` as the first remedy and record it in lessons.
- A2: If the FPC address moved (expected), the redeploy+fund step needs your funded Sepolia
  `L1_PRIVATE_KEY`, run by you (or by me only while you're present and supply the env
  interactively). Confirm you'll be available for that step, or pre-authorize the exact command.
- A3: Merge is yours: PR goes ready-for-review only after the smoke gate; you review the Windows
  pin provenance note and merge (squash).

## Decision ledger (light)

- Vehicle: local updater run in worktree (user, at clarify) — bot dispatch rejected.
- Min-age: permanent `@aztec/*` exclusion in bunfig.toml (user, 2026-08-18) — one-off
  `--minimum-release-age=0` override rejected as it re-litigates every cycle.
- Scope: bump PR merged = done; publish/deploy/release prep excluded (user, at clarify).
- Validation: PR CI + live testnet smoke (user, at clarify).
- aztec-standards: pending Ask A1.

## Post-implementation (self-contained — the implementing session executes THIS, not the skill)

1. Run `/code-review max --fix` on the implementation diff. Skim the applied fixes for unintended
   changes, then commit them SEPARATELY from implementation commits (identifiable as
   code-review-applied).
2. Codex post-impl audit — `/codex xhigh` with: the net diff from plan baseline (`b98352b`), a
   separate summary of the code-review commits, this plan.md + decision ledger, an explicit
   adversarial/security ask, and this rule verbatim: "Report bugs and small, targeted improvements
   only. Do not propose speculative abstractions, extra configuration surface, new layers, or
   rewrites — the smallest change that fixes each real problem. If code works and is clear, leave
   it alone."
3. Iterative fix loop: verify codex's factual claims against the repo before acting; apply accepted
   fixes; commit; log the consult + verdict in `lessons/`; RESUME the same codex session with the
   fix diff for re-review. Repeat until a round yields no new material findings (rejected nitpicks
   don't count). Still churning after 3 rounds → stop and surface (scope smell).
4. Delivery per the Delivery section below; keep the PR draft until the Phase-4 smoke gate passes,
   then mark ready and hand to the owner for merge.
5. On merge: update `implementations-plan/index.md` (completed marker), suggest
   `agent-worktree done aztec-5.2.0-2026-08-18` (owner's call).

## Delivery

**Single arc, single PR** — no stack ceremony. Branch `worktree-aztec-5.2.0-2026-08-18` → PR
titled `chore(aztec): bump @aztec/* 5.0.1 → 5.2.0` against main via `gh pr create --draft`
(Phase 3), marked ready after Phase 4. All four phases ship in this one PR: the bump is only
reviewable/revertable as a unit (pins + lockfile + CRS + pin entry must move together — the
`_aztec-update.yml` same-commit guard is the precedent).

## Seeds (DRAFT — finalized after approval)

Artifact URL: (recorded at publish)

**Recommended: `/goal`** (completion is transcript-observable: phase ✓s, gate outputs, PR state).

```
/goal All four phases marked ✓ in implementations-plan/aztec-5.2.0-2026-08-18/plan.md (the phase headers in the file, not the chat), each ✓ backed by its phase's validation gate as defined in plan.md reported passing in the transcript; for each phase LESSONS_FILE=implementations-plan/aztec-5.2.0-2026-08-18/lessons/phase-N.md printed in the transcript; /code-review max --fix complete with fixes committed separately; the codex post-impl fix loop converged (a resumed codex pass reporting no new material findings, quoted in the transcript); the PR exists and is marked ready with all four required Status checks green (gh pr view output in the transcript); bun run test and bun run lint:actions both exit 0 in the transcript. If Phase 4 blocks on the FPC redeploy (owner-gated Sepolia key), the goal is instead: Phases 1–3 ✓, the blocked state logged in lessons/phase-4.md, and the owner ping surfaced in the transcript.
```

**Alternative: `/loop 15m`**

```
/loop 15m Drive implementations-plan/aztec-5.2.0-2026-08-18 forward. Never idle waiting for my input. Each firing: (1) Reality check: read plan.md + lessons/ (authoritative), git status, git log --oneline -5; if the PR exists, gh pr view --json statusCheckRollup (no --watch). (2) Waiting on CI is fine — confirm it progresses (gh run watch <id> up to 10 min); use waits to review the diff or prep the next phase. (3) No task in hand? Take the next pending plan.md step. After each meaningful edit run bun run test for the touched surface; commit and push. (4) Stuck or facing a decision I'd normally make? /codex xhigh, reach a defensible call, act, log consult+verdict in lessons/phase-N.md. Hard limits stay hard: never merge, never publish, never touch the Sepolia key (FPC redeploy is owner-only — log blocked and continue elsewhere), never expand scope. (5) Same step failed 5 times? Stop retrying, reassess with codex. (6) Phase green means ITS VALIDATION GATE in plan.md passed — run it, paste the result, mark ✓ in plan.md, write lessons/phase-N.md, print LESSONS_FILE=implementations-plan/aztec-5.2.0-2026-08-18/lessons/phase-N.md. (7) All phases ✓? Run plan.md's Post-implementation section (code-review max --fix → separate commit → codex xhigh audit with the no-over-engineering rule → resumed-codex fix loop to convergence), mark the PR ready, write the wrap-up report, surface and stop.
```

Use exactly ONE per session — they don't compose.
