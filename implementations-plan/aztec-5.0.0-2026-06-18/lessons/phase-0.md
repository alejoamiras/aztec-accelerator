# Phase 0 — Recon (2026-06-18)

## Signature-diff table (SDK-imported symbols, 4.3.1 → 5.0.0-rc.1)
Verified empirically: `bun run --cwd packages/sdk test:lint` (tsc --noEmit, strict) **passes clean** against the installed 5.0.0-rc.1 types. So every SDK-imported symbol is **source-compatible (none)**:

| Symbol | Source | 5.0 change | Action |
|---|---|---|---|
| `BBLazyPrivateKernelProver` | `@aztec/bb-prover/client/lazy` | ctor `super(simulator)` + `createChonkProof(steps)` still compatible | none (highest-risk inference CONFIRMED safe) |
| `WASMSimulator`/`CircuitSimulator` | `@aztec/simulator/client` | `new WASMSimulator()` default-constructs | none |
| `serializePrivateExecutionSteps`/`PrivateExecutionStep` | `@aztec/stdlib/kernel` | type-compatible | none |
| `ChonkProofWithPublicInputs` | `@aztec/stdlib/proofs` | `fromBuffer`/`toBuffer` intact | none |

SDK unit tests: **45 pass / 0 fail** (incl. the `createChonkProof` prototype spy). SDK build: clean.

## v5 canonical FPC check (decides the live-smoke FPC source)
- Derived the salt=0 `SponsoredFPCContract` address from the **5.0 artifact**: `0x261366b3c0a9b4c30864629556cf282be409e6822b1f3a065fcb7e34f36d7880`.
- `node.getContract(addr)` against `v5.testnet.rpc.aztec-labs.com` → **undefined (NOT published)**.
- **Conclusion:** there is no pre-deployed *canonical salt=0* sponsored FPC on v5 testnet. The live-network smoke (P5) needs the v5 testnet's actual sponsored-FPC salt (as the old testnet used a custom `SPONSORED_FPC_SALT=0x2a0f…`), or a self-deploy+fund (Appendix-C-style `deploy-sponsored-fpc.ts`). **This does NOT block the SDK-only release** — P4 parity runs on a local 5.0 sandbox (its own canonical FPC), and the npm publish is independent. Flagged to the user as a P5 external dependency.

## Notes
- The "is_valid_version rejects non-alphanumeric" SDK comment (`accelerator-prover.ts:382`) confirmed false earlier; comment corrected in P2.
- 5.0 uses subpath exports (`@aztec/aztec.js/fields`, `@aztec/stdlib/contract`, …) — the bare `@aztec/aztec.js` root isn't an entrypoint.

LESSONS_FILE=implementations-plan/aztec-5.0.0-2026-06-18/lessons/phase-0.md
