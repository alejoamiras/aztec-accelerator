# PR-4 — F-05 SDK doc-sync + F-06 AcceleratorTransport

Branch: `quality/pr4-sdk` off `main@c3569d9` (started in PARALLEL while PR-3 #335 is in CI — PR-4 is
SDK-only, zero overlap with PR-3's Rust, so no conflict). PR-3 codex post-impl came back **clean**.

## Log
- **F-05 ✓** (edfd5f5): barrel `index.ts` now exports `AcceleratorProtocol` (was missing → the documented
  import failed); README's obsolete flat `interface AcceleratorStatus` → the discriminated union + a
  `setForceLocal` method row; SKILL phase table gains the `denied` phase. New `src/lib/public-contract.test.ts`:
  type-imports pin the barrel (a dropped export becomes a `tsc --noEmit` error) + asserts README/SKILL/MIGRATION
  markers (no flat interface, `denied` present, `AcceleratorProtocol` referenced). `tsc --noEmit` exit 0;
  31 SDK unit tests green. Biome reordered the test imports (hook).

## F-06 plan (next — the last finding) — extract `AcceleratorTransport`
`accelerator-prover.ts` (494 LOC). Transport-relevant members (verified by grep):
- state: `#acceleratorProtocol` (:166), `#statusCache` (:167), host/port config (`setAcceleratorConfig` :204 resets both).
- `#acceleratorBaseUrl` getter (:224); `checkAcceleratorStatus` (:235, TTL cache hit → `#probeAndParseHealth`).
- `#probeAndParseHealth` (:252-374) — the dual `fetch(httpUrl)`/`fetch(httpsUrl)` `Promise.any` probe (:254-272) +
  protocol set/reset (:302/:314/:371) + cache write (:272).
- `createChonkProof` (:376) → `ky.post(`${baseUrl}/prove`)` (:414).
Approach: new **non-exported** `class AcceleratorTransport` in `src/lib/accelerator-transport.ts` owning URL
construction + protocol negotiation + the status cache + one error model; **keep the parse→`AcceleratorStatus`
discriminated-union construction in the prover** (domain logic — opus). **Unify on `ky` for both** /health +
/prove (health uses `throwHttpErrors:false`) to preserve the thrown-error surface (codex). Route every
`#acceleratorProtocol` mutation through `transport.setProtocol`. **No public API change** (`tsc --noEmit` + the
F-05 doc-sync test are the gate). Safety net: the 27 existing SDK tests mock `globalThis.fetch` (and `ky` rides
on fetch) → they exercise BOTH stacks through the new transport unchanged. Add 1-2 `AcceleratorTransport` unit
tests (baseUrl http-vs-https-after-negotiation). **Required PR-4 gate:** `bun run --cwd packages/sdk build`
(not just `tsc --noEmit`) — the publish artifact is `dist/`, source-only typecheck can't prove it.

## Next
- implement F-06 → commit → push PR-4 (F-05+F-06) → codex post-impl → CI green → merge.
- both PR-3 + PR-4 merged → /code-review max --fix (the net diff) → final codex post-impl → /harden security.
