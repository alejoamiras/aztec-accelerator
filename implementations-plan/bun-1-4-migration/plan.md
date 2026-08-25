# Plan — Bun 1.3.14 → 1.4 migration + bun-native adoption

- **Tier**: `/blueprint mid` (owner-confirmed; rubric: blast radius HIGH-ish — all CI + the runtime
  under every script/test; external coupling HIGH — an upstream Bun Worker bug and aztec-internal
  logger behavior; everything else LOW-MOD).
- **Success criterion** (owner, 2026-08-25): repo runs Bun 1.4 in CI and locally where the empirical
  gates allow; bun-native adoptions land where proportionate ("never a 500-line implementation just
  to be bun-native"); publish pipeline stops floating `latest`.
- **Owner strategy**: attempt-now-fallback-staged — the Phase-2 spike decides whether the bump wave
  lands now or holds behind an upstream fix; the safe wave lands regardless. File the upstream bun
  Worker bug either way.
- **Owner reversals folded in**: ky → raw fetch (published-SDK dep removal; transport tests are the
  net), serve → Bun.serve dir-serving (1.4 collapsed the cost), `bun test --parallel` tried
  empirically (bare --parallel only; --isolate stays out).
- **Validation layers** (owner): full PR CI + a release-path exercise + a local live-testnet smoke
  under 1.4.
- **eli5_mode**: artifact. Source `eli5.html` in this dir; URL recorded in Seeds once published.
- **Worktree/branch**: `bun-1-4-migration` / `worktree-bun-1-4-migration`, base `b605075`.
- Recon: `recon.md` (three Sonnet agents + empirical 1.4.0 tests). Audits: `audit-codex.md`,
  `audit-fable.md`.

---

## Phase 1 — Safe wave (lands at 1.3.14; no behavior change on today's toolchain)

1. **Pin `_publish-sdk.yml`** (`bun-version: latest` → the repo pin) — the one floating site; the
   publish pipeline is exposed to 1.4.0 TODAY without this.
2. **Centralize the bun pin**: create `.bun-version` containing `1.3.14`; convert all 23
   `setup-bun` sites (21 workflow + 2 composite-action) to `bun-version-file: .bun-version`.
   Extend the guard-test family with `scripts/bun-pin.test.ts`: every `setup-bun` use carries
   `bun-version-file` (no inline `bun-version:` reintroduction), `.bun-version` parses as exact
   semver.
3. **Worker-crash prophylaxis**: `process.env.JEST_WORKER_ID = "1";` as the first line of
   `packages/sdk/src/test-setup.ts`, `packages/sdk/e2e/e2e-setup.ts`,
   `packages/playground/src/happydom.ts`, each with a one-sentence comment naming the contract
   (aztec's logger takes its sync non-worker destination under this env; required under bun ≥1.4.0
   where the transport worker crashes, harmless under 1.3.14). Verify under 1.3.14: full suite
   green, log output still present in failures.
4. **Isolated linker**: add `linker = "isolated"` to root bunfig `[install]` with a comment (global
   store under `~/.bun/install/cache/` — already covered by every CI cache step; opt-in since
   1.3.14). Validations IN this phase: (a) `bun install` then `git diff bun.lock` must be EMPTY
   (linker changes materialization, not resolution — any lock diff is a stop); (b) full local gate;
   (c) `bash scripts/sdk-tarball-consumer.sh` against a fresh pack (its find is glob-based, prove
   it); (d) `.github/scripts/packaged-e2e-swap-sdk.sh` dry parity — run its rm/mkdir/extract dance
   against an isolated-linker node_modules copy and assert resolution still picks the materialized
   dir; (e) multi-worktree contention: two concurrent `bun install`s in scratch worktrees, both
   exit 0, both suites green.

**Validation gate**
- Commands: `bun run test` (under 1.3.14) && `bun run lint:actions` && `bun test
  scripts/bun-pin.test.ts` && the four linker validations above.
- Pass: all exit 0; empty lockfile diff at step 4a; PR CI fully green on the arc PR.
- Layers: lint · typecheck · unit · install-topology integration.

## Phase 2 — Empirical spike (1.4.0 locally; produces evidence + upstream report, gates Phase 3)

Run everything with the scratch 1.4.0 binary against the Phase-1 tree:

1. **Suites under 1.4**: full `bun run test` — expect green now (JEST_WORKER_ID bypasses pino;
   F-E2 recon). Any residual crash = new finding, triage.
2. **The bb.js Worker question (decisive)**: run the SDK e2e WASM fallback leg under 1.4 —
   headless accelerator + live testnet (the proven 5.2.0-cycle recipe) or local sandbox;
   `proving.test.ts`'s WASM deploy path constructs bb.js's node-factory `new Worker`. Green ⇒ the
   1.4 Worker bug is pattern-specific (thread-stream's port dance) and the bump is unblocked.
   Crash ⇒ NO-GO for the bump wave (fallback: hold Phases 3–4's bump-dependent items, keep the
   branch, revisit on the next bun patch).
3. **TLS semantics**: mint a throwaway self-signed cert WITH IP SANs (openssl, scratch dir), serve
   via `Bun.serve({tls})` under 1.4, `fetch("https://127.0.0.1:<port>/…", {tls:{ca}})` — proves the
   IP-SAN + servername-from-host path our accelerator leaf (certs.rs SANs) depends on. Also run the
   real dual-probe if the desktop app is buildable here (best-effort; the synthetic test is the
   gate).
4. **`bun test --parallel` semantics**: per suite (sdk unit, playground unit, accelerator scripts,
   root scripts): run with and without `--parallel`, byte-compare pass/fail counts and that preload
   effects hold in every worker (a canary test asserting `expect.addEqualityTesters` exists +
   `JEST_WORKER_ID` set). Adopt-list = suites where identical.
5. **File the upstream bun issue**: minimal repro (pino transport under bun test → `port.on is not
   a function` at `internal:worker/messaging`), link it in lessons + the eventual PR body.

**Validation gate**
- Commands: the five items above, each with recorded output in `lessons/phase-2.md`.
- Pass: every item has a recorded verdict; the GO/NO-GO decision for Phase 3 is written into the
  decision ledger with evidence quotes. (A NO-GO is a PASS of this phase — the gate is the
  evidence, not the outcome.)
- Layers: integration · e2e (live network) · runtime semantics.

## Phase 3 — Bump wave (gated on Phase-2 GO)

1. `.bun-version` → `1.4.0`; `@types/bun` → `^1.4.0`; ONE isolated lockfile-regen commit
   (`bun install`, expect format-only churn, zero version movement — reviewed line-class by
   line-class as in the 5.2.0 cycle).
2. Comment hygiene: bunfig's "verified on 1.3.14" empirical notes → re-verified-on-1.4.0 wording
   (wildcard still ignored — F-E3; addEqualityTesters still absent — F-E4).
3. **`bun run --parallel`**: split the root `lint` chain (biome/pkg/shell/rust — sequential inside
   3 live CI jobs) into a parallel invocation; parallelize local `test:unit` and `test:typecheck`
   chains. Keep root `test`'s lint→typecheck→unit fail-fast ordering. Empirically time the
   playground 3-tsc chain on one CI run before keeping it parallel (CPU-contention caveat).
4. **`{retry: 1}`** on exactly the four live-network surfaces (sdk e2e connectivity/proving/
   remote-network network-bound tests; playground live-node block). NEVER on
   `release-contract.test.ts` (it unit-tests retry logic — double-retry masks bugs).
5. `bun test --parallel` flags added per the Phase-2 adopt-list only.

**Validation gate**
- Commands: full local gate under 1.4 && `bun run lint:actions`; arc PR CI fully green (which now
  runs 1.4 everywhere via `.bun-version`).
- Pass: all Status checks green; lockfile-regen commit shows zero version movement; lint-chain
  parallel split shows equal-or-better job wall time on the PR run.
- Layers: full CI (lint · typecheck · unit · integration · e2e sandbox + built app).

## Phase 4 — Native adoptions (each its own commit, independently revertable; gated on Phase 3)

1. **ky → raw fetch** in `packages/sdk` transport: `AbortSignal.timeout()` per call, existing
   `AcceleratorHttpError` for non-ok, no retry regression (ky already ran `retry:0`). Contract:
   ZERO public-API change; `accelerator-transport.test.ts` (behavior net) green untouched except
   where it asserts ky-specific error classes — those assertions may rename, semantics identical;
   preserve the header-vs-body deadline comments' documented behavior. Remove `ky` from SDK deps.
2. **`serve` → Bun.serve dir-serving**: ~6-line `packages/accelerator/scripts/serve-static.ts`
   using 1.4 `routes: {"/*": {dir}}`; swap the playwright webServer command; delete the devDep.
   Verify the packaged flow's expectations (index resolution, Content-Type) via the desktop-UI
   Playwright suite.
3. **`Bun.Archive` in `copy-bb.ts`** (Windows leg): replace the System32-bsdtar execFileSync with
   Bun.Archive extraction; keep the SHA-256-pin-before-extract order and the name/count canary.
   `download-bb.ts` is OUT unless Bun.Archive documents per-entry type inspection + bounded output
   equivalent to the F-007 walk — record the check's outcome in lessons either way; the zlib
   `maxOutputLength` gzip bound stays regardless.
4. **`--no-orphans`** on the two Playwright webServer commands.

**Validation gate**
- Commands: full local gate && arc PR CI green && the owner-selected extras: a release-path
  exercise (`build-test-bundle.yml` dispatch or the packaged-e2e leg — exercises swap-sdk +
  copy-bb + serve-static together) && the live-testnet smoke under 1.4 (headless accelerator +
  `test:e2e:smoke` + sdk `test:e2e`, the 5.2.0-cycle recipe).
- Pass: all green; ky removal shows zero public-API diff (`public-contract.test.ts` untouched).
- Layers: full CI · release-path integration · e2e live network.

---

## Competing outline (mid-tier requirement — the road not taken)

**"Bump-first monolith"**: one branch, one PR — flip `.bun-version` to 1.4.0 immediately, fix
whatever breaks in-place (preload env, retries, parallel flags, adoptions), single review.
- For: one CI gauntlet, no stacked-PR ceremony, fastest wall-clock if everything works.
- Against (why rejected): (a) the bb.js Worker unknown could strand a half-migrated branch —
  attempt-now-fallback-staged requires the safe wave to be landable alone; (b) a 20+-file pin
  refactor + runtime bump + dep swaps in one diff destroys bisectability (the 5.2.0 cycle's
  isolated-commit lockfile discipline exists for this reason); (c) the linker flip and ky swap are
  independently revertable ONLY if separately landed; (d) review quality: the ky transport rewrite
  deserves a reviewer not fatigued by 23 mechanical pin edits.
Both outlines go to the audits; the phased plan is the draft's position.

## Architecture & Implementation

- **Reuse**: guard-test pattern (`action-pins.test.ts` sibling for the bun pin); the 5.2.0 cycle's
  isolated-lockfile-commit discipline; the JEST_WORKER_ID contract aztec's own logger provides; the
  proven headless-accelerator + testnet smoke recipe; existing preload files (no new preload
  surface).
- **New files**: `.bun-version`; `scripts/bun-pin.test.ts`; `packages/accelerator/scripts/
  serve-static.ts` (~6 lines); a scratch openssl TLS fixture (spike-only, not committed).
- **Modified**: 21 workflows + 2 composites (bun-version-file), root bunfig (linker + comment), 3
  preloads (one line each), root package.json scripts (parallel variants), sdk package.json (−ky),
  `accelerator-transport.ts` (fetch port), `copy-bb.ts` (Archive), 2 playwright configs
  (webServer commands), accelerator package.json (−serve).
- **Data/control flow deltas**: transport swaps HTTP client implementation under an unchanged
  interface; copy-bb swaps extractor under an unchanged pin-then-extract order; everything else is
  toolchain config.
- **Non-obvious mechanics**: the pino import-time crash (workaround must run BEFORE any @aztec
  import — first line of preloads, which bun executes before test files); `--parallel` implies
  worker processes each re-running preloads (spike-verified); isolated linker resolution via
  `node_modules/.bun` symlink chains (resolver-based call sites already proven agnostic in recon).
- **Trade-offs**: phased-over-monolith (above); exact-pin `.bun-version` over floating minor;
  JEST_WORKER_ID env contract over patching/vendoring aztec's logger (smallest lever, upstream-owned
  behavior, cosmetic cost in test logs).

## Security & Adversarial Considerations

- **Threat model**: the toolchain itself is the surface — a runtime bump changes the binary every
  build/test/publish runs; the linker changes how packages materialize; dep removals SHRINK the
  published SDK's attack surface (ky out), dep additions: none.
- **Supply chain**: `.bun-version` is exact-pin (no drift; guard-tested); `_publish-sdk.yml`'s
  floating `latest` is eliminated — closing the one place a fresh-of-the-press bun binary could
  touch a PUBLISH unreviewed. Bun binaries in CI come from setup-bun (pinned by SHA per #474) which
  verifies its own downloads. The 7-day min-age + excludes regime is UNCHANGED (F-E3: wildcard
  still unsupported; list + parity test stay).
- **Isolated linker**: same registry artifacts, different layout; the global store is per-user on
  ephemeral runners; locally it's the same trust domain as today's shared cache. No privilege
  changes.
- **F-007 discipline**: download-bb's bounded-decompression + entry-walk stays unless Archive
  proves equivalent guarantees — the plan's default is KEEP for that path.
- **TLS**: 1.4 tightening only makes clients stricter; our one real path is verified in the spike.
  No rejectUnauthorized loosening anywhere, ever.
- **Least privilege**: no token/permission changes in any workflow; `--no-orphans` reduces stray
  process lifetime.
- **Upstream report**: the bun Worker bug gets a public minimal repro — no repo internals included.

## Assumptions

**Facts (verified)**
- F1–F4: the empirical 1.4.0 results (F-E1..E4 in recon.md — latest=1.4.0; worker crash repro'd;
  wildcard excludes still ignored; addEqualityTesters still absent).
- F5: `JEST_WORKER_ID` truthy ⇒ aztec's logger takes `pino.destination(2)`, no worker (read in
  installed `pino-logger.js`; the branch is aztec's own Jest support).
- F6: 23 setup-bun sites, exactly one floating (`_publish-sdk.yml:65` `latest`); no
  engines/packageManager anywhere (agent B, file:line table).
- F7: setup-bun@v2 supports `bun-version-file` (its README/action.yml — verify exact input name at
  implementation, listed as I3).
- F8: isolated linker shipped in 1.3.14; global store under `~/.bun/install/cache/` — inside every
  existing CI cache path (agent B, bun docs fetched live).
- F9: accelerator leaf cert carries IP SANs for 127.0.0.1/::1 + DnsName localhost
  (`certs.rs:227-229`).
- F10: no github:/tarball:/file:/link: deps and no trustedDependencies keys anywhere in the tree.
- F11: sdk unit tests mock fetch entirely; the only real bun-TLS site is the e2e dual-probe.
- F12: bb.js's node factory constructs a real `worker_threads.Worker`; unit tests never reach it
  (createChonkProof spyOn-mocked), sdk e2e WASM leg does.

**Inferences (attackable)**
- I1: The 1.4.0 Worker bug is specific to thread-stream's messaging pattern and bb.js's worker
  will function — PLAUSIBLE ONLY; the spike decides (this inference is deliberately NOT relied on:
  Phase 3 gates on evidence, not I1).
- I2: `--parallel` workers re-run bunfig preloads per process (changelog wording + bun's process
  model); spike-verified with a canary before adoption.
- I3: setup-bun's version-file input is `bun-version-file` and reads a bare semver line (verify at
  implementation against the pinned setup-bun SHA's action.yml).
- I4: The linker flip produces zero lockfile diff and no path breakage (recon found only
  resolver-based call sites + one glob-based find; the swap-sdk script is mechanically
  linker-agnostic) — Phase-1 validations prove it.
- I5: ky's used surface reduces to timeout + non-ok-throw + no-retry (transport file read; the
  test suite is the net if the reading missed a behavior).
- I6: First 1.4 `bun install` produces format-only lockfile churn (zero version movement) — the
  isolated commit + review is the containment either way.

**Asks (owner)**
- A1: The spike's NO-GO fallback is pre-agreed (owner strategy): hold Phase 3–4 bump-dependent
  items, land safe wave, revisit on bun's next patch. Confirm at gate.
- A2: Release-path exercise choice: `build-test-bundle.yml` dispatch (cheaper) vs running the
  packaged-e2e leg (deeper, exercises swap-sdk for real). Plan default: build-test-bundle, escalate
  to packaged-e2e only if Phase 4 touches its surface (it does — serve-static + copy-bb) → default
  flips to packaged-e2e for Arc C. Confirm.
- A3: Filing the upstream bun issue under your GitHub account (I draft, you're the author) vs me
  noting it for you to file. Plan default: I draft the repro + body into lessons; you file.

## Decision ledger

- Strategy: attempt-now-fallback-staged (owner, clarify).
- Pinning: `.bun-version` + bun-version-file, guard-tested (owner, clarify — reverses the
  unadopted C3-era centralization rejection).
- Validation: full PR CI + release-path exercise + live-testnet smoke under 1.4 (owner, clarify).
- Owner reversals vs recon draft: ky IN (published-dep removal + test-suite net), serve IN (1.4
  dir-serving collapsed cost), bare `--parallel` spike-tried (isolate stays out).
- Rejected: bump-first monolith (competing outline, reasons above); --shard/--changed/audit-fix
  automation/WebView/parsers/PTY/--isolate/--no-env-file/prof-tooling (recon §out-of-scope).
- Phase-2 GO/NO-GO: pending evidence (recorded here when decided).

## Post-implementation (self-contained — the implementing session executes THIS)

1. `/code-review max --fix` on the full diff → skim → commit separately (per arc if stacked).
2. Codex post-impl audit (`/codex xhigh`): net diff from `b605075` + code-review commit summary +
   this plan + ledger + adversarial/security ask + verbatim: "Report bugs and small, targeted
   improvements only. Do not propose speculative abstractions, extra configuration surface, new
   layers, or rewrites — the smallest change that fixes each real problem. If code works and is
   clear, leave it alone." Comment-quality per the global policy (no workflow references).
3. Iterative fix loop: verify claims → apply → commit → log consult in lessons/ → RESUME same
   session with fix diff → repeat to convergence (no new material findings). 3+ churning rounds →
   stop, surface.
4. Delivery per Delivery section; PRs marked ready only after loops converge (open drafts are
   allowed during arcs for CI, but ready-flip waits).
5. On merge(s): index.md completed marker; suggest `agent-worktree done bun-1-4-migration`.

## Delivery

**Three arcs, stacked (`gh stack`), PRs opened AFTER per-arc loops converge** (per the updated
global workflow rule):
- **Arc A** = Phase 1 (safe wave @1.3.14) — independently mergeable regardless of spike outcome.
- **Arc B** = Phases 2–3 (spike evidence + the bump) — stacks on A; exists only on spike GO
  (NO-GO: Arc A merges alone, branch parks with lessons).
- **Arc C** = Phase 4 (adoptions) — stacks on B; commits per adoption (ky · serve · archive ·
  no-orphans) for independent revert.
Arc sizing honors reviewability: A is mechanical (pin refactor), B is the judgment diff, C is the
code diff.

## Seeds (DRAFT — finalized after approval)

Artifact URL: (recorded at publish)

**Recommended: `/goal`**

```
/goal All four phases marked ✓ in implementations-plan/bun-1-4-migration/plan.md (phase headers in the file), each ✓ backed by its validation gate output quoted in the transcript, with lessons/phase-N.md printed as LESSONS_FILE lines; the Phase-2 GO/NO-GO recorded in the decision ledger with evidence; on GO: Arcs A+B+C PRs exist, all required Status checks green, /code-review max --fix applied+committed, codex post-impl loop converged (resumed pass with no new material findings quoted); on NO-GO: Arc A PR green+ready, Arcs B/C parked with the ledger entry, upstream bun issue repro drafted in lessons; bun run test and bun run lint:actions exit 0 in the transcript under the .bun-version toolchain.
```

**Alternative: `/loop 15m`** — as the blueprint template, parameterized for this plan
(implementations-plan/bun-1-4-migration; fast layers `bun run test` + `bun run lint:actions`;
hard limits: never merge, never publish, never file the upstream issue as me (draft only — A3),
never bump `.bun-version` past what the spike's recorded verdict allows).

Use exactly ONE per session — they don't compose.
