# Phase 3 — Publish SDK + redeploy testnet FPC + playground (2026-06-30)

## P3a — rc.2 SponsoredFPC redeploy (the audit-predicted, confirmed-required step)
- Derived the rc.2 salt=0 FPC = **`0x1969946536f0c09269e2c75e414eef4e21a76e763c5514125208db33d7d944d7`** — ≠ the rc.1 `0x2613…7880` (bytecode `d7cd8d05`→`fc86f9b5` moved it). `node.getContract` → undefined → redeploy required, exactly as the dual audit predicted.
- Sepolia wallet `0xFcc2…F6F5` had 9.06 ETH (precondition met, no hold). Verified `--salt 0x0` derives the identical address as `new Fr(0)` before spending.
- **Two rc.2 breaking changes in the deploy scripts that tsc MISSED** (because `scripts/` isn't in `test:lint`'s scope — a real gap):
  1. `AztecAddress.fromBigInt` **removed** in rc.2 (#24230) → `AztecAddress.fromBigIntUnsafe`. Runtime `TypeError` at first run (before any spend).
  2. `proverOrOptions` no longer accepts a bare `WASMSimulator` (it wants `PrivateKernelProver | BBPrivateKernelProverOptions`) → wrapped in `new BBLazyPrivateKernelProver(new WASMSimulator())` (the WASM prover the SDK's AcceleratorProver also extends). Would've failed at *proving* (after the spend) — caught by typechecking the scripts before the re-run.
- Fixed both in `deploy-sponsored-fpc.ts` + `batch-fund-fpc.ts`; re-ran → bridged 1000 FJ from Sepolia, deployed account (WASM proof OK → fix #2 validated), deployed FPC, **claimed fee juice (block 401) → FPC funded.** `node.getContract` → DEPLOYED ✓.

## P3b — publish + redeploy
- `publish-testnet.yml --ref main` (no skip_sdk_publish): run success. **npm `testnet` = 5.0.0-rc.2**, `latest` = 4.3.1 (unchanged). Playground `deploy-app` ✓.

## P3c — verify + smoke
- Live bundle: HTTP 200, `VITE_AZTEC_SDK_VERSION="5.0.0-rc.2"`, v5 host, `CRS_CACHE_VERSION` rc.2 baked, **stale rc.1 FPC address absent** (derived at runtime).
- Sponsored-fee smoke (account deploy on testnet paying via the rc.2 SponsoredFPC, WASM proving): **PASS** — account deployed on testnet (block 409, reverted=false, txFee 0.94 FJ) paid by the rc.2 SponsoredFPC. The rc.2 fee path works end-to-end..

## "tsc didn't cover scripts/" — follow-up
P1's tsc gate passed but missed #1/#2 because `packages/playground/tsconfig.json` includes only `src`. Worth adding `scripts/**` to a typecheck so future `@aztec` bumps catch script breaks at compile time, not at runtime mid-deploy. (Noted for a follow-up; out of this bump's scope.)

LESSONS_FILE=implementations-plan/aztec-5.0.0-rc.2-2026-06-30/lessons/phase-3.md
