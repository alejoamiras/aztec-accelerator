# Quality audit — sdk cluster (claude)

- **Run**: 2026-06-10-max-q7e3 · maintainability only (no correctness/security)
- **Scope**: `packages/sdk/src/lib/accelerator-prover.ts` (440), `accelerator-transport.ts` (135), `logger.ts` (3), `index.ts` (9); tests glanced only for prod-change-cost amplifiers
- **Hotspot data** (git, verified): `accelerator-prover.ts` = 14 commits (the #1 SDK hotspot); 8 of those 14 touched the probe/status/protocol cluster specifically (#84 protocol race, #100 health retry, #287 cache/proved-phase, #306 extract `#probeAndParseHealth`, #315 union reshape, #317 fallback dedup, #319 malformed-JSON mapping, #336 transport extraction). `accelerator-transport.ts` = 1 commit (extracted in #336).

Findings: 7 (2 High, 3 Medium, 2 Low).

---

## H-1. Protocol-pin lifecycle is a convention scattered across two files

**Smell**: Temporal Coupling (with a side of Inappropriate Intimacy — the prover micromanages the transport's private state cell through a raw setter).

**Impact**: High. Blast radius: `accelerator-prover.ts` + `accelerator-transport.ts` + every future probe/health/protocol change. Change frequency: this exact cluster is where 8 of the prover's 14 commits landed, including two behavior fixes (#84 "SDK protocol race", #319 malformed-JSON handling) that were fixes *to this lifecycle*.

**Instances**:
- Store + raw setter: `packages/sdk/src/lib/accelerator-transport.ts:30` (`#protocol` field), `:53-55` (`setProtocol`), `:48` (reset in `configure`), `:58-63` (`baseUrl` consumes it), `:125` (`postProve` silently defaults to http when never negotiated)
- Pin: `packages/sdk/src/lib/accelerator-prover.ts:256`
- Deliberately-not-pinned, enforced only by comment: `accelerator-prover.ts:245-254` ("Don't pin the protocol on error…")
- Clear on unparseable JSON, enforced by comment: `accelerator-prover.ts:264-268`
- Clear on offline: `accelerator-prover.ts:325`

**Why future change gets harder**: The invariant isn't even expressible in one sentence — today there are *three* distinct treatments (pin on healthy+parseable; leave-as-was on non-OK; clear on parse-fail/offline), and nothing structural distinguishes the intentional from the accidental. Every new probe outcome (timeout tier, auth challenge, redirect) forces the editor to re-derive the whole pin/clear matrix from two compensating comments spread across 80 lines, then mirror it correctly against the *second* lockstep state cell (`#statusCache` via `cacheAndReturn`). The transport also tolerates `postProve` before negotiation (`baseUrl` falls back to http), so the "probe ran first" ordering is an unchecked caller obligation. The two explanatory comments are the tell: prose doing the job of structure.

**Smallest safe refactoring**: Move Method — replace `cacheStatus` + scattered `setProtocol` calls with one transport-owned `commitStatus(status: AcceleratorStatus): AcceleratorStatus` that caches *and* derives the pin from the discriminant (pin when `available: true` or `reason: "version-mismatch"`; clear when `offline`; current non-OK/parse-fail behavior preserved per branch). Keep `setProtocol` non-exported or delete it.

**What disappears**: 4 scattered call sites, both "don't pin / clear because…" comments, the lockstep-update convention between `#protocol` and `#statusCache`, and the class of bug that #84/#319 already were.

---

## H-2. Published type surface lives inside the implementation hotspot → type-only import cycle

**Smell**: Cyclic Dependency (runtime-acyclic only via `import type`) + Divergent Change on `accelerator-prover.ts` (one file changes for: proving orchestration, the published type contract, phase vocabulary, config/env resolution).

**Impact**: High — this is a public npm package where type stability is the contract, and the `.d.ts` surface is generated from the single busiest file in the SDK (14 commits). Blast radius: all 4 source files, the barrel, the published `.d.ts`, and every consumer.

**Instances**:
- Back-edge: `packages/sdk/src/lib/accelerator-transport.ts:3` (`import type { AcceleratorProtocol, AcceleratorStatus } from "./accelerator-prover.js"`)
- Forward edge: `packages/sdk/src/lib/accelerator-prover.ts:7` (`import { AcceleratorTransport } from "./accelerator-transport.js"`)
- Types defined in the hotspot: `accelerator-prover.ts:11-20` (`AcceleratorPhase`), `:23-25` (`AcceleratorPhaseData`), `:27-34` (`AcceleratorConfig`), `:36-43` (`AcceleratorProverOptions`), `:46` (`AcceleratorProtocol`), `:56-92` (`AcceleratorStatus`)
- Barrel re-exports all of them from the implementation file: `packages/sdk/src/index.ts:1-9`
- The cycle leaks into tests too: `accelerator-transport.test.ts:2` imports the type from the prover file

**Why future change gets harder**: (a) The prover⇄transport edge is two-way; the moment the transport needs any *runtime* value from the prover module (a constant, a `isAvailable()` type guard — the natural next step for the union) the `import type` shield drops and it becomes a real ESM cycle. (b) Contract review is polluted: every one of the frequent implementation PRs diffs the same file that defines the published types, so an accidental contract change hides in implementation noise — exactly how `AcceleratorProtocol` previously fell out of the barrel (the incident `public-contract.test.ts` F-05 now pins). (c) Transport can never be published/extracted independently while its vocabulary lives in its caller.

**Smallest safe refactoring**: Move Type Declarations — create `packages/sdk/src/lib/types.ts` holding the six public types; prover and transport both import from it; barrel re-exports types from `types.ts` (runtime class still from the prover file). Pure relocation, zero behavior change; the F-05 contract test verifies the barrel stays intact.

**What disappears**: The two-way module edge, the future runtime-cycle trap, and contract-vs-implementation diff mixing — type changes become small reviewable diffs in a near-static file.

---

## M-1. `#probeAndParseHealth` — Long Method mixing transport outcomes, payload policy, side effects, and status construction

**Smell**: Long Method (Fowler).

**Impact**: Medium. Blast radius: the single most-edited method in the SDK's hottest file (#306, #315, #319, #336 all reshaped it). Change frequency: high — every health-protocol evolution (and there have been two protocol generations already) lands here.

**Instances**: `packages/sdk/src/lib/accelerator-prover.ts:233-328` (96 lines), interleaving five jobs:
1. HTTP outcome interpretation (`:245-254`)
2. Protocol-pin side effects (`:256`, `:268`, `:325` — see H-1)
3. JSON parse + error mapping (`:258-275`)
4. Multi-version protocol policy (`:281-299`)
5. Legacy exact-match policy + default-available (`:301-323`) and offline mapping (`:324-327`)

**Why future change gets harder**: Version-compatibility *policy* (which payload shapes mean compatible) is welded to I/O orchestration and cache/pin side effects, so the pure decision logic can't be unit-tested or modified without re-reasoning about side-effect ordering. A third protocol generation (the file has already seen two) means splicing another branch into a method where every `return` also performs caching and where two branches additionally mutate the pin. Note the method's own docblock records it was *already* extracted once from `checkAcceleratorStatus` (Q5/#306) — the decomposition stopped halfway.

**Smallest safe refactoring**: Extract Method — pull lines `:277-323` into a pure `#statusFromHealthPayload(data, protocol, sdkAztecVersion): AcceleratorStatus` (no I/O, no mutation). The shell keeps probe → pin/clear → cache (which then collapses further if H-1's `commitStatus` lands).

**What disappears**: The interleaving — protocol-generation policy becomes a side-effect-free function testable with plain objects; the shell drops to ~30 lines of mechanics.

---

## M-2. `AcceleratorStatus` union built by hand at 6 sites, 2 of them verbatim duplicates

**Smell**: Duplicate Code + missing-factory (Fowler: Introduce Factory Function).

**Impact**: Medium. Blast radius: one file, but it's the hottest method (M-1) of the hottest file, and the union has already been reshaped once as a breaking change (Q12/#315).

**Instances** (all in `packages/sdk/src/lib/accelerator-prover.ts`, all wrapped in `cacheAndReturn(...)`):
- `:248-253` — `{ available: false, reason: "error", sdkAztecVersion, protocol }`
- `:269-274` — **verbatim duplicate** of `:248-253`
- `:291-298` — available (multi-version)
- `:308-314` — version-mismatch
- `:317-323` — available (legacy)
- `:326` — offline

`sdkAztecVersion` is threaded into all 6; `protocol` into 5; the `cacheAndReturn` wrapper is repeated 6×.

**Why future change gets harder**: Most cross-variant fields are *optional* (`sdkAztecVersion?`, `acceleratorVersion?`, `availableVersions?`), so when the union grows a new optional field (the realistic evolution — e.g. an endpoint or timestamp), the compiler will NOT flag the branches you forgot — exactly the silent-drift mode a discriminated union was adopted to prevent. The duplicated error literal also invites the two error paths to diverge accidentally (they differ only in commentary today).

**Smallest safe refactoring**: Introduce Factory Function(s) — a tiny builder closing over `sdkAztecVersion` (computed once at `:234`): `mkError(protocol)`, `mkOffline()`, `mkMismatch(acceleratorVersion, protocol)`, `mkAvailable({...})`, each internally calling `cacheAndReturn`. Pure construction relocation; return values identical.

**What disappears**: The verbatim duplicate, 6× cache-wrapping, 6× `sdkAztecVersion` threading, and the forgot-a-branch-on-new-optional-field failure class.

---

## M-3. `createChonkProof` — Long Method with wire-protocol knowledge embedded in orchestration

**Smell**: Long Method + Feature Envy (the prover handles ky-specific error anatomy and raw response headers that belong with the transport it delegates I/O to).

**Impact**: Medium. Blast radius: the public entrypoint of the package; also the only place that knows two wire details (`x-prove-duration-ms`, ky 2.x `err.data` pre-parsing), so accelerator-server header changes and ky major bumps both land in the orchestrator. Change frequency: phase choreography has churned repeatedly (#84, #287, #317).

**Instances**: `packages/sdk/src/lib/accelerator-prover.ts:330-399` (70 lines) mixing five jobs:
1. Force-local routing (`:333-336`)
2. Detection + `downloading` phase (`:340-351`)
3. Serialization + `transmit`/`proving` phases (`:357-363`)
4. Remote prove + 403-denial decode — ky-version-specific `err.data` shape sniffing (`:365-385`, note the "ky 2.x pre-parses…" comment at `:372`)
5. Duration extraction from `x-prove-duration-ms` header + proof decode (`:387-398`)

**Why future change gets harder**: The transport docblock (`accelerator-transport.ts:19-23`) declares "the caller maps a 403 to origin denial", which pushes HTTP anatomy *up* into the domain orchestrator: a ky 3.x bump, an error-body shape change, or a duration-header rename all require editing the method whose real job is phase choreography — and every such edit re-risks the phase-ordering regressions that #84/#287 fixed.

**Smallest safe refactoring**: Extract Method ×2 inside the prover — `#decodeDenialBody(err): { error?, message? } | undefined` (`:373-376`) and `#resolveProveDuration(res, startMs): number` (`:389-391`). (The fuller move — transport returns `{ proofBase64, durationMs }` and throws a typed `OriginDeniedError` — is the H-1-aligned follow-up, but the extracts alone are safe and local.)

**What disappears**: ky-version and header-name knowledge each get exactly one named home; `createChonkProof` reads at a single altitude (route → detect → serialize → prove → decode).

---

## L-1. host/port/httpsPort data clump + mirrored env-resolution blocks in the constructor

**Smell**: Data Clump + Duplicate Code (Fowler).

**Impact**: Low. Blast radius: constructor + transport signature + config docs. Change frequency: low since #84 (NaN-port fix) — but that fix is evidence this resolution logic does get touched.

**Instances**:
- Clump travels as loose triple: `packages/sdk/src/lib/accelerator-prover.ts:174-196` (constructor resolution), `accelerator-transport.ts:33-37` (3 positional ctor params), `:44-50` (`configure`), `:58-63` + `:90-91` (URL assembly)
- Mirrored per-port resolution: `accelerator-prover.ts:184-190` (`envPort`/`envHttpsPort` guards) and `:189-194` (`parsedPort`/`parsedHttpsPort` + NaN-default, structurally identical twice)

**Why future change gets harder**: A fourth endpoint knob (path prefix, probe timeout override, second host) means editing the option interface, two mirrored env blocks, the transport's positional signature, and `configure` — and the option→env→default precedence is re-implemented per field, so a precedence change must be applied N times.

**Smallest safe refactoring**: Extract Function + Introduce Parameter Object — pure `resolveAcceleratorConfig(config?: AcceleratorConfig): Required<AcceleratorConfig>` (option → env → default, with one shared `resolvePort(explicit, envName, fallback)` helper); transport constructor takes the resolved object.

**What disappears**: The mirrored env/NaN blocks, the positional triple, and per-field precedence re-implementation; resolution becomes independently unit-testable.

---

## L-2. Tests hard-code unexported timing constants and pay real probe-retry sleeps

**Smell**: Duplicate Code (magic numbers coupled to private constants) + Meszaros "Slow Tests" caused by a production-side non-injectable delay. Flagged because it amplifies *production* change cost: tuning two prod constants breaks/slows tests in two files.

**Impact**: Low. Blast radius: both test files + any future probe-behavior change.

**Instances**:
- TTL `10_000` (`packages/sdk/src/lib/accelerator-transport.ts:6`, unexported) mirrored as magic `11_000` at `accelerator-prover.test.ts:381`, `:490` and `accelerator-transport.test.ts:55`
- `PROBE_RETRY_DELAY_MS = 1_000` (`accelerator-transport.ts:10-12` with real `setTimeout` at `:112`) makes every offline-path test (`accelerator-prover.test.ts` "falls back…unavailable", `accelerator-transport.test.ts:113-120`) burn ~1s of wall clock; retry-count/delay tuning is untestable without real waits

**Why future change gets harder**: Changing `STATUS_CACHE_TTL_MS` or the retry delay — one-line prod edits — requires hunting unlabeled numbers across two test files, and each added offline-path test taxes the suite by another real second, discouraging coverage of exactly the probe logic that churns most (H-1/M-1).

**Smallest safe refactoring**: Export the two constants (or expose via a single `TIMINGS` const) and reference them from tests; optionally give `AcceleratorTransport` an internal `retryDelayMs` constructor default that tests can shrink. No behavior change at defaults.

**What disappears**: The TTL magic-number triplication and the fixed 1s-per-offline-test tax on the hottest test surface.

---

## Out-of-scope observations

- `createLazySimulator`'s Proxy (`accelerator-prover.ts:123-136`) wraps *every* string-keyed access as an async function — if `CircuitSimulator` ever grows a data property or sync-result method, callers silently get promises/functions instead (latent correctness under upstream evolution, not a maintainability smell).
- `accelerator-prover.test.ts:380-396` restores `Date.now` inline rather than in `afterEach`, so an assertion failure mid-test leaks a skewed clock into later tests (test hygiene, not prod change cost).

## Non-findings (leads attacked and rejected)

- `createLazySimulator` Proxy as Middle Man: rejected — delegation *is* its purpose (lazy-load seam); no behavior-preserving refactor removes anything.
- Test mocking style as brittleness: rejected — both suites mock at the `globalThis.fetch` boundary and the public API, so the refactorings above (H-1…M-3) would not break them; only the L-2 constants couple.
