# Repo map — SDK (quality audit)

**Package:** `@alejoamiras/aztec-accelerator` (published TS lib, AGPL-3.0). Scope: `packages/sdk/src`.

## Inventory
| File | LOC | Purpose |
|---|---|---|
| `src/index.ts` | 8 | public re-exports: AcceleratorProver + 5 types |
| `src/lib/accelerator-prover.ts` | 434 | core: extends `BBLazyPrivateKernelProver`; HTTP/HTTPS health probe, `/prove` routing, phase callbacks, WASM fallback, status cache |
| `src/lib/logger.ts` | 3 | LogTape factory |
| `src/test-setup.ts` | 10 | bun:test shim |

## Public surface
`new AcceleratorProver(opts?)`, `checkAcceleratorStatus()`, `createChonkProof(steps)`, `setAcceleratorConfig()`, `setOnPhase()`, `setForceLocal()`.

## Patterns
- `extends BBLazyPrivateKernelProver`; overrides `createChonkProof`, delegates to `super` for WASM.
- Proxy-based lazy WASMSimulator (`createLazySimulator`, ~L90-103; blocks `then`/symbols).
- Status cache, 10s TTL. `Promise.any()` dual-protocol HTTP+HTTPS race (Safari mixed-content). Retry-once + 2s timeout.
- Phase callbacks: detect→serialize→transmit→proving→proved→receive (or fallback/denied).

## Tests
`src/lib/accelerator-prover.test.ts` (629 LOC, ~29 unit tests) + `e2e/` (real node/wallet, skipIf). Good coverage.

## Exclude
`dist/`, `*.test.ts`, e2e fixtures.

## First-glance smells (locations only)
- **Long Method:** `checkAcceleratorStatus()` ~L202-319 (118 LOC — probe + retry + dual-protocol + legacy-vs-multi-version + cache).
- **Boilerplate:** 17× `#onPhase?.(...)` ad-hoc calls across `createChonkProof` + fallback paths (no phase state-machine).
- Magic numbers (2000/1000/10_000ms, "10 min") — but already named consts.
