# Plan — Bun 1.3.14 → 1.4 migration + bun-native adoption (rev 2, post dual-audit)

- **Tier**: `/blueprint mid` (owner-confirmed). Dual audit complete: codex conditional-approve ×7
  conditions, fable conditional-approve ×4 — ALL adopted in this revision (see ledger +
  audit-codex.md / audit-fable.md).
- **Success criterion** (owner, 2026-08-25): repo runs Bun 1.4 in CI and locally where empirical
  gates allow; bun-native adoptions land where proportionate; the publish pipeline stops floating
  `latest`.
- **Owner strategy**: attempt-now-fallback-staged; upstream Worker bug filed either way (A3:
  agent drafts, owner files).
- **Validation layers** (owner): full PR CI + release-path exercise (resolved A2: **packaged-e2e
  leg, mandatory for Arc C** — it exercises swap-sdk + serve-static together) + live-testnet smoke
  under 1.4.
- **eli5_mode**: artifact. Source `eli5.html` here; URL in Seeds at publish.
- **Worktree/branch**: `bun-1-4-migration` / `worktree-bun-1-4-migration`, base `b605075`.
- **Local toolchain reality** (fable cond. 4): this machine's global bun is ALREADY 1.4.0; bun does
  not read `.bun-version` itself (only setup-bun does). Every "under 1.3.14" gate runs with the
  scratch-pinned 1.3.14 binary PATH-prefixed (already provisioned); every "under 1.4" gate with the
  scratch 1.4.0 binary. Each gate names its binary; lessons record `bun --version` output.

---

## Phase 1 — 1.3-compatible wave (codex: NOT "no behavior change" — logger destination in tests
and node_modules topology DO change; compatible-with-1.3.14 is the honest claim)

1. **Pin `_publish-sdk.yml`** (`latest` → the repo pin) — the publish pipeline builds the
   published artifact under a floating bun TODAY; highest-priority single edit.
2. **Centralize the pin**: `.bun-version` = `1.3.14`; all 23 setup-bun sites →
   `bun-version-file: .bun-version` (input name VERIFIED at the pinned SHA — promoted to fact F7).
   New `scripts/bun-pin.test.ts` guard: every setup-bun use carries `bun-version-file`, no inline
   `bun-version:`, `.bun-version` is exact semver.
   **Honest supply-chain note (codex HIGH)**: setup-bun does NOT verify binary provenance (no
   digest/signature check — it downloads over TLS and executes). `.bun-version` stops DRIFT and
   removes the floating-`latest` publish exposure; it does not add provenance. Accepted residual,
   unchanged from today, recorded in Security.
3. **Worker-crash prophylaxis**: `process.env.JEST_WORKER_ID ??= "1";` (codex: `??=`, NOT `=` —
   bun's `--parallel` assigns real distinct worker IDs that must survive) first-line in the three
   preloads, comment naming the contract. **Plus a guard test** (fable cond. 4):
   `scripts/aztec-logger-contract.test.ts` (~15 lines, sibling of `bunfig-aztec-excludes.test.ts`)
   greps the INSTALLED `@aztec/foundation` pino-logger for the `JEST_WORKER_ID` branch — the
   tripwire for aztec renaming/removing it on a future @aztec bump. Sunset: remove env+guard when
   bun's Worker bug is fixed upstream AND the guard would otherwise be the only consumer.
4. **Isolated linker** (`linker = "isolated"` in root bunfig), with CORRECTED claims (codex MED /
   fable LOW): the download cache (`~/.bun/install/cache`, already CI-cached) is a separate thing
   from the per-project materialization store (`node_modules/.bun`); the advertised 7× global-store
   numbers involve mechanisms we are NOT enabling. Our claimed benefits: measured install-time
   delta (record it, don't promise it) + per-worktree disk reduction. Validations:
   a. `bun install` (scratch 1.3.14) → `git diff bun.lock` EMPTY (any diff = stop);
   b. full local gate (scratch 1.3.14);
   c. **the release-gate resolution proof (fable HIGH, rewritten)**: under the isolated-linker
      tree, run `.github/scripts/packaged-e2e-swap-sdk.sh`'s swap against a freshly packed SDK
      tarball, then from the repo root execute a node/bun resolution probe that imports
      `@alejoamiras/aztec-accelerator` AND its transitive `@aztec/*`/`ky` deps FROM the swapped-in
      materialized dir — the hoist the script's comment depends on is exactly what the linker
      removes; this probe is the evidence. (`sdk-tarball-consumer.sh` is NOT linker evidence — it
      npm-installs in a scratch dir.)
   d. multi-worktree contention: two concurrent scratch-worktree installs, both green;
   e. cleanup check: `packages/playground/tsconfig.e2e.json`'s vite path override — verify it
      became unnecessary (don't remove in this arc; note for Arc C if confirmed).

**Validation gate** (binary: scratch 1.3.14)
- `bun run test` && `bun run lint:actions` && `bun test scripts/bun-pin.test.ts
  scripts/aztec-logger-contract.test.ts` && validations 4a–4d recorded in lessons.
- Pass: all exit 0; 4a empty diff; 4c probe resolves every import; Arc A PR CI fully green.
- Layers: lint · typecheck · unit · install-topology integration.

## Phase 2 — Empirical spike (scratch 1.4.0; evidence + upstream report; gates Phase 3)

1. **Fresh-install gate (codex HIGH)**: in a pristine clone/worktree, scratch-1.4.0
   `bun install --frozen-lockfile` — clean install under the 1.4 PM, then full `bun run test`.
   (Phase-1-tree reuse alone would mask install/linker/min-age deltas.)
2. **bb.js Worker question (decisive)**: SDK e2e WASM fallback leg under 1.4 (headless accelerator
   + live testnet — the proven recipe). Green ⇒ bump unblocked. Crash ⇒ NO-GO, valid ONLY with the
   exact `internal:worker/messaging` signature quoted in lessons (fable: a generic failure is
   triage, not NO-GO).
3. **TLS semantics**: openssl IP-SAN self-signed cert + `Bun.serve({tls})` under 1.4 +
   `fetch("https://127.0.0.1:<port>", {tls:{ca}})` — the semantics our accelerator leaf depends on.
4. **`--parallel` semantics (premise corrected — it IMPLIES `--isolate`; opt-out is
   `--no-isolate`)**: per suite, three-way run: baseline / `--parallel` / `--parallel
   --no-isolate`. Canary test asserts per-worker preload effects (addEqualityTesters patched,
   JEST_WORKER_ID present, distinct BUN_TEST_WORKER_ID values under parallel) and, for bare
   `--parallel`, whether per-file isolation breaks the happy-dom/global-mutation model. Adopt-list
   per suite per mode = whatever is byte-identical to baseline.
5. **Upstream issue**: minimal repro drafted into `lessons/phase-2.md` (pino transport under
   `bun test` → the messaging crash); owner files (A3).

**Validation gate**
- All five items' outputs recorded in lessons; GO/NO-GO in the ledger with evidence quotes.
- A NO-GO passes THIS PHASE only — Phases 3–4 stay unmarked and the plan parks (codex: no
  completion loophole; the seeds encode this).
- Layers: install · integration · e2e live network · runtime semantics.

## Phase 3 — Bump-only wave (gated on GO; deliberately minimal for bisectability)

1. `.bun-version` → `1.4.0`.
2. ONE isolated lockfile-regen commit: `bun install` (scratch 1.4.0), reviewed line-class by
   line-class. Expected: format/metadata churn only — EXCEPT `@types/bun`: **honor min-age (codex
   HIGH — no exemption)**: `@types/bun@1.4.0` published 2026-08-21, eligible ~2026-08-28; bump it
   in a trailing commit once aged (or same commit if the calendar has moved past eligibility).
   I6 corrected accordingly: intentional movement of @types/bun (and transitively bun-types) only.
3. Comment hygiene: bunfig empirical notes re-stamped as re-verified on 1.4.0 (F-E3/F-E4).
4. NOTHING ELSE — retries, parallelization, and adoptions all move AFTER a green bump (codex MED:
   a retry added in the bump commit can mask a runtime regression the bump itself caused).

**Validation gate** (binary: scratch 1.4.0 locally; CI runs 1.4.0 via `.bun-version`)
- Full local gate && `lint:actions`; Arc B PR CI fully green; lockfile commit review recorded.
- Layers: full CI.

## Phase 4 — Post-bump tooling adoptions (each its own commit; gated on Phase 3 green)

1. `bun run --parallel` for the lint chain (3 live CI jobs) + local `test:unit`/`test:typecheck`
   chains; playground's 3-tsc chain kept parallel only if one CI timing run shows no regression.
2. `bun test` parallel flags per the Phase-2 adopt-list ONLY (mode included — likely
   `--parallel --no-isolate` if bare-parallel's isolation breaks preload semantics).
3. `{retry: 1}` on exactly the four live-network surfaces; NEVER `release-contract.test.ts`.
4. **`serve` → Bun.serve dir-serving** (~6-line `serve-static.ts`; delete the devDep) — verified by
   the desktop-UI Playwright suite + the packaged-e2e leg (ETag/Content-Type parity — the recon
   check codex flagged as dropped).
5. **`--no-orphans`** on the two Playwright webServer commands.
6. CUT (both audits): the copy-bb `Bun.Archive` swap — Archive exposes no decompressed-output cap
   and validates after write; System32-tar path works, is Windows-only (least-covered platform),
   and the win was ~6 deleted lines. Ledgered as future-work when Archive matures.
   `download-bb.ts` stays KEEP per F-007 (unchanged from rev 1).

**Validation gate**
- Full local gate && Arc C PR CI green && **packaged-e2e leg** (mandatory — A2 resolved) &&
  live-testnet smoke under 1.4 (headless accelerator + `test:e2e:smoke` + sdk `test:e2e`).
- Layers: full CI · release-path integration · e2e live network.

## Phase 5 / Arc D — ky removal from the published SDK (standalone; codex CRITICAL rescope;
owner-requested feature, full contract honored)

Scope (fable cond. 1 + codex): `accelerator-transport.ts` AND `accelerator-prover.ts` (lines ~420+:
`instanceof HTTPError` gates network-vs-HTTP classification, HTTPS downgrade control, status
fallback — the F14 degrade-vs-throw taxonomy) AND `errors.ts` (`parseServerError` consumes ky's
pre-parsed `err.data`) AND both test files that construct `HTTPError`.
Design: an INTERNAL response-error contract (private class or discriminated result) replacing
`HTTPError` `instanceof` checks 1:1; bounded error-body reads (the F-11 body-cap discipline);
explicit redirect + timeout parity with ky's `retry:0` behavior; `AbortSignal.timeout()` per call.
Freeze the F14 semantics as the behavior net: prover + transport + errors suites must pass with
assertions updated ONLY where they name ky's class; `public-contract.test.ts` untouched (zero
public-API change). Remove `ky` from published dependencies.

**Validation gate**
- SDK suites green (scratch 1.4.0) with F14 taxonomy assertions intact; `public-contract.test.ts`
  untouched; full Arc D PR CI green; sdk e2e re-run (the taxonomy governs live fallback behavior).
- Layers: unit · integration · e2e.

---

## Competing outline (audited; both auditors chose phased)

"Bump-first monolith" — one PR flipping everything. Rejected: strands on the bb.js unknown;
destroys bisectability; the ky rewrite (now Arc D-scale) must not share a diff with 23 mechanical
pin edits; per-adoption revert requires separate commits/PRs. (Codex additionally pushed the
phased shape FURTHER apart — bump-only isolated from adoptions — adopted as Phases 3/4.)

## Architecture & Implementation

- **Reuse**: guard-test family (bun-pin + logger-contract join action-pins/bunfig-excludes); the
  5.2.0 lockfile-commit discipline; aztec's own JEST_WORKER_ID branch; the headless+testnet smoke
  recipe; existing preloads.
- **New files**: `.bun-version`; `scripts/bun-pin.test.ts`; `scripts/aztec-logger-contract.test.ts`;
  `packages/accelerator/scripts/serve-static.ts`; spike fixtures (uncommitted); Arc D's internal
  error contract inside existing SDK modules (no new public module).
- **Modified**: 21 workflows + 2 composites; root bunfig; 3 preloads; root package.json scripts;
  playwright configs ×2; accelerator package.json (−serve); Arc D: sdk transport/prover/errors +
  tests + package.json (−ky).
- **Non-obvious mechanics**: import-time pino crash ⇒ workaround must precede any @aztec import
  (preloads run first); `--parallel` = worker processes × implied per-file isolation (three-way
  spike); isolated linker kills the root hoist the swap-sdk script's comment relies on (Phase 1.4c
  probe is the proof); ky's `HTTPError.data` pre-parse contract (Arc D's internal contract
  replicates it with bounded reads).
- **Trade-offs**: phased-over-monolith; exact `.bun-version` over floating; env-contract over
  patching aztec (guard-tested); bump-only isolation over convenience batching; retries/parallel
  after the bump, never with it.

## Security & Adversarial Considerations

- **Threat model**: the toolchain is the surface — runtime binary, package materialization,
  published-SDK dep set (shrinks by one in Arc D; grows by zero).
- **Supply chain (corrected)**: `.bun-version` eliminates drift and the floating-`latest` publish
  exposure. It does NOT add provenance: setup-bun performs no digest/signature verification of the
  bun binary it downloads and executes (verified in the pinned action source) — an accepted
  residual identical to today's posture, now stated instead of wished away. The 7-day min-age
  regime is UNCHANGED and applies to the migration's own deps: `@types/bun@1.4.0` waits out its
  window — no exemption.
- **Isolated linker**: per-project store under `node_modules/.bun`; the shared download cache's
  hardlink mutation surface on a multi-agent machine predates this change and is unchanged by it
  (stated, not solved). No privilege deltas.
- **Logger suppression**: JEST_WORKER_ID silences transports in TESTS only; failure loudness
  preserved (sync fd destination still writes); guard test tripwires the upstream contract.
- **F-007/F-008 discipline**: download-bb untouched; copy-bb swap CUT — no security-audited
  extraction path is replaced anywhere in this plan.
- **TLS**: 1.4 client-side tightening only; the one real path spike-verified; no
  rejectUnauthorized loosening anywhere.
- **Upstream report**: public minimal repro, no repo internals; filed by the owner.

## Assumptions

**Facts (verified)**
- F1–F4: empirical 1.4.0 results (recon F-E1..E4).
- F5: JEST_WORKER_ID ⇒ sync non-worker destination (installed pino-logger read; guard-tested from
  Phase 1).
- F6: 23 setup-bun sites; exactly one floating (`_publish-sdk.yml:65`).
- F7 (promoted from I3 — fable verified at the pinned SHA): setup-bun supports
  `bun-version-file`, resolves from workspace root, trims a bare version line.
- F8 (corrected): download cache and materialization store are distinct; isolated linker's store
  is per-project (`node_modules/.bun`); CI cache paths unchanged; performance claims to be
  MEASURED, not assumed.
- F9–F12: unchanged from rev 1 (cert SANs; no special dep specifiers; sdk unit fetch fully mocked
  — F11 reworded: the e2e dual-probe is the only *self-signed/IP* bun-TLS client path; GitHub
  fetches are ordinary hostname TLS).
- F13 (new, codex): setup-bun does no binary provenance verification.
- F14 (new, both audits): ky's `HTTPError` is load-bearing in prover error taxonomy + errors.ts
  `err.data` — the removal is Arc-D-scale, not a transport-file swap.
- F15 (new, codex): `bun test --parallel` implies `--isolate`; `--no-isolate` is the opt-out;
  workers receive distinct IDs.
- F16 (new, codex): `@types/bun@1.4.0` published 2026-08-21 — inside the 7-day window until
  ~2026-08-28.

**Inferences (attackable)**
- I1: the 1.4.0 Worker bug MAY be pattern-specific; deliberately not relied on — Phase 2 evidence
  gates Phase 3.
- I2 (reframed): preloads run per worker PROCESS under `--parallel`; per-FILE isolation semantics
  under implied `--isolate` are unknown — the three-way spike measures both; nothing is adopted on
  this inference alone.
- I4: linker flip yields zero lockfile diff and unbroken resolution INCLUDING the swap-sdk hoist
  question — Phase 1.4a/4c are the proofs, with 4c redesigned to test the actual risk.
- I5 (narrowed): ky's replaceable surface in the TRANSPORT is timeout/non-ok/no-retry; the
  prover/errors surface is a contract port, not a removal — Arc D's design section is the spec.
- I6 (corrected): first 1.4 install = format-only churn EXCEPT intentional @types/bun movement
  post-aging.

**Asks (owner — none silently assumed)**
- A1: NO-GO fallback pre-agreed (strategy answer); confirm at gate: Arc A merges alone, B–D park,
  plan status "parked-upstream" until a bun patch clears the spike.
- A2 (RESOLVED default per audits): packaged-e2e leg is the mandatory release-path exercise for
  Arc C; build-test-bundle alone is insufficient because serve-static + swap-sdk both live in that
  path. Confirm.
- A3: upstream issue — agent drafts repro+body in lessons; OWNER files under their account.
  Confirm.
- A4 (new): the JEST_WORKER_ID sunset criteria (remove when upstream Worker fix ships and the
  branch-guard would be the only consumer) — confirm this disposition.

## Decision ledger

- Strategy / pinning / validation: owner at clarify (rev 1 entries stand).
- Owner reversals: ky IN (now Arc D, full-contract scope), serve IN, `--parallel` empirical.
- Dual audit round 1 (2026-08-25): codex conditional ×7, fable conditional ×4 — ALL adopted:
  ky split to Arc D with F14 contract scope (codex CRITICAL, fable HIGH); linker validation
  redesigned onto the real hoist risk (fable HIGH); `--parallel implies --isolate` premise
  corrected + `??=` + three-way spike (codex HIGH, fable MED); supply-chain claim corrected — no
  setup-bun provenance (codex HIGH); Phase 1 renamed 1.3-compatible, behavior deltas named (codex
  HIGH); fresh 1.4 frozen-install spike gate (codex HIGH); @types/bun min-age honored, no
  exemption (codex HIGH); NO-GO completion loophole closed in seeds + gates (codex HIGH, fable
  LOW); JEST_WORKER_ID guard test + sunset (fable MED, codex MED); retries/parallel moved
  post-bump (codex MED); copy-bb Archive CUT (codex MED, fable LOW-MED); F8/F11 corrections;
  tsconfig vite-override check restored (codex).
- Rejected in audits, upheld: monolith outline (both); pulling any out-of-scope item back in
  (fable: "nothing deserves pulling back in").
- Phase-2 GO/NO-GO: pending evidence.

## Post-implementation (self-contained)

1. `/code-review max --fix` per arc → skim → separate commits.
2. Codex post-impl audit per arc (`/codex xhigh`): net arc diff + code-review summary + plan +
   ledger + adversarial ask + verbatim no-over-engineering rule ("Report bugs and small, targeted
   improvements only. Do not propose speculative abstractions, extra configuration surface, new
   layers, or rewrites — the smallest change that fixes each real problem. If code works and is
   clear, leave it alone.") + comment-quality per global policy.
3. Fix loop: verify → apply → commit → lessons → resume same session → converge (3+ rounds
   churning → stop, surface).
4. Delivery below; ready-flip only post-convergence per arc.
5. On merges: index completed marker; suggest `agent-worktree done bun-1-4-migration`.

## Delivery

**Four arcs, stacked (`gh stack`), PRs opened after per-arc loop convergence:**
- **Arc A** = Phase 1 (1.3-compatible wave) — mergeable regardless of spike.
- **Arc B** = Phases 2–3 (spike evidence + bump-only) — exists on GO; on NO-GO: A merges alone,
  B–D park with ledger + lessons.
- **Arc C** = Phase 4 (post-bump tooling: parallel, retry, serve, no-orphans) — packaged-e2e
  mandatory in its gate.
- **Arc D** = Phase 5 (ky removal, standalone published-SDK PR — reviewable in one sitting on its
  own).

## Seeds (DRAFT — finalized after approval)

Artifact URL: (recorded at publish)

**Recommended: `/goal`** (NO-GO honesty per codex: Phases 3–5 stay unmarked on NO-GO)

```
/goal EITHER (GO path) all five phases marked ✓ in implementations-plan/bun-1-4-migration/plan.md with each gate's output quoted in the transcript, LESSONS_FILE lines printed per phase, Arcs A–D PRs green on all required Status checks with /code-review max --fix applied and the per-arc codex loops converged (resumed passes quoting no new material findings), the packaged-e2e leg and live-testnet smoke green for Arc C, and bun run test + bun run lint:actions exit 0 under the scratch-pinned .bun-version binary; OR (NO-GO path) Phases 1–2 ✓ only, the ledger's NO-GO entry quoting the exact internal:worker/messaging crash signature, Arc A PR green and ready, Arcs B–D explicitly parked in plan.md status, and the upstream-issue repro drafted in lessons/phase-2.md. Phases 3–5 must NEVER be marked ✓ on the NO-GO path.
```

**Alternative: `/loop 15m`** — blueprint template parameterized for this plan; hard limits: never
merge/publish; never file the upstream issue (draft only — A3); never flip `.bun-version` past
what the ledger's recorded spike verdict allows; never add min-age exemptions (@types/bun waits).

Use exactly ONE per session — they don't compose.
