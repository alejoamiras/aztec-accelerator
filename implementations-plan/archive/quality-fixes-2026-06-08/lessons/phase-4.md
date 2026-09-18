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

## F-06 — implemented (e9ec00b), PR-4 #336 in CI
New **internal** `class AcceleratorTransport` (`src/lib/accelerator-transport.ts`, NOT barrel-exported)
owns all accelerator network I/O: endpoint/URL construction, the dual HTTP/HTTPS `/health` probe +
protocol negotiation (`setProtocol`/`baseUrl`), the status cache (`getFreshCachedStatus`/`cacheStatus`,
TTL moved here), and the `/prove` POST (`postProve`). `AcceleratorProver` keeps the domain logic:
parsing `/health` → the `AcceleratorStatus` union, and the `403`→denied interpretation. Runtime dep is
one-directional (prover imports the transport *value*; transport imports prover *types* only → erased,
no cycle). `setAcceleratorConfig` → `transport.configure` (resets protocol + status cache).

### Two non-obvious gotchas (both verified before the switch)
1. **ky-for-/health must pass `{ retry: 0, throwHttpErrors: false, timeout: 2000 }` to stay behavior-
   identical to the old raw `fetch`.** Without `throwHttpErrors:false`, a 500 would *throw* → both
   probes reject → `Promise.any` AggregateError → mapped to `offline` (WRONG — must be reason:`error`).
   Without `retry:0`, ky's default GET-retry would re-fetch a 500 **3×** and stack on top of the single
   explicit retry. With both flags, ky resolves any HTTP status / rejects only on network+timeout —
   exactly raw-fetch semantics. The 31 prover tests (which mock `globalThis.fetch`, and ky rides on it)
   pass unchanged → behavior-preservation proof. Verified against the offline-retry-timing, 500-non-ok,
   malformed-JSON-200, Safari-mixed-content, and protocol-winner tests specifically.
2. **`postProve` body must be typed `Uint8Array<ArrayBuffer>`, not bare `Uint8Array`.** Under TS6's
   generic `Uint8Array<TArrayBuffer>`, a bare param widens to `Uint8Array<ArrayBufferLike>` which is NOT
   assignable to ky's `BodyInit` (wants ArrayBuffer-backed, excludes SharedArrayBuffer). The old inline
   `new Uint8Array(msgpack)` was already `<ArrayBuffer>`; the extraction surfaced the variance.

Also: `res.json<{proof}>()` (ky's typed shorthand) → `(await res.json()) as {proof:string}` since
`postProve` returns a plain `Response`. Added 10 `AcceleratorTransport` unit tests (baseUrl negotiation,
cache TTL, `configure` reset, probe winner / mixed-content / non-2xx-resolves / offline-rejects).
Gates green locally: `tsc --noEmit`, `bun run build` (dist artifact), 41 unit tests, biome.
`LESSONS_FILE=implementations-plan/quality-fixes-2026-06-08/lessons/phase-4.md`

## Next
- PR-3 #335: clippy fix pushed (gate `CertPaths::remove` to macOS — Linux dead-code); CI re-running.
- PR-4 #336: F-05+F-06 pushed; CI running (sdk.yml + app.yml, since playground depends on the SDK).
- both green → mark F-06 ✓ in plan.md + merge both → /code-review max --fix (net diff) → final codex
  post-impl → /harden security. (plan.md ✓ marks + this lessons finalization land in the closing docs PR,
  since main is branch-protected and re-pushing a green branch just to tweak docs wastes a CI cycle.)
