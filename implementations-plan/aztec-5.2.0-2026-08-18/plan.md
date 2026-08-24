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
  `bunfig.toml` (5.2.0 published 2026-08-18, inside the 7-day window). Codex audit surfaced a
  SECOND, independent min-age surface in CI (the Aztec CLI installer) — see Phase 2 and Ask A4.
- **eli5_mode**: artifact. ELI5 source: `eli5.html` in this dir; Artifact URL recorded in Seeds
  section once published.
- **Worktree/branch**: `aztec-5.2.0-2026-08-18` / `worktree-aztec-5.2.0-2026-08-18`, base
  `b98352b` (origin/main).
- Recon: see `recon.md` (same dir). Codex audit: `audit-codex.md` (same dir) — final verdict on
  rev 3: **approve** (3 rounds; all round-1/round-2 conditions addressed and verified).

---

## Phase 1 ✓ — Bump + Windows pin (one local gate) — GREEN 2026-08-19, commit `2780336`

*(Codex: former Phases 1+2 merged — observing the missing-pin report and running the full suite
twice added no confidence.)*

1. Edit `bunfig.toml`: under `[install]`, add `minimumReleaseAgeExcludes = [...]` as an
   **exact-name list** — empirically settled 2026-08-18 on Bun 1.3.14 (the latest stable, and CI's
   pin): exact names are honored (tested: excluding `@aztec/foundation` let it resolve and moved
   the block to its transitive `@aztec/bb.js`), the `"@aztec/*"` wildcard is silently ignored
   (tested: identical block with and without it), and the filter applies to TRANSITIVE resolution.
   Seed the list from the current lock's 31 distinct `@aztec/`-scoped names (includes the
   `@aztec/viem` alias). Comment records: rationale (first-party trusted upstream; bump flow
   inherently targets fresh releases; owner decision 2026-08-18), residual risk (see Security),
   and the maintenance note that future bumps may need list updates when Aztec's graph changes.
2. `bun run aztec:update 5.2.0` — expect the loud warn-skip for `@aztec-foundation/aztec-standards`
   (stays 5.0.1 per Ask A1). Verify stdout lists every `@aztec/*` package as updated; treat any
   OTHER skip as a blocker (fail-open `npm view` lesson from the 5.0.1 cycle — re-run to
   distinguish 404 from network blip).
3. `bun install` — **iterative, fail-closed** (codex: the old lock can't enumerate NEW 5.2.0
   transitives): each min-age block error names the exact blocked `@aztec/*` package → add it to
   the excludes list → re-run, until resolution succeeds. Only `@aztec/`-scoped names may ever be
   added; a blocked NON-@aztec package is a stop-and-investigate, never an exclusion. After the
   final lock resolves, prune list entries not present in the new lock (keep the waiver minimal)
   and review the result.
4. Lockfile discipline: `git diff bun.lock` — every changed line must trace to an `@aztec/*`
   package; **each non-@aztec change must be individually explained from a changed dependency
   edge** (no category-level waving-through). Anything unexplained is stop-and-investigate.
5. `bun install --frozen-lockfile` — must pass (CI parity for the bun surface).
6. Windows bb pin (F-008 flow):
   a. Confirm the live bb.js version: `grep '"version"' node_modules/@aztec/bb.js/package.json`
      (expected `5.2.0`; the pin key MUST be this exact string, not the release tag).
   b. Download `https://github.com/AztecProtocol/aztec-packages/releases/download/v5.2.0/barretenberg-amd64-windows.tar.gz`,
      compute `sha256sum`; record the GitHub asset id + digest from
      `gh release view v5.2.0 --repo AztecProtocol/aztec-packages --json assets` alongside.
   c. Add `WINDOWS_BB_CHECKSUMS["5.2.0"] = { sha256, provenance: "manual-review", note }` in
      `packages/accelerator/scripts/copy-bb.ts`. The note must contain the release URL, fetch
      date, the one-command reproduction
      (`curl -fsSL -o bb-win.tar.gz <url> && sha256sum bb-win.tar.gz` — download to file, not
      piped), and the API-reported digest/asset-id; the computed hash MUST equal the API digest. This is honestly a **change detector**, not provenance (codex): it
      authenticates nothing if the asset was compromised before both observations — the real
      control is Ask A3 (owner's independent digest before merge).
7. `bun scripts/check-windows-bb-pin.ts` — must now report a reviewed pin for the live bb.js
   version.
8. Optional single local proof: `bun run --cwd packages/accelerator prebuild` (extracts Linux bb
   from the installed `@aztec/bb.js`, regenerates gitignored `src-tauri/AZTEC_VERSION`) — proves
   `resolveAztecBb()` resolves 5.2.0 before CI does.
9. Fix any typecheck/unit fallout from API drift (recon: SDK surface minimal, 5.2.0 claims no
   breaking TS changes; historical breaks land in playground code — fix in place, separate
   commits for non-trivial fixes).

**Phase assumptions**: exact-name excludes are honored by Bun 1.3.14 incl. transitives (F11 —
verified); the iterative procedure converges because every new 5.2.0 transitive is @aztec-scoped
(I1); all pinned @aztec packages have a 5.2.0 (I2 — updater pre-check is the full test); bb.js
string is "5.2.0" (I3, verified 6a); the Windows asset corresponds to bb.js 5.2.0 (I6 — CI
Windows Build Smoke is the enforcing proof).

**Validation gate**
- Commands: `bun run test` && `bun scripts/check-windows-bb-pin.ts`
  (codex: root `test` already includes playground `typecheck:scripts` via `test:typecheck` and
  accelerator `test:unit` via `test:unit` — verified in root package.json; no separate runs).
- Pass: both exit 0; pin-check reports a reviewed pin; lockfile diff fully explained.
- Layers: lint · typecheck · unit.

## Phase 2 ✓ — CI min-age exemption for the Aztec CLI installer — GREEN 2026-08-19, commit `2e78bcc` (npm-leg validation + control; full path proves in Phase-3 CI)

**Why**: `setup-aztec/action.yml` runs the Aztec CLI installer with
`npm_config_min_release_age=7` (post-snappy-outage quarantine of the installer's UNLOCKED
transitive tree). Empirically verified 2026-08-18: npm 11.16 refuses even the exact-pinned
same-day 5.2.0 (`ETARGET … before 8/11/2026`). Without this phase, the sandbox e2e legs
(`_e2e.yml` → SDK Status, `_e2e-app.yml` → App Status, plus `_e2e-packaged.yml` on the release
path) stay red until ~Aug 25. Also verified: npm **12** ships `min-release-age-exclude`
(minimatch globs; `npm_config_min_release_age_exclude='@aztec/*'` env form works — tested, 281
packages resolved); npm 11.16 has no such key in any form (npmrc/flag/env all tested).

1. In the "Install Aztec CLI" step of `.github/actions/setup-aztec/action.yml`:
   - Upgrade npm first: `npm install -g npm@12.0.2` (exact pin).
   - Add `export npm_config_min_release_age_exclude='@aztec/*'` next to the existing
     `npm_config_min_release_age=7` — @aztec (first-party, exact-version-pinned by the installer)
     exempted; the 7-day quarantine on all third-party transitives (the snappy defense) stays.
   - Extend the step's comment block with the rationale + date.
2. Bump the cache-key salt (`-minage7` → `-minage7-npm12-aztecexempt`) — the comment says the salt
   is load-bearing: without the bump, a cached tree skips the changed install recipe.
3. **npm 12 install-scripts caveat — FAIL CLOSED** (codex round 2): npm 12 blocked 2 packages'
   install scripts by default in a dry run (`allowScripts` gating) — exactly the class of
   native-binding breakage the action's snappy probe defends against. Before pushing, validate
   locally on this Linux box: run the installer end-to-end with the same env
   (`npm_config_min_release_age=7 npm_config_min_release_age_exclude='@aztec/*'`, npm 12,
   `VERSION=5.2.0 bash <(curl -fsSL https://install.aztec.network/5.2.0/install)` into a scratch
   `HOME`), then run the action's own snappy probe against the result. If the installer or probe
   fails: **stop and report the exact blocked packages** — never broadly enable install scripts.
   Any exception must be an explicit minimal per-package allowlist, added via a plan revision
   (logged in lessons + ledger) and revalidated locally. The action's loud probe step is the CI
   backstop either way.

**Phase assumptions**: the installer honors ambient `npm_config_*` (it did for
`min_release_age` — that's how the quarantine works today); npm 12's exclude semantics hold for
the installer's internal resolutions (validated locally in step 3, enforced by the probe in CI).

**Validation gate**
- Commands: local installer run + snappy probe (step 3); `bun run lint:actions`.
- Pass: installer completes for 5.2.0 under the 7-day+exclude env; probe loads snappy; actionlint
  exit 0.
- Layers: lint · integration (installer, local).

## Phase 3 ✓ — Commit, PR, CI green — GREEN 2026-08-19: PR #467, all 4 required Status checks SUCCESS (2 CI rounds + flake reruns; see lessons/phase-3.md)

1. Stage exactly the canonical bump set + this cycle's additions:
   `packages/sdk/package.json packages/playground/package.json bun.lock
   packages/playground/src/aztec.ts packages/accelerator/scripts/copy-bb.ts bunfig.toml
   .github/actions/setup-aztec/action.yml implementations-plan/` — then verify `git status` shows
   nothing else touched by the updater (replicates `_aztec-update.yml`'s `git diff --exit-code`
   guard).
2. Commit `chore(aztec): bump @aztec/* 5.0.1 → 5.2.0` (separate commits for the setup-aztec change
   and any Phase-1 code fixes). Pre-push local gate: `bun run test` && `bun run lint:actions`.
3. Push; `gh pr create --draft` (draft until the Phase-4 smoke passes — ruleset needs 0 approvals,
   so draft is the only pre-merge brake).
4. `gh pr checks --watch` — expect ~20–25 min, long pole Windows Build Smoke (~20 min), which is
   also the enforcing proof of the Phase-1 pin. Note: the ruleset requires branches up to date —
   any post-CI fix or main movement restarts the required checks; budget for it.

**Phase assumptions**: the three package workflows + actionlint fire and aggregate into the 4
required Status checks (F7 — setup-aztec is explicitly in sdk.yml/app.yml path filters, and
actionlint's filter covers `.github/actions/**`, so all four run for real).

**Validation gate**
- Commands: `gh pr checks --watch` (plus the pre-push `bun run test` && `bun run lint:actions`)
- Pass: `SDK Status`, `App Status`, `Accelerator Status`, `Actionlint Status` all green — includes
  sandbox e2e with the real accelerated proving path (`_e2e.yml` `build_accelerator: true`), the
  npm-12 installer path, WebDriver ×4, both Windows jobs, and the local-network token-flow spec
  (the aztec-standards@5.0.1 × @aztec 5.2.0 compat arbiter — Ask A1).
- Layers: full CI — lint · typecheck · unit · integration · e2e (local sandbox + built app).

## Phase 4 ✓ — Live testnet proving smoke — GREEN 2026-08-24 (FPC third-party-deployed+funded, owner key unused; Playwright smoke ✓, SDK e2e 10/10 phase-trail ✓, token flow 4/4 ✓ — see lessons/phase-4.md)

1. Node pre-flight: `AZTEC_NODE_URL=https://v5.testnet.rpc.aztec-labs.com bun test
   packages/playground/src/aztec.test.ts` — live-node block asserts reachable + nodeVersion
   defined. (Testnet already serves `5.2.0-nightly.20260815` — verified 2026-08-18.)
2. **Read-only** SponsoredFPC drift check (codex: `deploy-sponsored-fpc.ts` exits without L1
   creds before deriving — cannot be used as a preflight). Run this secret-free one-liner from
   `packages/playground`:
   ```
   bun -e 'import {SponsoredFPCContract} from "@aztec/noir-contracts.js/SponsoredFPC";
   import {getContractInstanceFromInstantiationParams} from "@aztec/stdlib/contract";
   import {createAztecNodeClient} from "@aztec/aztec.js/node";
   import {Fr} from "@aztec/aztec.js/fields";
   const node = createAztecNodeClient("https://v5.testnet.rpc.aztec-labs.com");
   const inst = await getContractInstanceFromInstantiationParams(SponsoredFPCContract.artifact, {salt: Fr.fromHexString("0x0")});
   console.log("derived:", inst.address.toString());
   console.log("deployed:", !!(await node.getContract(inst.address)));'
   ```
   - `deployed: true` → continue.
   - `deployed: false` (address moved — expected per I5) → **STOP and surface to owner**:
     redeploy+fund via `deploy-sponsored-fpc.ts --salt 0x0` needs the owner's funded Sepolia
     `L1_PRIVATE_KEY` (Ask A2 — owner-present only, never autonomous). The smoke is blocked until
     the FPC exists and is funded; log the blocked state in lessons — that is a legitimate gate
     outcome, not a silent pass.
3. Headless accelerator (canonical CI recipe, run-isolation rules apply — claim the port in
   `~/.agents/ports.md`, spawn detached, tear down own pgid only):
   `bun run --cwd packages/accelerator prebuild` → `cargo build` in `packages/accelerator/server`
   → run `target/debug/accelerator-server` (headless auto-approves localhost origins — F9).
4. Playwright smoke (deploy-only — it has NO token flow): `AZTEC_NODE_URL=https://v5.testnet.rpc.aztec-labs.com
   ACCELERATOR_URL=http://127.0.0.1:59833 bun run --cwd packages/playground test:e2e:smoke` —
   deploy in BOTH accelerated and WASM modes; the accelerated leg self-skipping (unset
   `ACCELERATOR_URL`) is a FAIL for this gate. Real testnet proving takes minutes per tx (15-min
   timeout ×2 retries budgeted).
5. SDK native-path e2e — exact command (codex: `test:e2e:remote` is connectivity-only; the
   `transmit`-present/`fallback`-absent asserts live in `proving.test.ts`):
   `AZTEC_NODE_URL=https://v5.testnet.rpc.aztec-labs.com ACCELERATOR_URL=http://127.0.0.1:59833
   bun run --cwd packages/sdk test:e2e` — runs connectivity + proving (phase-trail asserts = the
   native-path discriminator) + the testnet-only remote-network block.
6. Manual token flow vs testnet (A1's live arbiter; budget ~20+ min — real proving per tx):
   `bun run --cwd packages/playground dev:testnet`. **Run Token Flow is disabled in a fresh
   session until an account exists** (codex round 2): first select the intended mode, click
   **Deploy Test Account**, wait for success, THEN click **Run Token Flow**; success = log shows
   `Token flow complete` + `Balances — Alice: 500, Bob: 500` with explorer links. A fee-payment
   failure here can also mean the FPC is deployed-but-underfunded — that's the A2 contingency
   (`--fund-only`), not an aztec-standards compat failure; distinguish before touching A1's
   remedy. (CI's local-network spec covers the flow against the 5.2.0 sandbox; this step proves
   it against real testnet.)
7. Version-banner note: the UI's SDK-vs-node compare is string-based; `5.2.0` vs
   `5.2.0-nightly.20260815` may render amber. Informational — log what it shows, don't fail on it.
8. Smoke green → mark the PR ready; hand to owner for merge. (A3 as resolved: the agent's
   two-channel digest verification — file hash == API digest — is already documented in the pin
   note; no owner comment required.)

**Phase assumptions**: testnet stays on the 5.2.0 line during the smoke (F5, re-verified step 1);
sponsored-FPC fee path is the only funding dependency (F8); headless server accepts localhost
origins without config (F9).

**Validation gate**
- Commands: steps 1, 2, 4, 5, 6 above.
- Pass: live-node test green; read-only FPC check `deployed: true` (or explicit blocked-on-owner
  outcome logged); Playwright smoke green INCLUDING the accelerated leg; SDK `test:e2e` green with
  phase-trail asserts; manual token flow complete. Blocked-at-step-2 outcome: Phases 1–3 stand,
  Phase 4 logged blocked, owner pinged — never silently passed.
- Layers: e2e against live testnet (the user-mandated layer).

---

## Architecture & Implementation (compact, light tier)

- **Reuse/location**: everything rides existing tooling — `scripts/update-aztec-version.ts` (pins +
  CRS bump), `scripts/check-windows-bb-pin.ts` (pin report), `packages/accelerator/scripts/copy-bb.ts`
  (pin table — the only hand-edited TS besides bunfig.toml), the CI installer recipe for the
  headless smoke. No new modules, no Rust changes (bb cache is dynamic per `x-aztec-version`;
  verified no version floor).
- **Touched files**: `bunfig.toml`, `packages/sdk/package.json`, `packages/playground/package.json`,
  `bun.lock`, `packages/playground/src/aztec.ts` (CRS string, script-written),
  `packages/accelerator/scripts/copy-bb.ts` (one pin entry),
  `.github/actions/setup-aztec/action.yml` (npm 12 + exclude + salt), `implementations-plan/…`,
  plus any Phase-1 drift fixes (expected: none in SDK; possible in playground).
- **Critical flow**: updater rewrites pins → bun resolves 5.2.0 (bunfig exclusion) → typecheck
  arbitrates source compat → pin unlocks Windows CI → installer exclusion unlocks sandbox e2e →
  sandbox e2e proves the native proving path → live smoke proves it against the real network.
- **Simpler alternative considered**: dispatch `aztec-stable.yml` (bot PR) — rejected by user;
  also its auto-detect (dist-tag `rc` → 4.3.0-rc.1) wouldn't find 5.2.0 without the forced-version
  input. Second alternative: skip Phase 2 and wait out the 7-day window (merge ~Aug 25) — Ask A4.
- **N/A**: new interfaces/types, data-flow design, algorithms — dependency bump.

## Security & Adversarial Considerations

- **Threat model**: supply chain is the whole surface. (1) A compromised @aztec npm publish inside
  the 7-day window we are explicitly waiving for that scope (now on TWO surfaces: bunfig + the CI
  installer); (2) a tampered Windows bb release asset getting pinned; (3) lockfile poisoning
  hidden in a large regen diff; (4) the npm@12 upgrade itself (new major, new install-scripts
  semantics).
- **Min-age waiver residual risk** (codex High — accepted with eyes open): the waiver removes the
  ONLY observation window for a maliciously published @aztec release; exact pins + lockfile review
  + green tests establish reproducibility and compatibility, **not benign behavior**. It covers
  the final resolved `@aztec/*` graph (incl. the `@aztec/viem` alias and transitives — the pruned
  exact-name list), not arbitrary future package names.
  Mitigations: resolution only moves when a human-authored bump PR runs the updater; lockfile-diff
  discipline reviews every moved line; CI stays frozen-lockfile (bun surface); the installer keeps
  the 7-day gate for every NON-@aztec package (the snappy defense). Accepted by owner decision
  2026-08-18 on the grounds that @aztec is the product's first-party upstream and bump PRs are
  human-merged. Revisit if @aztec publishing is ever compromised upstream.
- **Windows pin (F-008)**: never auto-pinned. The entry added here is a change detector, not
  provenance — it cannot authenticate an asset compromised before observation. Controls: full
  reproduction note (URL, date, command, API digest/asset-id), the fail-closed format gate in CI,
  and Ask A3 (owner's independently computed digest posted to the PR before merge — real
  separation of duties, since ruleset review count is 0 and the owner is sole merger).
- **npm@12 pin**: exact-pinned (`npm@12.0.2`) global upgrade inside an ephemeral CI runner; its
  stricter install-scripts default is validated locally (Phase 2.3) and backstopped by the
  action's loud snappy probe.
- **Least privilege**: no new tokens, no secrets. The only credential in play is the owner-held
  Sepolia key for the FPC contingency (A2), used interactively by the owner, never stored.
- **Input validation**: updater's strict version regex gates `5.2.0`; lockstep skip double-checked
  manually (fail-open `npm view` lesson).
- **Cryptography**: none added; bb binaries verified by sha256 pin (Windows) and npm-package
  extraction (Unix), both existing mechanisms.
- **Domain risks**: no contract/protocol changes shipped by us; testnet interop verified live
  before merge (Phase 4). Frontend/XSS surface untouched.

## Assumptions

**Facts (verified)**
- F1: All **updater-managed direct `@aztec/*` keys** (13 distinct across the two manifests) are
  exactly `5.0.1` on base `b98352b`; `CRS_CACHE_VERSION = "5.0.1"` at
  `packages/playground/src/aztec.ts:168`. (The playground ALSO carries the alias
  `viem: npm:@aztec/viem@2.38.2` — not updater-managed, but inside the waived scope.)
- F2: `@aztec/aztec.js@5.2.0` and `@aztec/bb.js@5.2.0` published 2026-08-18 (~10:1x UTC); aztec.js
  5.2.0 is npm `latest`; `@aztec/bb-prover@5.2.0` depends on `@aztec/bb.js@5.2.0` (npm view).
- F3: `@aztec-foundation/aztec-standards` has no 5.2.0 — versions end at `5.1.0-rc.1` (npm view).
- F4: `WINDOWS_BB_CHECKSUMS` ends at `"5.0.1"`; both Windows CI jobs fail closed without a 5.2.0
  entry (presence+format only — CI proves hash equality, not that review happened);
  `barretenberg-amd64-windows.tar.gz` exists on the v5.2.0 release (gh release view).
- F5: Live testnet `node_getNodeInfo` (2026-08-18): `nodeVersion 5.2.0-nightly.20260815`,
  `l1ChainId 11155111`, `realProofs true`, protocol feeJuice `0x03` — matches the hardcoded
  constant in the playground scripts.
- F6 (corrected per codex): CI has TWO min-age surfaces. (a) Bun: every job runs
  `bun install --frozen-lockfile` → never blocked. (b) npm: `setup-aztec`'s CLI installer exports
  `npm_config_min_release_age=7` and installs UNLOCKED — empirically blocks same-day 5.2.0 on npm
  11.16 (ETARGET, tested 2026-08-18). npm 12.0.2's `min-release-age-exclude` resolves it (tested,
  env-var form included); npm 11.16 lacks the key entirely (npmrc/flag/env all tested).
- F7: Required checks on main (GitHub ruleset): `SDK Status`, `App Status`, `Accelerator Status`,
  `Actionlint Status`; `required_approving_review_count: 0`; squash/rebase only, linear history,
  strict up-to-date required.
- F8: The playground smoke's only funding dependency is the sponsored FPC itself; FPC funding
  needs an owner-held Sepolia L1 key. `deploy-sponsored-fpc.ts` REQUIRES L1 creds before deriving
  anything (verified — hence the read-only preflight in Phase 4.2).
- F9: CI's `_e2e.yml` starts `accelerator-server` bare (no origin config) and the SDK e2e's
  accelerated leg passes against it — headless auto-approves localhost.
- F10: The Playwright `smoke` project is deploy-only (no token flow); the token-flow spec lives in
  the `local-network` project (CI) and the manual UI (`Run Token Flow`, which requires a deployed
  session account first).
- F11 (empirical, 2026-08-18, Bun 1.3.14 = latest stable = CI's pin): `minimumReleaseAgeExcludes`
  honors EXACT names (excluding `@aztec/foundation` resolved it and moved the block to its
  transitive `@aztec/bb.js`); the `"@aztec/*"` wildcard is silently ignored; min-age filters
  transitive resolution. Current lock holds 31 distinct `@aztec/`-scoped names (incl.
  `@aztec/viem`).

**Inferences (attackable)**
- I1: The iterative exclude procedure (Phase 1.3) converges — i.e. every NEW package 5.2.0's
  graph introduces under min-age is itself `@aztec/`-scoped. If a non-@aztec package blocks,
  that's a designed stop-and-investigate, not an exclusion.
- I2: Every `@aztec/*` package pinned in the two manifests has a 5.2.0 on npm (verified for 3; the
  updater's per-package `npm view` pre-check is the full test; any skip is a Phase-1 blocker).
- I3: The live-resolved `@aztec/bb.js` version string will be exactly `"5.2.0"` (historically
  bb.js == the release tag; Phase 1.6a verifies before pinning).
- I4: TS API drift for OUR surfaces is nil-to-small (5.2.0 notes claim no breaking TS changes;
  5.1.0's breaks are aztec.nr-side; SDK coupling is 5 symbols). Layered proof: 3-graph typecheck +
  sandbox e2e + live smoke — precisely because tsc can't see runtime-only contracts (base-class
  constructor arg, dynamic `import("@aztec/simulator/client")`, `node_getNodeInfo` shape).
- I5: The SponsoredFPC salt=0 address will move (it did on all 3 prior cycles). Expected-case in
  Phase 4.2, owner-gated funding contingency A2.
- I6 (split out of I4 per codex): the v5.2.0 release's `barretenberg-amd64-windows.tar.gz` is the
  Windows build of bb.js 5.2.0's bb. Historically true; CI's Windows Build Smoke (which builds and
  runs against the pinned asset) is the enforcing proof.
- I7: The Aztec installer honors ambient `npm_config_*` overrides under npm 12 exactly as it does
  under npm 11 (validated locally in Phase 2.3 before any push).

**Asks — RESOLVED at the approval gate (owner, 2026-08-19)**
- A1 ✔ HOLD at 5.0.1 ("I think it should work with 5.0.1 still. Let's give it a try. We can
  update afterwards."). Arbiters: CI's local-network token-flow spec (vs the 5.2.0 sandbox) and
  Phase 4.6's manual token flow vs testnet — NOT the Playwright smoke (no token flow there). If
  the token flow fails on aztec-standards compat, try `5.1.0-rc.1` as the first remedy and record
  it in lessons.
- A2 ✔→⚠ Owner directed "check all the available worktrees for a .env". Searched 2026-08-19: NO
  real `.env` exists on this machine — all worktrees, both clones, all of `~/Projects` env files,
  `~/.agents` hold only `.env.example` templates without values. The key likely lives on the
  owner's other machine (the 5.0.1-era FPC funding). Standing resolution: if Phase 4.2/4.6 needs
  deploy or funding, the owner drops the key into `packages/playground/scripts/.env`
  (verified gitignored: `.gitignore:6`) and the agent runs `deploy-sponsored-fpc.ts --salt 0x0`
  (or `--fund-only`) WITHOUT ever printing the value; until then that step parks as
  blocked-on-owner. Fee-payment failures ≠ compat failures — disambiguate before touching A1's
  remedy.
- A3 ✔ FLIPPED to the agent ("please do it yourself"): the agent performs the verification via
  two independent channels — download-to-file `sha256sum` AND the GitHub API-reported asset
  digest, which MUST be equal — and documents both in the pin note. No owner PR comment required;
  the owner's review at merge is discretionary.
- A4 ✔ (answered in chat; proceeding): the CI file carries a SECOND, independent 7-day npm
  quarantine for the Aztec sandbox installer — empirically shown to reject same-day 5.2.0 — so
  without the edit the e2e legs stay red until ~Aug 25. Proceeding on the strength of the owner's
  standing min-age decision ("if not excluded, let's include it"); the edit is worktree-branch
  only, PR-reviewed, reversible. Owner veto at PR review reverts to the wait-to-Aug-25
  alternative.

## Decision ledger (light)

- Vehicle: local updater run in worktree (user, at clarify) — bot dispatch rejected.
- Min-age (bun): permanent `@aztec/*` exclusion in bunfig.toml (user, 2026-08-18) — one-off
  `--minimum-release-age=0` override rejected as it re-litigates every cycle.
- Min-age (npm/CI): npm@12 + scoped exclude in setup-aztec (codex finding; pending Ask A4) —
  blanket removal of the installer quarantine rejected (it defends the unlocked transitive tree —
  the snappy outage); waiting 7 days rejected by default per user's momentum decision.
- Scope: bump PR merged = done; publish/deploy/release prep excluded (user, at clarify).
- Validation: PR CI + live testnet smoke (user, at clarify).
- aztec-standards: pending Ask A1.
- Codex round 1 (2026-08-18, session in `audit-codex.md`): conditional approve. Adopted: F6
  correction + Phase 2 (verified empirically), read-only FPC preflight, Phases-1+2 merge, exact
  SDK e2e command, F1/I1/I4 corrections + I6/I7/F10, lockfile-criterion tightening, A3
  strengthening, A1 arbiter correction, provenance→change-detector rewording. Rejected: nothing
  material.
- **APPROVAL (owner, 2026-08-19)**: conditional approve — A1 hold at 5.0.1; A2 key-search
  directed (result: not on this machine → park-if-needed with gitignored `.env` drop-point); A3
  flipped to agent two-channel verification; A4 answered + proceeding under the standing min-age
  decision. Plan is APPROVED for implementation with these dispositions.
- Codex round 2 (same session, rev 2): conditional approve, 3 blockers — all adopted:
  (1) allowScripts branch made fail-closed (minimal per-package allowlist only, via plan
  revision); (2) Bun exclude mechanism settled EMPIRICALLY (exact names work on 1.3.14, wildcard
  ignored, transitives filtered → iterative fail-closed exact-name list, F11); (3) token-flow
  step gained the Deploy-Test-Account prerequisite + A2 extended to underfunded-FPC. Non-blocking
  gate-redundancy simplification also adopted.

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
   then mark ready and hand to the owner for merge (with the A3 digest comment).
5. On merge: update `implementations-plan/index.md` (completed marker), suggest
   `agent-worktree done aztec-5.2.0-2026-08-18` (owner's call).

## Delivery

**Single arc, single PR** — no stack ceremony. Branch `worktree-aztec-5.2.0-2026-08-18` → PR
titled `chore(aztec): bump @aztec/* 5.0.1 → 5.2.0` against main via `gh pr create --draft`
(Phase 3), marked ready after Phase 4. All phases ship in this one PR: pins + lockfile + CRS +
pin entry + installer exemption must move together to be green and revertable as a unit (the
`_aztec-update.yml` same-commit guard is the precedent). Within the PR, separate commits: bump ·
setup-aztec change · any drift fixes · code-review fixes.

## Seeds (FINAL — approved scope, 2026-08-19)

No scope changes at approval affect the seeds: A1/A3/A4 dispositions are already encoded in
plan.md (the seeds' source of truth), and the /goal's blocked-on-owner clause covers the A2
key-not-on-this-machine outcome verbatim.

Artifact URL: https://claude.ai/code/artifact/3634bcb6-385b-4e6d-af23-cc04fa4ca176
(source: `eli5.html` in this dir — redeploying that same file updates the same URL)

**Recommended: `/goal`** (completion is transcript-observable: phase ✓s, gate outputs, PR state).

```
/goal All four phases marked ✓ in implementations-plan/aztec-5.2.0-2026-08-18/plan.md (the phase headers in the file, not the chat), each ✓ backed by its phase's validation gate as defined in plan.md reported passing in the transcript; for each phase LESSONS_FILE=implementations-plan/aztec-5.2.0-2026-08-18/lessons/phase-N.md printed in the transcript; /code-review max --fix complete with fixes committed separately; the codex post-impl fix loop converged (a resumed codex pass reporting no new material findings, quoted in the transcript); the PR exists and is marked ready with all four required Status checks green (gh pr view output in the transcript); bun run test and bun run lint:actions both exit 0 in the transcript. If Phase 4 blocks on the FPC redeploy (owner-gated Sepolia key), the goal is instead: Phases 1–3 ✓, the blocked state logged in lessons/phase-4.md, and the owner ping surfaced in the transcript.
```

**Alternative: `/loop 15m`**

```
/loop 15m Drive implementations-plan/aztec-5.2.0-2026-08-18 forward. Never idle waiting for my input. Each firing: (1) Reality check: read plan.md + lessons/ (authoritative), git status, git log --oneline -5; if the PR exists, gh pr view --json statusCheckRollup (no --watch). (2) Waiting on CI is fine — confirm it progresses (gh run watch <id> up to 10 min); use waits to review the diff or prep the next phase. (3) No task in hand? Take the next pending plan.md step. After each meaningful edit run bun run test for the touched surface; commit and push. (4) Stuck or facing a decision I'd normally make? /codex xhigh, reach a defensible call, act, log consult+verdict in lessons/phase-N.md. Hard limits stay hard: never merge, never publish, never touch the Sepolia key (FPC redeploy is owner-only — log blocked and continue elsewhere), never expand scope. (5) Same step failed 5 times? Stop retrying, reassess with codex. (6) Phase green means ITS VALIDATION GATE in plan.md passed — run it, paste the result, mark ✓ in plan.md, write lessons/phase-N.md, print LESSONS_FILE=implementations-plan/aztec-5.2.0-2026-08-18/lessons/phase-N.md. (7) All phases ✓? Run plan.md's Post-implementation section (code-review max --fix → separate commit → codex xhigh audit with the no-over-engineering rule → resumed-codex fix loop to convergence), mark the PR ready, write the wrap-up report, surface and stop.
```

Use exactly ONE per session — they don't compose.
