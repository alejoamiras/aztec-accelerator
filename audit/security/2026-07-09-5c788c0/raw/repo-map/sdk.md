# Repo Map — packages/sdk (@alejoamiras/aztec-accelerator)

TS SDK that runs in the browser/dApp, detects a local accelerator on :59833, and offloads ZK proving. Falls back to WASM. Sensitive data: the **proving witness** (private ZK inputs) which leaves the JS heap on POST /prove.

## Modules
- `src/index.ts` (9) — barrel: `AcceleratorProver` + 6 types. `AcceleratorTransport` NOT exported.
- `src/lib/accelerator-prover.ts` (390) — prover (extends `BBLazyPrivateKernelProver`); health-detect, serialize witness, POST /prove, decode proof, WASM fallback.
- `src/lib/accelerator-transport.ts` (158) — all network I/O: URL build, dual HTTP/HTTPS /health probe, protocol pin, status cache, POST /prove.
- `src/lib/types.ts` (89) — phases, config, `AcceleratorStatus` union.
- `src/lib/logger.ts` (3) — LogTape logger `["aztec-accelerator","prover"]`.

## Network calls
- `GET http://127.0.0.1:59833/health` and `GET https://127.0.0.1:59834/health` in parallel, `Promise.any` first-wins (transport:112-138). 2s timeout, 1 retry. No body/creds.
- `POST {baseUrl}/prove` (transport:144-157) — **witness-bearing.** baseUrl is https iff HTTP probe failed (Safari case), else `http://` (transport:80-86). Body = `Uint8Array` of msgpack `PrivateExecutionStep[]`. Headers: `content-type: application/octet-stream`, conditional `x-aztec-version`. 10min timeout, retry:0.

## Trust boundary / privacy anchors
- **Witness normally POSTed over cleartext HTTP** to loopback on Chrome/Firefox (HTTP probe wins race). HTTPS only when HTTP fails (transport:82-86, prover:162-165).
- 403 from /prove => "origin denied" => silent WASM fallback (prover:311-324). Origin authz is SERVER-side, not SDK.
- Logs: baseUrl logged (prover:294-296); 403 response body `{error,message}` logged (prover:318-321). Witness bytes NOT logged.
- Env: `AZTEC_ACCELERATOR_PORT` / `_HTTPS_PORT` feed URL ports (prover:109-121), NaN-guarded.

## Witness data flow
`createChonkProof(executionSteps)` -> if forceLocal/unavailable/denied => WASM (`super.createChonkProof`, in-process, no egress). Else `serializePrivateExecutionSteps` (@aztec/stdlib/kernel) -> msgpack -> Uint8Array -> `postProve` -> POST /prove. Response `{proof: base64}` -> decode.

## Deps
`@aztec/bb-prover/client/lazy` (superclass/WASM), `@aztec/stdlib/kernel` (witness serializer), `@aztec/stdlib/proofs`, `@aztec/simulator/client` (lazy), `ky` (HTTP), `ms`, `@logtape/logtape`.
