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
- `node.getContract(addr)` against `v5.testnet.rpc.aztec-labs.com` → at recon time **undefined (NOT yet published)**.
- **Conclusion (recon-time):** no pre-deployed canonical salt=0 sponsored FPC on v5 testnet *yet*. **This did NOT block the SDK-only release** — P4 parity runs on a local 5.0 sandbox (its own canonical FPC), and the npm publish is independent.
- **UPDATE (2026-06-18, later same session):** we then deployed + funded the canonical salt=0 SponsoredFPC on v5 ourselves — address `0x261366b3…7880`, claim mined block 1387. So salt=0 **is now published + funded on v5**, and the playground/e2e resolve it everywhere with no `SPONSORED_FPC_SALT` (that env var was subsequently removed — see `implementations-plan/fpc-salt-removal-docs-2026-06-18`). The "needs a custom salt" caveat above is superseded.

## Notes
- The "is_valid_version rejects non-alphanumeric" SDK comment (`accelerator-prover.ts:382`) confirmed false earlier; comment corrected in P2.
- 5.0 uses subpath exports (`@aztec/aztec.js/fields`, `@aztec/stdlib/contract`, …) — the bare `@aztec/aztec.js` root isn't an entrypoint.

LESSONS_FILE=implementations-plan/aztec-5.0.0-2026-06-18/lessons/phase-0.md
