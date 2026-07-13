# Phase 3 lessons — FPC redeploy → smokes → publish → promote (2026-07-13, in progress)

## (a) Fail-closed pre-flight — ✅ all green (run before any signing)

- `node_getNodeInfo` on `v5.testnet.rpc.aztec-labs.com`: `nodeVersion=5.0.0` ✓, `l1ChainId=11155111` (Sepolia) ✓, `rollupVersion=1821665230` (unchanged from rc.2 — the stable upgrade was node-side, not a rollup redeploy).
- Derived 5.0.0 salt=0 SponsoredFPC address: `0x0628377e98bca5913dc86765ad0758f7b7aa83eac49079c6fba125807b393fe1` (moved off rc.2's `0x1969…44d7`, as the artifact-sha diff predicted).
- `node.getContract(addr)`: **ABSENT** — clean `undefined`, explicitly distinguished from an RPC error (the probe throws on error rather than treating it as absence). Deploy required.
- Sepolia wallet (key via the gitignored `scripts/.env`, copied from the canonical clone; value never printed/echoed): `0xFcc2238319aC360e985f1736aBB3df6251DAF6F5`, balance **8.76 ETH** — ample gas, no funding hold.
- `BRIDGE_AMOUNT` set explicitly: `1000000000000000000000` wei = **1000 FJ** (rc.2 parity, user-approved). The script's `bridgeTokensPublic(recipient, amount, mint=true)` takes base units; leaving it unset would use the portal manager's default — plan requires explicit.
- Deploy launched: `bun run scripts/deploy-sponsored-fpc.ts --salt 0x0` (flag mandatory — script defaults to `Fr.random()`), env sourced from `scripts/.env` inside the command's own shell (no key material in any command line / shell history).

Tooling gotcha: scripts must run from inside `packages/playground` — a copy under the session scratchpad resolved `@noble/hashes` from bun's global cache with a broken nested path (`Cannot find module '@noble/hashes/crypto'`). Workspace-relative execution only.

## (a) Deploy + fund — ✅ complete (2026-07-13 ~20:35 UTC)

- SponsoredFPC deployed at salt 0: `0x0628…3fe1`, L2 block 346, tx `0x2312ef57…6ff49ed5` (aztecscan link in run log).
- Funding: minted+approved+deposited **1000 FJ** on L1 (txs nonce 5051-5053, all `status: success`; message leaf 284672) → claimed on L2 block 350, tx `0x245db5b4…f2e428b5` (claim fee 1.25 FJ paid by the deployer account, not the FPC).
- **Post-flight (explicit, recorded):** `node.getContract(0x0628…3fe1)` → **DEPLOYED**; Fee Juice public balance of the FPC read via `getPublicStorageAt` (balances-map slot derivation) → **1000000000000000000000 = exactly 1000 FJ**.
- Ops gotchas: two failed launches (exit 127) before the successful one — the background shell does NOT inherit the session's cwd reliably; `source scripts/.env` silently targeted the worktree ROOT's `scripts/` dir earlier too (the pre-flight probes "worked" against that same wrong path, which masked it). Everything relaunched with fully absolute paths; the stray root-`scripts/.env` was moved to `packages/playground/scripts/.env` (gitignored, verified; never printed).

## (b) Pre-publish smoke — found a REAL 5.0.0 break before any publish (the smoke earning its keep)

- Booting `dev:testnet` for the smoke surfaced: **the 5.0.0 node returns 405 to a plain GET `/status`** (verified direct against `v5.testnet.rpc.aztec-labs.com`, not a proxy artifact) → `checkAztecNode()` reported unreachable → action buttons disabled. **This means the LIVE rc.2 playground is broken against today's node** — the track-now urgency argument turned out true, just via the health check rather than the fee path.
- Fix: `checkAztecNode` now probes via the `node_getNodeInfo` JSON-RPC POST it already used for the version (single round-trip); the 4 unit tests rewritten to the new contract. Follow-up PR after #376 (which had already auto-merged).
- Dev-server env gotchas: vite's `loadEnv` did NOT see inline process-env in this setup — `AZTEC_NODE_URL` had to go into a gitignored `packages/playground/.env.local`. Also the root `node_modules/.bin/vite` resolves to a hoisted **vite 8.0.1** (the version this repo blocks) — always spawn `packages/playground/node_modules/.bin/vite` (7.3.1). Port 5273 claimed in `~/.agents/ports.md`.

## Pending
- (b) pre-publish dev:testnet transfer smoke (gates the publish)
- (c) publish-testnet dispatch (post-merge, autonomous per Ask 1)
- (d) registry-artifact install test + released-1.0.6 native smoke + Safari + WASM
- (e) promote-latest dispatch (same-day per Ask 3)
- Delete `scripts/.env` at P3 completion.
