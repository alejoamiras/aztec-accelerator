# Phase 3 — Playground migration (2026-06-18)

The playground is **not** in the CI typecheck gate (`test:typecheck` only runs the SDK; the playground relies on `vite build` + Playwright). So I ran a raw `tsc --noEmit` to surface 5.0 type breaks. Real breaks found + fixed in `src/aztec.ts`:

1. **`DeployMethod.address` removed → `await getAddress()`** (the changelog's DeployMethod-construction-time change). Two sites: `aztec.ts:564` (`const address = (await tokenDeploy.getAddress()).toString()`) and `:656` (`await TokenContract.at(await tokenDeploy.getAddress(), ...)`). The deployer is locked by the preceding `executeStep` send, so `getAddress()` resolves.
2. **mined-but-reverted handling (codex finding).** v5's receipt union exposes `hasExecutionReverted()` / `hasExecutionSucceeded()` on every variant (`@aztec/stdlib/dest/tx/tx_receipt.d.ts`). `waitForTx` now throws on `receipt.hasExecutionReverted()` instead of treating any non-pending/non-dropped receipt as success. (`isPending`/`isDropped` confirmed still present — the brief's "add `.isMined()`" was wrong; the code already narrowed.)
3. **`dev:testnet` host → `v5.testnet.rpc.aztec-labs.com`**; dropped the stale **v4** `SPONSORED_FPC_SALT=0x2a0f…` (its FPC doesn't exist on v5; salt=0 canonical isn't on v5 either — see phase-0). The live smoke supplies the real v5 FPC salt via env.

The FeeJuice-`0x05`→`0x03` script fixes (`deploy-sponsored-fpc.ts`/`batch-fund-fpc.ts`) were **left out** — contingency-only (self-hosted FPC), per the SDK-only scope.

**Gate:** PASS — `aztec.ts` tsc errors gone (6 remaining = pre-existing bun:test/css config noise, present on main, not gated); `vite build` clean; `bun run lint` exit 0 (the lone `firstCallMs` warning is pre-existing on main); 8 mocked Playwright pass; `bun run test` exit 0 (73 unit + 6 scripts).

LESSONS_FILE=implementations-plan/aztec-5.0.0-2026-06-18/lessons/phase-3.md

## Follow-up: local-network E2E caught a real 5.0 deploy bug (2026-06-18)
The mocked gate passed, but the full-stack **Local Network E2E** (playground deploying against a local 5.0 sandbox, app.yml) FAILED:
```
✗ Deploy failed: Assertion failed: Failed to get a note 'assert(self.is_some())'
```
Not a flake. The SDK e2e's `deploySchnorrAccount` (`from: NO_FROM`) passed, but the playground's `deployTestAccount` used `from: registeredAddresses[0]` (a funded signer) on local sandbox.

**Codex consult (xhigh, session in /tmp/codex-98k5s5Cz) — source-verified verdict:** in 5.0, `DeployAccountMethod.prepareDeployOptions()` sets `sendMessagesAs = deployedAddress` ONLY when `from === NO_FROM`; `senderForTags = sendMessagesAs ?? (from===NO_FROM ? undefined : from)` (`@aztec/wallet-sdk/base_wallet.ts`). So a signer `from` tags the new account's constructor notes as that signer → the new account can't discover its own notes → "Failed to get a note".

**Fix applied:** `deployTestAccount` → `from: NO_FROM` (drop the `proofsRequired ? NO_FROM : registeredAddresses[0]` ternary) + drop the now-redundant `additionalScopes: [accountManager.address]` (DeployAccountMethod injects it). Same `additionalScopes` cleanup on the `runTokenFlow` bob deploy (already NO_FROM). Verified: no new tsc errors, build clean, lint exit 0; CI local-network E2E re-verifies. Also dropped a dead `await TokenContract.at` (sync in 5.0, code-review nit).

LESSONS_FILE=implementations-plan/aztec-5.0.0-2026-06-18/lessons/phase-3.md
