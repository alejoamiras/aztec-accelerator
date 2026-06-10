# Repo map — SDK (`@alejoamiras/aztec-accelerator`)

5 source files (~597 LOC) + 3 test files (~852 LOC). TypeScript strict, Bun, `ky` HTTP client.

## Module inventory
| Path | Purpose | LOC |
|------|---------|-----|
| `src/index.ts` | Public barrel (AcceleratorProver + 6 types) | 9 |
| `src/lib/accelerator-prover.ts` | Main class; extends `BBLazyPrivateKernelProver`; proving lifecycle, health probe→status mapping, phase callbacks, WASM fallback | 440 |
| `src/lib/accelerator-transport.ts` | Network I/O: dual HTTP/HTTPS `/health` probe w/ retry, protocol pinning, 10s status cache, `/prove` POST | 135 |
| `src/lib/logger.ts` | logtape singleton | 3 |
| `src/test-setup.ts` | Bun expect equality-tester patch for @aztec/foundation | 10 |

## Public surface (index.ts)
`AcceleratorProver` (class) + types: `AcceleratorConfig`, `AcceleratorPhase` (9-member string union), `AcceleratorPhaseData`, `AcceleratorProtocol` (`"http"|"https"`), `AcceleratorProverOptions`, `AcceleratorStatus` (discriminated union on `available` — Q12 shape, pinned by public-contract.test.ts). **Types must stay stable (npm consumers).**

## Long-function hotspots
- **`#probeAndParseHealth()` — accelerator-prover.ts:233–328 (~96 lines).** The fattest method. Nested try/catch + JSON parse + dual-protocol (legacy single-version vs multi-version) + version-matching + the AcceleratorStatus construction. Decision tree (when to pin protocol, error vs offline vs version-mismatch) hard to follow. Comment at :230 says it was already extracted from something bigger (Q5) — still the fattest.
- **`createChonkProof()` — accelerator-prover.ts:330–399 (~70 lines).** Mixes proving orchestration + phase emission + fallback routing + `x-prove-duration-ms` header extraction. Hard to trace which path emits which phase.
- **`probeHealth()` — accelerator-transport.ts:89–114.** `Promise.any([http,https])` + embedded retry/delay race logic.

## Similarity / duplication candidates (highest-value, quality)
1. **Manual AcceleratorStatus union construction in ≥5 places** — accelerator-prover.ts:248–253 (error), :302–316 (version-mismatch), :281–299 / :317–323 (available:true). No factory/constructor helper → adding a field to one variant risks missing the others. (Fowler: Duplicate Code / absent Factory.)
2. **Protocol-pinning suppression rule duplicated** — "don't pin protocol on error" lives at :256 (pin on OK), :268 + :325 (clear on parse-fail / catch-all). Rule not centralized → future edit may pin prematurely. (Temporal coupling / scattered state machine.)
3. **Error→fallback mapping split across two files** — probeHealth exception→offline (transport) vs createChonkProof 403→denied/fallback (prover, :367–385). Similar shape, two homes.
4. **baseUrl computation** — transport exposes `baseUrl` getter (:58–62) but prover also reasons about `http(s)://host:port` (:354 logs it). Minor.

## House conventions
- Status checks return the discriminated union, never throw; `createChonkProof` throws/propagates non-403 network errors, catches 403→fallback.
- `ky` config: `{ retry:0, throwHttpErrors:false, timeout }` — 2s/probe, 10min/prove.
- Phase = string-literal union (not enum); emitted via `onPhase` callback.
- Two coexisting accelerator protocols (legacy `aztec_version` exact-match; multi-version `available_versions[]`).

## Test surfaces
- `accelerator-prover.test.ts` (690 LOC, 30 tests): lifecycle, fallback (offline/403/mismatch), phase order, caching, the Q12 discriminant-invariant guard.
- `accelerator-transport.test.ts` (122 LOC, 7 tests): protocol negotiation, cache, probeHealth race.
- `public-contract.test.ts` (40 LOC, 3 tests): barrel-export + README/MIGRATION doc-sync guard (F-05).
- **Brittleness note (quality):** offline-cache test uses a hardcoded 50ms wall-clock threshold (:498–515) → timing-fragile on slow CI. No direct probeHealth-retry unit test; no `/prove` timeout test.

## Coupling
- `AcceleratorProver` constructs `AcceleratorTransport` 1:1 in ctor (:196) — not injectable (only matters for testability; tests mock at the network layer instead).
- `createLazySimulator()` Proxy (:99–137) forwards with a `then`-symbol exclusion to avoid thenable confusion — non-obvious.
