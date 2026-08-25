# Plan — Bun 1.3.14 → 1.4 migration + bun-native adoption (rev 3, post dual-audit + final pass)

- **Tier**: `/blueprint mid` (owner-confirmed). Dual audit round 1: codex conditional ×7, fable
  conditional ×4 — every condition addressed in rev 2 (adopted as stated or reshaped where the
  final pass found the translation lossy; the ledger + audit files carry the per-finding
  dispositions). Final fresh-context pass on rev 2: conditional ×6 consolidation gaps — folded
  into THIS rev 3.
- **Success criterion** (owner, 2026-08-25): repo runs Bun 1.4 in CI and locally where empirical
  gates allow; bun-native adoptions land where proportionate; the publish pipeline stops floating
  `latest`.
- **Owner strategy**: attempt-now-fallback-staged; upstream Worker bug filed either way (A3:
  agent drafts, owner files).
- **Validation layers** (owner): full PR CI + release-path exercise (resolved A2: **packaged-e2e
  leg, mandatory for Arc C** — it exercises swap-sdk; serve-static is covered by its OWN contract
  test, packaged-e2e never touches it) + live-testnet smoke under 1.4.
- **eli5_mode**: artifact. Source `eli5.html` here; URL in Seeds at publish.
- **Worktree/branch**: `bun-1-4-migration` / `worktree-bun-1-4-migration`, base `b605075`.
- **Local toolchain reality** (fable cond. 4): this machine's global bun is ALREADY 1.4.0; bun does
  not read `.bun-version` itself (only setup-bun does). Every "under 1.3.14" gate runs with the
  scratch-pinned 1.3.14 binary PATH-prefixed (already provisioned); every "under 1.4" gate with the
  scratch 1.4.0 binary. Each gate names its binary; lessons record `bun --version` output.

---

## Phase 1 ✓ — 1.3-compatible wave — GREEN 2026-08-25, linker RETAINED (outcome i): commits
`65fa9a7`/`3d60271`/`c80cb41`/`f4680a5`; gate `bun run test` exit 0 + `lint:actions` exit 0 +
guards 4/4 under scratch 1.3.14; 4a lockfile-invariant, 4c probe all-resolve, 4f ~4× install
speedup / disk neutral — evidence in lessons/phase-1.md

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
   bun's `--parallel` assigns real per-worker IDs that must survive) first-line in the three
   preloads, comment naming the contract. **Not cosmetic-only (final codex pass)**: the env is
   process-global and other installed packages branch on it (undici; Playwright can reject
   Jest-marked execution) — scope containment: the preloads run ONLY under `bun test` (bunfig
   `[test].preload` + the e2e `--preload` flag), never under `bunx playwright test` or app
   runtimes; Phase 1 includes a consumer inventory (grep installed deps for `JEST_WORKER_ID`
   readers reachable under bun test) recorded in lessons. **Guard test** (fable cond. 4,
   strengthened by the final pass): `scripts/aztec-logger-contract.test.ts` resolves
   `@aztec/foundation`'s pino-logger FROM a declaring workspace (isolated-linker-aware — via
   `Bun.resolveSync` from `packages/sdk`, not a hardcoded node_modules path) and verifies the
   actual `JEST_WORKER_ID → pino.destination(2)` branch semantics, not a bare variable-name grep.
   Sunset (A4): remove env+guard when bun's Worker bug is fixed upstream AND the guard would
   otherwise be the only consumer.
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
      became unnecessary (don't remove in this arc; note for Arc C if confirmed);
   f. **exit rule (final codex pass)**: measure install time AND disk versus the hoisted baseline
      and record both; the 1.4c probe must resolve from the MATERIALIZED SDK's own module context
      (not cwd / direct consumer imports). If resolution needs more than a small script fix, or
      the measured benefits are immaterial, CUT the linker from Arc A — it must never strand the
      urgent publish pin (Arc A's commit order: publish pin first, pin centralization second,
      prophylaxis third, linker LAST and separable).

**Validation gate** (binary: scratch 1.3.14)
- `bun run test` && `bun run lint:actions` && `bun test scripts/bun-pin.test.ts
  scripts/aztec-logger-contract.test.ts` && validations 4a–4f recorded in lessons (4e's
  vite-override finding and 4f's time+disk measurements included).
- Pass — TWO valid outcomes (final pass: gate must match the exit rule):
  (i) linker RETAINED: 4a empty diff, 4c probe resolves every import from the materialized SDK's
  module context, 4f shows material benefit — all recorded; or
  (ii) linker CUT: the cut recorded in the ledger with 4c/4f evidence, and Arc A's preceding
  commits (publish pin, centralization, prophylaxis) still pass the full gate on their own.
  Either way: Arc A PR CI fully green.
- Layers: lint · typecheck · unit · install-topology integration.

## Phase 2 ✓ — Empirical spike — **VERDICT: GO** (2026-08-25): fresh-install gate exit 0 after
catching the undeclared @types/node (fixed, verified in the failing scenario); bb.js WASM leg
10/10 under 1.4 vs testnet; TLS IP-SAN 200; --parallel three-way identical; upstream already
fixed (#40268/#40271, nothing to file) — evidence in lessons/phase-2.md

1. **Fresh-install gate (codex HIGH)**: in a pristine clone/worktree, scratch-1.4.0
   `bun install --frozen-lockfile` — clean install under the 1.4 PM, then full `bun run test`.
   (Phase-1-tree reuse alone would mask install/linker/min-age deltas.)
2. **bb.js Worker question (decisive)**: SDK e2e WASM fallback leg under 1.4 (headless accelerator
   + live testnet — the proven recipe), WITH `JEST_WORKER_ID` set so the pino path is bypassed.
   Green ⇒ bump unblocked. Crash ⇒ NO-GO, valid ONLY when the exact `internal:worker/messaging`
   signature is produced BY THIS bb.js-leg execution and quoted from ITS output in lessons (final
   codex pass: the pino repro trivially produces the same signature — it can never justify NO-GO;
   any other failure is triage, not NO-GO).
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
- A NO-GO passes THIS PHASE only — Phases 3–5 stay unmarked and the plan parks (codex: no
  completion loophole; the seeds encode this).
- Layers: install · integration · e2e live network · runtime semantics.

## Phase 3 — Bump-only wave (gated on GO; deliberately minimal for bisectability)

1. `.bun-version` → `1.4.0`.
2. ONE isolated lockfile-regen commit: `bun install` (scratch 1.4.0), reviewed line-class by
   line-class. Expected: format/metadata churn only — EXCEPT `@types/bun`: **honor min-age (codex
   HIGH — no exemption)**: `@types/bun@1.4.0` published 2026-08-21, eligible ~2026-08-28. Once
   aged: a trailing commit sets the root manifest to `"@types/bun": "^1.4.0"` AND regenerates the
   lock (the manifest edit is the explicit step the final pass asked to be named; same commit as
   the version flip only if the calendar already permits). I6 corrected: intentional movement of
   @types/bun (and transitively bun-types) only.
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
4. **`serve` → Bun.serve dir-serving** (~6-line `serve-static.ts`; delete the devDep). Verified by
   its OWN contract test (final codex pass: packaged-e2e runs the PLAYGROUND's Vite webServer and
   never touches serve-static, and the desktop-UI suite asserts no header semantics — the prior
   parity claim was wrong): new `serve-static.test.ts` asserting index + asset resolution,
   Content-Type, ETag/304 behavior, traversal rejection, loopback-only binding. The desktop-UI
   Playwright suite then proves the swap in situ.
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
pre-parsed `err.data`) AND both test files that exercise `HTTPError` semantics through mocked
fetch responses (they do not construct it directly — final-pass correction).
Design: an INTERNAL response-error contract (private class or discriminated result) replacing
`HTTPError` `instanceof` checks 1:1; `AbortSignal.timeout()` per call; explicit redirect + timeout
parity with ky's `retry:0` behavior. **Classification invariant (final codex pass, security)**:
a non-2xx RESPONSE stays classified as an HTTP error even when its body read stalls, exceeds the
bound, or is malformed (`data`-equivalent becomes undefined; status is ALWAYS preserved) — body
failure must never demote an HTTP response to a network failure, because network-failure
classification is what activates HTTPS→HTTP demotion. Bounded body reads per the F-11 cap
discipline. Adversarial tests for stalled/oversize/malformed non-2xx bodies on BOTH the primary
and the downgrade-retry paths.
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
  `packages/accelerator/scripts/serve-static.ts` + `serve-static.test.ts` (its contract test);
  spike fixtures (uncommitted); Arc D's internal error contract inside existing SDK modules (no
  new public module).
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
- F9: the accelerator leaf cert carries `IpAddress(127.0.0.1)`/`IpAddress(::1)`/
  `DnsName(localhost)` SANs (`certs.rs:227-229`).
- F10: no `github:`/`tarball:`/`file:`/`link:` dep specifiers and no `trustedDependencies` keys
  anywhere in the tree.
- F11: sdk unit tests mock `fetch` entirely; the e2e dual-probe is the only *self-signed/IP*
  bun-TLS client path (GitHub/registry fetches are ordinary hostname TLS).
- F12: bb.js's node factory constructs a real `worker_threads.Worker`; unit tests never reach it
  (`createChonkProof` spyOn-mocked); the sdk e2e WASM leg does.
- F13 (new, codex): setup-bun does no binary provenance verification.
- F14 (new, both audits): ky's `HTTPError` is load-bearing in prover error taxonomy + errors.ts
  `err.data` — the removal is Arc-D-scale, not a transport-file swap.
- F15 (new, codex): `bun test --parallel` implies `--isolate`; `--no-isolate` is the opt-out;
  workers receive distinct IDs via `BUN_TEST_WORKER_ID` (and a Jest-compat `JEST_WORKER_ID`,
  which is why the prophylaxis uses `??=`).
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
- (A2 moved to ledger — RESOLVED: packaged-e2e mandatory for Arc C as the swap-sdk release-path
  exercise; serve-static gets its OWN contract test since packaged-e2e never touches it — the
  earlier "both live in that path" rationale was wrong, per the final pass.)
- A3: upstream issue — agent drafts repro+body in lessons; OWNER files under their account.
  Confirm.
- A4 (new): the JEST_WORKER_ID sunset criteria (remove when upstream Worker fix ships and the
  branch-guard would be the only consumer) — confirm this disposition.

## Decision ledger

- Strategy / pinning / validation: owner at clarify (rev 1 entries stand).
- Owner reversals: ky IN (now Arc D, full-contract scope), serve IN, `--parallel` empirical.
- Dual audit round 1 (2026-08-25): codex conditional ×7, fable conditional ×4 — every condition
  addressed (two reshaped where the final pass found rev 2's translation lossy):
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
- Final fresh-context pass (rev 2 → rev 3, session `codex-H5tnT7Tl`): conditional ×6, all folded —
  NO-GO bound to the bb.js-leg execution specifically (the pino repro can never justify it);
  Arc D classification invariant (unreadable bounded error bodies keep HTTP status — never demote
  to network failure — + adversarial tests both paths; "tests construct HTTPError" corrected to
  mocked-fetch exercise); serve-static gets its own contract test (packaged-e2e never touches it —
  A2 rationale corrected); JEST_WORKER_ID consumer inventory + branch-semantics guard (not a name
  grep), resolved isolated-linker-aware; linker exit rule (measure, cut-on-failure, never strand
  the publish pin; Arc A commit order fixed); ledger/assumption hygiene (F9–F12 explicit,
  F15→BUN_TEST_WORKER_ID, A2→ledger, @types/bun manifest step named, Arc D branches from B).
- Phase-2 GO/NO-GO: **GO** (2026-08-25) — bb.js WASM-fallback leg 10/10 under scratch 1.4.0
  against live testnet (the decisive execution; no `internal:worker/messaging` signature
  anywhere in its output); fresh-install + TLS + parallel evidence in lessons/phase-2.md. Bonus
  catch: undeclared `@types/node` (hoisting-masked) fixed ahead of Phase 3.
- @types/bun: still inside min-age until ~2026-08-28 — trailing commit after aging (Phase 3.2).

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
- **Arc D** = Phase 5 (ky removal, standalone published-SDK PR) — **branches from Arc B, not C**
  (final codex pass: D has no dependency on C's optional tooling; stacking it on C would couple
  the SDK change to adoption churn). `gh stack` topology: A → B → {C, D} as siblings on B.

## Seeds (DRAFT — finalized after approval)

Artifact URL: https://claude.ai/code/artifact/54480f30-f3f7-4982-8ac6-1417a5f33bdb
(source: `eli5.html` in this dir — redeploying the same file updates the same URL)

**Recommended: `/goal`** (NO-GO honesty per codex: Phases 3–5 stay unmarked on NO-GO)

```
/goal EITHER (GO path) all five phases marked ✓ in implementations-plan/bun-1-4-migration/plan.md with each gate's output quoted in the transcript, LESSONS_FILE lines printed per phase, Arcs A–D PRs green on all required Status checks with /code-review max --fix applied and the per-arc codex loops converged (resumed passes quoting no new material findings), the packaged-e2e leg and live-testnet smoke green for Arc C, and bun run test + bun run lint:actions exit 0 under the scratch-pinned .bun-version binary; OR (NO-GO path) Phases 1–2 ✓ only WITH Phase 2's gate output quoted in the transcript, the ledger's NO-GO entry quoting the exact internal:worker/messaging crash signature AS PRODUCED BY the Phase-2.2 bb.js WASM-fallback execution run with JEST_WORKER_ID set (pino bypassed) — a pino-path or any other failure NEVER qualifies — Arc A PR green and ready, Arcs B–D explicitly parked in plan.md status, and the upstream-issue repro drafted in lessons/phase-2.md. Phases 3–5 must NEVER be marked ✓ on the NO-GO path.
```

**Alternative: `/loop 15m`** — blueprint template parameterized for this plan; hard limits: never
merge/publish; never file the upstream issue (draft only — A3); never flip `.bun-version` past
what the ledger's recorded spike verdict allows; never add min-age exemptions (@types/bun waits).

Use exactly ONE per session — they don't compose.

## APPROVAL (owner, 2026-08-25)

**Conditional approve** — one modification: A3 FLIPPED — the agent checks oven-sh/bun for an
existing report and, if absent, FILES the upstream issue directly (owner's gh auth), rather than
draft-only. A1/A2/A4 confirmed as written. Seeds finalized against this scope (the /loop hard
limit "never file the issue" is replaced by "file once, after a duplicate check; never re-file").

**A3 RESOLVED (2026-08-25, same day)**: duplicate check found **oven-sh/bun#40268** — same crash,
CLOSED/COMPLETED, fixed by PR #40271 (`b746c078`: worker_threads binds intrinsics, not mutable
globals). Nothing to file. True trigger = happy-dom's global MessagePort replacement (pino was
merely the first Worker constructor after registration); full corrected diagnosis + reduction
matrix in lessons/phase-2.md. NO-GO tail risk shrinks (e2e has no happy-dom; plain Worker passes
under 1.4.0); the prophylaxis stays for the 1.4.0 window (verified in-tree: playground 8/8 with
the env) and A4's sunset trigger is now concrete (a bun release containing `b746c078`). Phase 3
may target that release directly if shipped by then.

## OWNER OVERRIDE (2026-08-25, mid-implementation)

`/code-review max` is too token-expensive on this account (one full run was lost to a credits
outage; the rerun stalled twice and re-fanned a line-by-line angle over a small diff). For Arcs
A+B the max review ran and its confirmed findings were applied (`026a724`); for REMAINING arcs
(C, D) the per-arc protocol is amended: inline self-review by the implementing session + the
codex xhigh fix loop (ChatGPT account) replace the max-effort multi-agent review. A Claude-side
multi-agent review runs only on explicit owner request.

## Delivery deviation (2026-08-25, recorded)

GO path collapsed Arc A/B's independent-merge rationale (the split existed for the NO-GO
contingency; the post-impl fix commits interleave both arcs' surfaces). Delivery is now:
**PR-1 = Arcs A+B consolidated** (per-concern revertability preserved at commit granularity),
**PR-2 = Arc C** stacked on it, **PR-3 = Arc D** branched from PR-1's head as a sibling. The
four-arc CONTENT is unchanged — only PR packaging consolidated.
