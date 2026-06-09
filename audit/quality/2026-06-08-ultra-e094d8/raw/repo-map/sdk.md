# SDK package map — `@alejoamiras/aztec-accelerator` (`packages/sdk`)

Public npm package (`@alejoamiras/aztec-accelerator`, AGPL-3.0-only, `version: 0.0.0` placeholder — bumped at release). Exposes one prover class that routes Aztec private-kernel proving to a localhost native accelerator and falls back to WASM. **Weighted in this audit — this is the consumer-facing API.**

Production surface is tiny: a single 494-LOC class plus a 3-LOC logger and an 8-LOC barrel. Everything else is tests, e2e, config, and docs.

Note: `package.json` `"exports": "./src/index.ts"` — the package ships **raw TS source** as its entrypoint (consumers' bundlers transpile it), with `dist/` (tsc output) also in `files`. So `src/lib/accelerator-prover.ts` is literally the shipped artifact, not just a build input.

---

## 1. Module inventory

| File | LOC | Purpose |
|------|-----|---------|
| `src/lib/accelerator-prover.ts` | 494 | The entire production surface: `AcceleratorProver` class, all exported types (`AcceleratorStatus`, `AcceleratorPhase`, `AcceleratorConfig`, `AcceleratorProverOptions`, `AcceleratorPhaseData`, `AcceleratorProtocol`), the lazy-simulator proxy, dual HTTP/HTTPS health probe, status cache, native `/prove` dispatch, and WASM fallback. |
| `src/index.ts` | 8 | Barrel — re-exports the class (value) + 5 types from the prover module. The whole public contract. |
| `src/lib/logger.ts` | 3 | LogTape logger instance, category `["aztec-accelerator","prover"]`. Not exported. |
| `src/test-setup.ts` | 10 | bun:test preload (per `bunfig.toml`): monkey-patches `expect.addEqualityTesters` (a vitest API `@aztec/foundation` probes for) so Aztec equality helpers don't crash under bun:test. |
| `src/lib/accelerator-prover.test.ts` | 690 | Unit tests (bun:test) — 28 tests across Proving / checkAcceleratorStatus / Constructor. Mocks `globalThis.fetch`, ky, the serializer, and the WASM super-prover. |
| `e2e/proving.test.ts` | ~105 | E2E — real `EmbeddedWallet` + Sponsored FPC, deploys a Schnorr account in accelerated (skipped unless `ACCELERATOR_URL`) and WASM modes against a live/sandbox node. |
| `e2e/connectivity.test.ts` | ~38 | E2E — asserts Aztec node `/status` and (optional) accelerator `/health` reachable. |
| `e2e/remote-network.test.ts` | ~47 | E2E — remote-only (`skipIf(isLocalNetwork)`) testnet smoke: non-31337 chain id, node version present. |
| `e2e/e2e-helpers.ts` | ~45 | `deploySchnorrAccount(wallet, fpc, label?)` — network-agnostic deploy helper. |
| `e2e/e2e-setup.ts` | ~89 | E2E preload — LogTape config, `config` (nodeUrl/acceleratorUrl), `isLocalNetwork`, fail-fast service assertion. Also re-patches `addEqualityTesters`. |
| `package.json` | 48 | Manifest. `exports` points at `src/index.ts`. Aztec deps pinned to `4.2.0`. |
| `tsconfig.json` | 10 | Extends repo root; `rootDir src`, excludes `*.test.ts`. |
| `bunfig.toml` | 4 | `[test]` 600s timeout + preload `./src/test-setup.ts`. |
| `README.md` | 224 | Public README + API reference table. **Contains stale `AcceleratorStatus` interface — see §Quality (c).** |
| `MIGRATION.md` | 84 | Q12 migration guide: flat interface → discriminated union. Accurate vs. current code. |
| `.claude/skills/aztec-accelerator/SKILL.md` | 184 | Claude Code skill shipped in the package (`files` includes `.claude`). Integration guide; documents the union correctly. |
| `dist/` | — | tsc build output (`index.js`/`.d.ts` + `lib/`). Generated, in `files`. |

---

## 2. Public API surface (entrypoints)

Exported from `src/index.ts`: the value `AcceleratorProver` + types `AcceleratorConfig`, `AcceleratorPhase`, `AcceleratorPhaseData`, `AcceleratorProverOptions`, `AcceleratorStatus`.

**Not re-exported from the barrel but still public** (returned/accepted by exported signatures, so transitively part of the contract):
- `AcceleratorProtocol = "http" | "https"` — `export`ed from the module (line 46), reachable via `import { AcceleratorProtocol } from ".../lib/accelerator-prover.js"` and is the type of `status.protocol`. **Not** in the `index.ts` barrel — a consumer doing `import { AcceleratorProtocol } from "@alejoamiras/aztec-accelerator"` gets nothing. MIGRATION.md claims "The new `AcceleratorProtocol` type is exported for convenience" — true from the module, false from the package root. (Minor drift; see §c.)
- `CircuitSimulator` (from `@aztec/simulator/client`) — type of `AcceleratorProverOptions.simulator`; consumer must import it from `@aztec/*` themselves.
- `PrivateExecutionStep`, `ChonkProofWithPublicInputs` (from `@aztec/stdlib`) — param/return of `createChonkProof`.

### `class AcceleratorProver extends BBLazyPrivateKernelProver`

Constructor:
```ts
constructor(options?: AcceleratorProverOptions)
```
`AcceleratorProverOptions = { simulator?: CircuitSimulator; accelerator?: AcceleratorConfig; onPhase?: (phase, data?) => void }`. Zero-arg is the documented happy path. Passes `simulator ?? createLazySimulator()` to `super`. Resolves ports with precedence **constructor opt → env (`AZTEC_ACCELERATOR_PORT` / `_HTTPS_PORT`) → default** (59833 / 59834); host: opt → default `127.0.0.1` (host has **no** env override — asymmetry vs ports).

Public methods:

| Signature | Returns | Notes |
|-----------|---------|-------|
| `checkAcceleratorStatus()` | `Promise<AcceleratorStatus>` | TTL-cached (10s) health probe. Cache fast-path in this method; probe/parse delegated to private `#probeAndParseHealth`. |
| `setAcceleratorConfig(config: AcceleratorConfig)` | `void` | Mutates port/httpsPort/host (only fields present); resets **both** `#acceleratorProtocol` and `#statusCache`. |
| `setOnPhase(callback \| null)` | `void` | Swap/clear the phase callback post-construction. |
| `setForceLocal(force: boolean)` | `void` | Bypass accelerator entirely → straight to WASM. |
| `createChonkProof(executionSteps: PrivateExecutionStep[])` | `Promise<ChonkProofWithPublicInputs>` | **Override** of the base method. Dispatches: force-local → WASM; else detect → (native `/prove` \| WASM fallback). |

All internal state is `#private` (true ECMAScript privates): `#onPhase`, `#acceleratorPort`, `#acceleratorHttpsPort`, `#acceleratorHost`, `#acceleratorProtocol`, `#statusCache`, `#forceLocal`, static `#STATUS_CACHE_TTL`. No leaky public fields. README documents only 4 of the 5 public methods — omits `setForceLocal` from the method table (it's covered in prose §7).

### `type AcceleratorStatus` (discriminated union on `available`)

```ts
type AcceleratorStatus =
  | { available: true;  needsDownload: boolean; acceleratorVersion?: string;
      availableVersions?: string[]; sdkAztecVersion?: string; protocol: AcceleratorProtocol }
  | { available: false; reason: "offline";          sdkAztecVersion?: string }
  | { available: false; reason: "error";            sdkAztecVersion?: string; protocol: AcceleratorProtocol }
  | { available: false; reason: "version-mismatch"; acceleratorVersion: string; sdkAztecVersion?: string; protocol: AcceleratorProtocol };
```

Variant field matrix:

| Variant | discriminant(s) | required | optional |
|---|---|---|---|
| available | `available:true` | `needsDownload`, `protocol` | `acceleratorVersion`, `availableVersions`, `sdkAztecVersion` |
| offline | `available:false`, `reason:"offline"` | — | `sdkAztecVersion` |
| error | `available:false`, `reason:"error"` | `protocol` | `sdkAztecVersion` |
| version-mismatch | `available:false`, `reason:"version-mismatch"` | `acceleratorVersion`, `protocol` | `sdkAztecVersion` |

Well-formed: narrows on `available` then on `reason`; `protocol` present iff a host answered (absent only on `offline`); `acceleratorVersion` required exactly where it's load-bearing (version-mismatch). No illegal combos typecheck — this is the explicit Q12 improvement over the old flat interface (MIGRATION.md). Invariants are pinned by a characterization test (test file L315-355).

### Other exported types
- `AcceleratorPhase` = `"detect" | "serialize" | "transmit" | "proving" | "proved" | "receive" | "fallback" | "downloading" | "denied"` — 9 UI sub-phases.
- `AcceleratorPhaseData = { durationMs: number }` — only ever carried by the `"proved"` phase (untyped association — see §a Primitive/typing note).
- `AcceleratorConfig = { port?: number; httpsPort?: number; host?: string }`.
- `AcceleratorProverOptions` (above).
- `AcceleratorProtocol = "http" | "https"`.

---

## 3. Trust boundaries

Untrusted/external data enters at exactly two network reads, both from the **local accelerator** (`127.0.0.1` by default, but host/port are caller-configurable, so "local" is not enforced):

1. **`GET /health`** — parsed in `#probeAndParseHealth` (L252-374). Response body cast to `{ aztec_version?: string; available_versions?: string[] }` (L304-309) via `as` with **no runtime schema validation**. `available_versions` truthiness selects the multi-version branch; `aztec_version` drives legacy version-match. JSON-parse failure is caught (L310) and downgraded to `reason:"error"` (correctly distinguished from `offline`). A malicious/buggy local responder could send arbitrary strings here; the SDK trusts them for download-needed logic and version-mismatch messaging, but the blast radius is limited to UI status + whether it attempts a native prove (which then fails closed to WASM). `unknown` sentinel for `aztec_version` is special-cased (L348).
2. **`POST /prove`** — via `ky` (L414). Response body cast to `{ proof: string }` (L449), base64-decoded with `Buffer.from(response.proof, "base64")` (L451) and fed to `ChonkProofWithPublicInputs.fromBuffer` (L452). **No validation that `proof` is a string before decode**; a non-string/absent `proof` would make `Buffer.from(undefined, "base64")` throw, surfacing as an uncaught rejection from `createChonkProof` (not the documented fail-safe). The proof bytes themselves are trusted to `fromBuffer` — deserialization is the real gate, but a hostile local server is in the TCB for whatever the kernel does with a crafted proof. The `403` path is explicitly modeled (origin-denied → WASM, L425-437); other non-2xx throw (ky default) and propagate.

**Base class trust:** extends `BBLazyPrivateKernelProver` (`@aztec/bb-prover/client/lazy`). `super.createChonkProof` is the WASM path; `super(simulator)` wiring is trusted. The override fully owns dispatch.

**WASM fallback path:** `#fallbackToWasm` → `#proveLocally` → `super.createChonkProof`. Trusted Aztec code; the lazy simulator proxy (`createLazySimulator`, L99-137) dynamically `import()`s `@aztec/simulator/client` — a deferred dependency-load trust point (throws a clear error if absent).

**Version self-trust:** `#getAztecVersion` (L486-493) reads `@aztec/stdlib` out of the **bundled `package.json`** (`import sdkPkg ... with { type: "json" }`) and strips the semver range prefix; sent as `x-aztec-version` to `/prove`. Self-sourced, low risk.

---

## 4. Dependency graph

**Runtime `@aztec/*` (all pinned `4.2.0`):**
- `@aztec/bb-prover/client/lazy` → `BBLazyPrivateKernelProver` (**extended**).
- `@aztec/stdlib/kernel` → `PrivateExecutionStep` (type), `serializePrivateExecutionSteps` (msgpack encode, L404).
- `@aztec/stdlib/proofs` → `ChonkProofWithPublicInputs` (return type + `.fromBuffer`, L452).
- `@aztec/simulator/client` → `CircuitSimulator` (type) + `WASMSimulator` (lazy `import()` only, L106 — NOT a static import; **declared as a devDependency**, not a runtime dep — intentional, the proxy defers it).
- `@aztec/foundation`, `@aztec/noir-acvm_js`, `@aztec/noir-noirc_abi` — transitive needs of bb-prover; not imported directly in src.

**Other runtime deps:**
- `ky ^2.0.2` — HTTP client for `/prove` only (`ky.post`, `HTTPError`). `/health` uses the **native `fetch`**, not ky (two different HTTP mechanisms — see §b).
- `ms ^2.1.3` — `ms("10 min")` for the prove timeout (L415).
- `@logtape/logtape ^2.0` — structured logging.

**Dispatch logic (`createChonkProof`, L376-453):**
```
forceLocal? ──yes──> #proveLocally (WASM)                              [L379-382]
   │no
   ▼ onPhase("detect"); status = checkAcceleratorStatus()             [L386-387]
!status.available? ──yes──> #fallbackToWasm                           [L389-392]
   │no
   ▼ status.needsDownload? → onPhase("downloading")                   [L394-397]
   ▼ serialize → ky.post /prove (10min, retry:0, x-aztec-version)     [L403-422]
   │  └─ catch HTTPError 403 → onPhase("denied") → #fallbackToWasm    [L425-437]
   │  └─ catch other → throw                                          [L438]
   ▼ emit "proved" (server x-prove-duration-ms or client-measured)    [L443-447]
   ▼ res.json → base64 decode → ChonkProofWithPublicInputs.fromBuffer [L449-452]
```
Protocol selection: `#acceleratorBaseUrl` getter (L224-229) returns the HTTPS base only if `#acceleratorProtocol === "https"` (set by the health probe, L302), else HTTP. So `/prove`'s protocol is **stateful** — it depends on a prior `checkAcceleratorStatus` having run and cached the winning protocol.

---

## 5. Frameworks / libs

- **Test runner:** `bun:test` (unit + e2e). No vitest in this package (vitest is the React-tests choice elsewhere in the monorepo). `test-setup.ts` shims the one vitest API (`addEqualityTesters`) that `@aztec/foundation` feature-detects.
- **HTTP:** `ky` (prove) + native `fetch` (health). `AbortSignal.timeout` for health (2s) and remote smoke; `ms` for the 10-min prove timeout.
- **Logging:** LogTape (`@logtape/logtape`).
- **Aztec stack:** bb-prover / stdlib / simulator / foundation / noir-* @ 4.2.0; e2e pulls aztec.js, accounts, pxe, wallets, noir-contracts.js (all devDeps).
- **TS:** strict, `noUncheckedIndexedAccess`, `verbatimModuleSyntax`, NodeNext, ESNext target, `declaration`+`declarationMap` (ships `.d.ts`).

---

## 6. Test surfaces

`accelerator-prover.test.ts` (28 tests) — strategy: mock `globalThis.fetch` via route-pattern handlers; spy on `BBLazyPrivateKernelProver.prototype.createChonkProof` (WASM) and `serializePrivateExecutionSteps`.

**Covered well:**
- Fallback to WASM: offline, legacy version-mismatch, 403-denied (asserts `denied`+`fallback` phases).
- Multi-version protocol: `needsDownload` true/false, "always proceeds" (no fallback on version drift).
- `x-aztec-version` header sent on `/prove`.
- `"proved"` always emitted even without `x-prove-duration-ms` (named regression).
- Status union discriminant invariants (characterization, L315-355): available⟹protocol+version; offline⟹no protocol; malformed-JSON⟹`reason:"error"` not `offline` (codex guard, L296-313).
- Protocol caching: HTTPS-fallback (Safari mixed-content sim, L411-433), protocol reused on `/prove`, not cached on non-ok.
- Status cache: hit within TTL (no re-probe), re-probe after TTL, offline cached (skips 1s retry, asserts <50ms second call).
- `setAcceleratorConfig` resets protocol **and** invalidates status cache (named regression, L568-591).
- Constructor: zero-config, invalid env port → default (no `NaN` in URL), env override, phase-callback ordering (offline path).

**Gaps (quality-relevant):**
- **No test for the `/prove` happy-path return value** — every native test either 404s, returns empty `proof:""`, or is denied; `ChonkProofWithPublicInputs.fromBuffer` of a real proof is never exercised in unit tests (only e2e, behind `ACCELERATOR_URL`). Empty-string proof's base64-decode behavior is incidental, not asserted.
- **No test for a malformed `/prove` body** (missing/non-string `proof`) — the un-guarded `Buffer.from` decode (§3.2) is untested; the "only corrupted steps / simulator unavailability throw" README claim is unverified for this path.
- **No test for the `x-prove-duration-ms`-present branch** preferring server time over client time (only the absent branch is tested).
- **No test asserting the `denied` 403 body parsing** (`err.data.error`/`message` extraction, L427-434) — only that the phase fires.
- **No direct test of `createLazySimulator`** (the `then`/symbol-trap guard, L129) or of `setForceLocal(true)` taking the early WASM path (force-local branch L379 is uncovered; `setForceLocal` only exists via README prose).
- `host` env-override absence is untested (because it doesn't exist).

E2E (`e2e/*`): real deploy in WASM + accelerated modes, connectivity, remote testnet smoke. Accelerated tests gated `skipIf(!ACCELERATOR_URL)` — correct pattern. Forces WASM via `setAcceleratorConfig({ port: 1 })` (unreachable) rather than `setForceLocal` — a slight inconsistency (exercises the fallback path, not the force-local path).

---

## 7. Generated / vendored / fixture

- `dist/` — tsc output (`.js` + `.d.ts` + maps). Generated; included in `files` for consumers whose bundler prefers built output.
- `amp` (repo-root, untracked per git status) — not in this package; ignore.
- No vendored deps, no fixture data files, no codegen in `src/`. The bundled `package.json` import (`with { type: "json" }`) is the only non-code import.
- `node_modules` excluded.

---

## Quality-relevant signals

### (a) Public API shape & ergonomics

**`AcceleratorStatus` union — well-formed. (high confidence)** Narrowable on `available` then `reason`; no illegal field combinations typecheck; required fields land exactly where load-bearing (`protocol` everywhere a host answered, `acceleratorVersion` on version-mismatch, `needsDownload` only when available). This is a deliberate, documented improvement (MIGRATION.md, Q12) and is regression-pinned by the characterization test. **No Primitive Obsession in the discriminant** — `reason` and `available` are proper literal unions, not bare booleans/strings smuggling state.

**Minor typing weaknesses:**
- **`AcceleratorPhaseData` is structurally decoupled from `AcceleratorPhase`** (`accelerator-prover.ts:11-25,42`). The callback is `(phase: AcceleratorPhase, data?: AcceleratorPhaseData) => void`, but only `"proved"` ever carries data (L447,468) — the type doesn't encode that. A discriminated callback (e.g. `{ phase: "proved"; durationMs: number } | { phase: <others> }`) would make the `data?.durationMs` access at call sites total instead of optional. Low severity; it's a one-field bag. This is mild **Primitive Obsession** (a raw `durationMs: number` rather than a `Duration`/branded ms), but for a UI-timing field that's defensible.
- **`acceleratorVersion?` is optional on the `available:true` variant** (L63) — comment says it's "absent on the multi-version protocol." But `availableVersions?` is *also* optional on the same variant. The available branch can therefore carry **neither** version field (legacy responder with `aztec_version:"unknown"` → `acceleratorVersion` set to `undefined` by the code path at L363-369? No: it's set to `data.aztec_version` which may be `"unknown"`). The available variant doesn't sub-discriminate legacy vs multi-version, so a consumer can't statically tell which version field to trust — mild **Data Clump** (the two version fields + `sdkAztecVersion` travel together and their validity is protocol-coupled but not type-coupled). Acceptable, but a `protocol`-style sub-discriminant (`mode: "legacy" | "multi"`) would tighten it.

**Long Method:** `createChonkProof` (`accelerator-prover.ts:376-453`, ~78 LOC) is the longest method and does a lot: force-local check, detect, fallback, download signal, serialize, the entire `ky.post` + 403-handling try/catch, duration resolution, and proof decode. It's **borderline** — the WASM paths were already extracted (`#proveLocally`, `#fallbackToWasm`), and the comments are good, but the inline 403 body-parsing block (L425-437) and the duration-resolution logic (L443-445) are extractable seams if it grows. `#probeAndParseHealth` (`accelerator-prover.ts:252-374`, ~122 LOC) is **the genuinely long one** — nested try/catch/try/catch with three return shapes; see (b). Not flagged as a violation yet, but it's the file's complexity hotspot.

### (b) Dual probe + caching + fallback

- **Two HTTP mechanisms for one server (duplication / inconsistency).** Health uses **native `fetch`** with manual `Promise.any` racing (`accelerator-prover.ts:259-269`); prove uses **`ky`** (L414). Different timeout APIs (`AbortSignal.timeout(2000)` vs `ms("10 min")`), different retry semantics (manual 1s sleep-retry vs `ky retry:0`), different error handling (catch-all vs `HTTPError`). Mild **duplicated transport concern** — a single configured client (or at least a shared probe helper) would remove the divergence. (moderate)

- **`#acceleratorBaseUrl` duplicates URL construction.** The base-URL getter (L224-229) rebuilds `http://host:port` / `https://host:httpsPort`, while `#probeAndParseHealth` independently builds `httpUrl`/`httpsUrl` from the same fields (L254-255). Two sources of truth for "where is the accelerator" — a refactor seam if a third endpoint is added. (low)

- **Temporal Coupling — `/prove` protocol depends on a prior `checkAcceleratorStatus`. (moderate, real)** `#acceleratorProtocol` is only ever set inside the health probe (L302). `createChonkProof` does call `checkAcceleratorStatus()` first (L387), so in the dispatch path it's safe — but the coupling is implicit: `#acceleratorBaseUrl` silently defaults to **HTTP** if protocol was never resolved (e.g. a Safari-only environment where a stale cached `available:true` from a *different* protocol path is reused). The cache stores the `AcceleratorStatus` but the protocol lives in a *separate* field (`#acceleratorProtocol`), and on the malformed-JSON / non-ok paths the protocol is explicitly reset to `null` (L294-302, L314) **while the status is still cached** — so a cached "error"/offline status and the live protocol field can momentarily disagree. The two pieces of cache state (`#statusCache` + `#acceleratorProtocol`) are coupled by convention, not structurally. `setAcceleratorConfig` has to remember to clear **both** (L210-211) — and a prior bug where it cleared only one is the subject of a named regression test (L568-591), which is direct evidence this coupling has already bitten once.

- **Error-swallowing-as-control-flow (pervasive, mostly intentional but worth flagging).** The probe is built on caught exceptions driving outcomes:
  - `try { probe() } catch { sleep; probe() }` (L282-288) — first-probe failure → silent retry.
  - outer `catch { protocol=null; return offline }` (L370-372) — **bare `catch` with no binding**: any throw (network, abort, *or a bug*) is flattened to `offline`. A programming error inside the try would be misreported as "accelerator offline." (moderate)
  - JSON-parse `catch` → `reason:"error"` (L310-321) — intentional and tested.
  - `Promise.any` (L260) uses rejection-as-signal by design (first success wins, all-fail → AggregateError → retry). Fine, but note the **losing** probe's promise rejects unhandled-ish (Safari case: HTTP rejection after HTTPS wins) — relies on `Promise.any` semantics not surfacing it.
  The net effect: the only ways `createChonkProof` throws are a non-403 `/prove` HTTPError, the un-guarded proof decode (§3.2), or WASM-super throwing — but several genuine bugs would be silently reclassified as "offline"/"error" and mask themselves as a WASM fallback. That's the classic cost of error-as-control-flow: **real failures become invisible.**

- **Cache TTL is a magic-ish constant** (`#STATUS_CACHE_TTL = 10_000`, L168) — fine, but the 2000ms probe timeout (L261,265) and the 1000ms retry sleep (L286) are inline literals, not named. (low)

### (c) Doc ↔ exported-type drift

- **README.md is STALE — documents the OLD flat `AcceleratorStatus` interface (`README.md:88-101`). (high confidence, real drift)** It still shows:
  ```ts
  interface AcceleratorStatus { available: boolean; needsDownload: boolean; acceleratorVersion?: string; availableVersions?: string[]; sdkAztecVersion?: string; protocol?: "http" | "https"; }
  ```
  i.e. the **pre-Q12 flat interface that MIGRATION.md explicitly says was replaced**. The actual export is the discriminated union. A consumer reading the README's API reference would write `status.needsDownload` without narrowing — a type error against the shipped types, and exactly the footgun the union exists to prevent. README §"How It Works"/"Error handling" prose is otherwise consistent, and the README *doesn't* mention the union or `reason` field at all. **This is the top doc-drift finding.** (SKILL.md and MIGRATION.md both correctly describe the union — only README lags.)
- **`AcceleratorProtocol` export-location drift (low).** MIGRATION.md (L83): "The new `AcceleratorProtocol` type is exported for convenience." It's exported from the *module* but **not from the package barrel** (`index.ts` re-exports 5 types, not this one). `import { AcceleratorProtocol } from "@alejoamiras/aztec-accelerator"` fails. Either add it to the barrel or soften the doc.
- **README method table omits `setForceLocal` (low).** The API table (`README.md:61-66`) lists 4 methods; `setForceLocal` is public and documented only in SKILL.md §7 + README prose §"Force WASM". Minor completeness gap.
- **SKILL.md phase table omits `denied` (low).** The SKILL.md phase table (L80-90) lists 8 phases and drops `"denied"` (the README table includes it). The type has 9. Minor.
- **No drift in the prove/health wire contract docs** — `x-aztec-version`, `x-prove-duration-ms`, `/health` `available_versions`, 403-denied flow all match the code.

---

## Top public-API quality signals (file:line)

1. **README documents the obsolete flat `AcceleratorStatus` interface** — `packages/sdk/README.md:88-101` contradicts the shipped discriminated union (`accelerator-prover.ts:56-92`) and MIGRATION.md. Highest-impact drift; misleads every consumer of the API reference.
2. **`/prove` response trusted without validation before base64 decode** — `accelerator-prover.ts:449-451`: `res.json<{proof:string}>()` then `Buffer.from(response.proof,"base64")` with no string check; a malformed local-server body throws an *uncaught* rejection, violating the README's "only corrupted steps / simulator throw" fail-safe contract. Untested.
3. **Bare catch-all reclassifies any error as `offline`** — `accelerator-prover.ts:370-372`: error-as-control-flow that masks real bugs as "accelerator offline → WASM fallback," making genuine failures invisible.
4. **Temporal coupling between `#statusCache` and `#acceleratorProtocol`** — `accelerator-prover.ts:166-167,302,314` + the two-field reset in `setAcceleratorConfig` (L210-211); already caused one shipped bug (regression test L568-591). Two pieces of cache state coupled by convention, not structure.
5. **Two HTTP stacks for one server** — native `fetch` for `/health` (`accelerator-prover.ts:259-269`) vs `ky` for `/prove` (L414): divergent timeout/retry/error semantics; a duplicated-transport smell.
6. **`AcceleratorProtocol` exported from module but not the package barrel** — `index.ts:1-8` vs MIGRATION.md:83; `AcceleratorPhaseData` is decoupled from the `"proved"` phase it exclusively serves (`accelerator-prover.ts:11-25,42`).
