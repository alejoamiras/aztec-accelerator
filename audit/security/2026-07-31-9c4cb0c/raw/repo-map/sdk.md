# Phase 1 Map — `packages/sdk` (`@alejoamiras/aztec-accelerator`)

Mapper: Opus Explore agent, 2026-07-31. Paths repo-relative.

## 1. Module inventory

| File | Purpose | LOC |
|---|---|---|
| `packages/sdk/src/index.ts` | Barrel: re-exports `AcceleratorProver` + 6 public types. | 9 |
| `packages/sdk/src/lib/accelerator-prover.ts` | Public class extending `BBLazyPrivateKernelProver`; config/env resolution, health→status classification, `/prove` orchestration (serialize → POST → decode), HTTPS→HTTP demotion retry, WASM fallback. **Primary hotspot.** | 555 |
| `packages/sdk/src/lib/accelerator-transport.ts` | All network I/O: URL construction, dual HTTP/HTTPS `/health` probe + protocol negotiation, bounded body reader, 10 s status cache, generation guard, `/prove` POST. Internal — not in the barrel. | 468 |
| `packages/sdk/src/lib/types.ts` | Published types (`AcceleratorStatus` union, config/options/phase). No runtime code. | 96 |
| `packages/sdk/src/lib/logger.ts` | LogTape logger `["aztec-accelerator","prover"]`. | 3 |
| `packages/sdk/src/test-setup.ts` | Bun preload monkey-patching `expect.addEqualityTesters` on `globalThis`. Test-only **but shipped** (§7). | 10 |
| `packages/sdk/src/lib/*.test.ts` | Unit tests, fetch fully mocked. | ~1564 |

Non-test runtime source: **~1141 LOC**.

## 2. Public API surface (third-party contract)

`package.json` uses a **string** `exports: "./src/index.ts"` — consumers import raw TypeScript.
`files: ["src","dist",".claude"]` means everything under `src/` ships in the tarball (reachable by
deep path even though `exports` does not map it).

Runtime export (`index.ts:1`): `AcceleratorProver` — `constructor(options?)`,
`checkAcceleratorStatus()` (`accelerator-prover.ts:160`), `setAcceleratorConfig()` (`:137`),
`setOnPhase()` (`:144`), `setForceLocal()` (`:149`), `createChonkProof(steps)` (`:292`, the override
Aztec's PXE calls), plus the **unenumerated inherited `BBLazyPrivateKernelProver` surface**.

Type-only (`index.ts:2-9`): `AcceleratorConfig`, `AcceleratorPhase`, `AcceleratorPhaseData`,
`AcceleratorProtocol`, `AcceleratorProverOptions`, `AcceleratorStatus`.

Not barrel-exported but module-level `export`ed (deep-path reachable): `AcceleratorTransport`,
`isRecognizedHealthBody`, `ProtocolTransition` (`accelerator-transport.ts:39,52,156`), `logger`.

## 3. Entrypoints / external interactions

Network (all via `ky` ^2.0.2 → `fetch`):

| # | Site | URL | Method / headers / body |
|---|---|---|---|
| 1 | `accelerator-transport.ts:302` | `https://${host}:${httpsPort}/health` | GET, `retry:0`, `throwHttpErrors:false`, `timeout:2000`, `redirect:"error"` |
| 2 | `:318` | `http://${host}:${port}/health` — skipped in `httpsOnly` (`:316-317`) | same; fired in parallel with #1 (`:319`) |
| 3 | `:418-428` | single-protocol `/health` re-probe validating HTTP before a downgrade | http refused in strict mode at `:417` |
| 4 | `:455-466` | `${baseUrl}/prove` or an explicit snapshot URL | **POST**, body = msgpack-serialized `PrivateExecutionStep[]`, `content-type: application/octet-stream`, `x-aztec-version`, `timeout: 10 min` (`:28`), `retry:0`, `redirect:"error"` |

**No `credentials`, no `mode`, no `signal` is ever set** — defaults apply; the dApp cannot abort a
10-minute `/prove` (no caller-supplied `AbortSignal` anywhere in `src/`).

URL derivation: `baseUrl` (`:253-258`), `proveUrlFor(protocol)` (`:438-442`, bypasses the pin for the
demotion retry). Prove URL snapshotted before any `onPhase` runs (`accelerator-prover.ts:351-357`).

Platform APIs: `fetch` (via ky); `ReadableStream` + `TextEncoder/Decoder` for the bounded health read
(`:63-143` — 2 s deadline, 64 KB cap, empty-chunk starvation guard); `performance.now()`;
`setTimeout/clearTimeout`; **`Buffer.from(..., "base64")`** (`accelerator-prover.ts:510` — Node global
in a browser library, needs a bundler polyfill; absent one it throws into the WASM fallback at
`:483-493`); `process.env` (guarded); dynamic `import("@aztec/simulator/client")` (`:31`) behind a
`Proxy` (`:48-61`). **No** storage/Worker/WebCrypto/postMessage/window/document.

Config reads: `AZTEC_ACCELERATOR_PORT` / `_HTTPS_PORT` (`accelerator-prover.ts:111-123`, `parseInt`
base 10, NaN→default, **no range validity check** — negative, 0, >65535, `"80junk"` accepted);
`AZTEC_ACCELERATOR_HTTPS_ONLY` (`:115-116,126-127`); `options.accelerator.{port,httpsPort,host,httpsOnly}`
(`:104-109`, runtime via `setAcceleratorConfig` → `accelerator-transport.ts:185-193`) — **`host` has no
env override and no validation**; `package.json` imported as JSON (`:6`, read `:550-553`) pulling the
whole manifest into the consumer bundle; defaults `59833`/`59834`/`127.0.0.1` (`:64-66`).

## 4. Trust boundaries

**A — the `/health` response decides whether private witness data leaves the page.**
`isRecognizedHealthBody` (`accelerator-transport.ts:52-56`) requires exactly `status === "ok" &&
api_version === 1`, documented as *collision resistance, not authentication* (`:44-50`). Bounded read
`readJsonBounded` (`:63-143`). Winner selection / protocol pin `:346-404` (healthy HTTPS beats HTTP; a
foreign 2xx cannot win over a healthy peer; `HTTPS_GRACE_MS = 250`). Pin transitions centralized in
`commitStatus` (`:236-246`) with a generation guard (`:241`, set by `configure` `:192`). Downgrade
guard: HTTP independently health-validated before a post-HTTPS-failure retry
(`accelerator-prover.ts:423-433` → `accelerator-transport.ts:415-434`). `redirect:"error"` everywhere
(`:312,427,461`).

**B — `/health` fields are trusted as strings and surfaced to the dApp.**
`accelerator-prover.ts:218` casts to `{ aztec_version?: string; available_versions?: string[] }` with
no runtime type validation beyond the two-field contract. `#classifyHealth` (`:242-290`) calls
`availableVersions.includes(...)` — a string value silently mis-classifies; a number throws into the
bare catch at `:227` → reported `offline`. These attacker-controllable strings are logged
(`:253-258,276-279`) and returned in `AcceleratorStatus.acceleratorVersion` / `.availableVersions` for
the dApp to render.

**C — the `/prove` response is trusted far more weakly than `/health`.**
`accelerator-prover.ts:508`: `(await res.json()) as { proof: string }` — **no size cap, no deadline, no
content-type check**, in direct contrast to the hardened health read. `:510-511`:
`Buffer.from(response.proof,"base64")` → `ChonkProofWithPublicInputs.fromBuffer(...)`. **The SDK never
verifies the returned proof corresponds to the submitted witness.** `:502-504`: `x-prove-duration-ms`
`Number()`-parsed and surfaced to UI. `:384-393`: a 403 body's `error`/`message` logged.

**D — TLS / origin.** No cert pinning or introspection; TLS trust is the browser's. `httpsOnly` is off
by default (`types.ts:44-49`), so the default posture sends the witness over **plaintext HTTP to
127.0.0.1** whenever HTTPS isn't healthy. `host` is caller-controlled with **no localhost constraint**
(`accelerator-prover.ts:107`, `accelerator-transport.ts:188`).

**E — data flowing outward.** `accelerator-prover.ts:362` serializes the **complete private execution
witness** (bytecode, VKs, witness maps) POSTed at `accelerator-transport.ts:455`, plus
`x-aztec-version` and the browser-supplied `Origin`. Nothing redacted.

**F — the dApp's `onPhase` callback is re-entrant surface.** Invoked synchronously at
`accelerator-prover.ts:308,361,366,367,394,506,509,522,527,539,541`; may call `setAcceleratorConfig`
mid-flight. Mitigated by URL snapshotting (`:351-357`) + generation re-checks (`:317,373,402,424`).
Exceptions from `onPhase` are **not** caught — they propagate out of `createChonkProof`.

## 5. Dependency graph

`@aztec/bb-prover` → `BBLazyPrivateKernelProver` (the WASM fallback that runs when the accelerator is
absent). `@aztec/stdlib` → `serializePrivateExecutionSteps` + `ChonkProofWithPublicInputs.fromBuffer`
(**parses attacker-reachable bytes**). `@aztec/foundation`, `@aztec/noir-acvm_js`,
`@aztec/noir-noirc_abi` declared but not imported in `src/`. `ky` ^2.0.2 — the network library, on a
caret range; the SDK relies on its `timeout`/`retry`/`throwHttpErrors`/`redirect`/`HTTPError.data`
semantics. `@logtape/logtape` ^2.0. `ms` ^2.1.3.
**`@aztec/simulator/client` is dynamically imported at runtime (`accelerator-prover.ts:31`) but declared
only as a devDependency** — the default zero-config path depends on a package the manifest does not
require consumers to have.

Publish config: `license: AGPL-3.0-only`, `publishConfig.access: public`, `version: 0.0.0`,
`prepublishOnly` runs `tsc --noEmit` + build. **No `provenance` flag in `publishConfig`.**

## 6. Test surfaces

Unit (`bun test src/`, preloads `src/test-setup.ts`) — no real network; fetch replaced by a route-table
mock, env mutated and restored. `accelerator-transport.test.ts` covers adversarial bodies
(never-closing stream `:423`, endless zero-length chunks `:444`, post-deadline close `:465`,
foreign-JSON pin poisoning `:263,277`, strict-mode no-http-URL `:406,495`).
E2E (`packages/sdk/e2e/`) hits real networks — `e2e-setup.ts:59,72`, `connectivity.test.ts:28`,
`remote-network.test.ts:20` (script hardcodes `https://v5.testnet.rpc.aztec-labs.com`),
`proving.test.ts` deploys real accounts. Not published (`files` excludes `e2e/`).

## 7. Generated / fixture — not finding-eligible

`packages/sdk/dist/**`, `node_modules/**`, `packages/sdk/e2e/**`, `src/lib/*.test.ts`,
`.claude/skills/.../SKILL.md` (shipped docs, asserted by `public-contract.test.ts`).

**Publish-surface note for later phases:** `files: ["src", ...]` + `exports: "./src/index.ts"` means the
three `*.test.ts` files and **`src/test-setup.ts` (which mutates `globalThis.expect`)** are published in
the npm tarball — not reachable through `exports`, but on disk in every consumer's `node_modules`.
