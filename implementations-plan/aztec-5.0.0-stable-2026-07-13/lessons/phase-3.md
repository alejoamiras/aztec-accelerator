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

## (b) Pre-publish smoke — ✅ FULL PASS (2026-07-13 ~20:43 UTC, dev:testnet + WASM path, driven via Playwright MCP)

- Node check via the fixed RPC probe: "Aztec node version: 5.0.0" ✓; wallet created in ~2s (**ephemeral store init/sync cost: negligible** — the plan's inference confirmed); FPC `0x0628…3fe1` registered ✓.
- Account deploy: 19.5s (create 0.5 / simulate 4.2 / prove+send 14.8) → tx `0x194576…34d82d`, fee sponsored by the new FPC.
- **Token flow: 88.5s total** — Bob deploy 26.4s (tx `0x0d21b9…313792`), TokenContract deploy 19.3s (`0x153766…9668b6`, tx `0x102e9c…2564c8`), mint 1000 ACEL to private 18.6s (tx `0x2e5b78…2e08e52d`), **private transfer 500 ACEL Alice→Bob 19.4s** (tx `0x0e784e…fd937d9`) — the note-tagging/discovery surface the changelog changed — and **balances verified Alice 500 / Bob 500**.
- Expected noise only: accelerator health-probe connection-refusals (no accelerator running), "SDK unknown" version-mismatch warn (dev mode doesn't bake VITE_AZTEC_SDK_VERSION).

## PR #378 CI round: two more REAL finds (the gates keep earning their keep)

1. **Mocked-suite contract drift (my own miss):** I ran the Playwright mocked gate BEFORE making the /status→RPC fix, then didn't re-run it — CI caught it. The specs stubbed GET `/aztec/status`; rewritten to stub the `node_getNodeInfo` POST (health probe fulfilled, all other RPCs 500 so wallet init fails gracefully). Lesson: any source change after a gate run invalidates the gate — re-run before push, no exceptions.
2. **Production-only sqlite3 wasm 404 (would have broken EVERY real user of the deployed 5.0.0 playground):** the emscripten loader inside `@aztec/sqlite3mc-wasm` resolves `sqlite3.wasm` via a dynamic `locateFile` fallback that bundlers can't rewrite → at runtime the worker requests bare `/assets/sqlite3.wasm` (unhashed); rollup only emits the hashed asset; the SPA fallback answers with index.html → `WebAssembly.compile` MIME error → wallet init dead in production builds. Fix: a tiny build plugin emits UNHASHED copies of `sqlite3.wasm` + `sqlite3-opfs-async-proxy.js` alongside the hashed ones. **Repro fortune:** another agent's 5.0.0 sandbox was listening on :8080 and `vite preview` inherits `server.proxy` — so the local production smoke ran a REAL wallet init (read-only use of their node; no teardown, per run-isolation). Without that accident, the bug would only have surfaced at the post-deploy acceptance smoke.
3. Also hit the classic squash-then-continue DIRTY conflict on the follow-up PR (branch continued past #376's squash-merge) → rebased `--onto origin/main`, force-pushed own branch. And a `git add -A` had swept Playwright-MCP session artifacts into the fix commit → removed + `.playwright-mcp/` gitignored.

## (c)–(e) Publish → acceptance → promote — ✅ ALL GREEN (2026-07-13 ~21:20 UTC)

- **(c) Publish dispatch**: run `29285158631` at `main=507efd9d…` (recorded; nothing else queued — only unrelated open PRs #375/#374). All jobs green (incl. the new publish-time native-bb e2e via `build_accelerator: true`). SDK published as clean **`5.0.0`** on `testnet`; GitHub release `--latest=false` preserved.
- **(d) Acceptance**:
  - Live bundle bakes `5.0.0`, HTTP 200; SDK 5.0.0 / node 5.0.0 matched; CRS-version guard fired on first visit (evicted the rc.2 CRS).
  - **Registry-artifact test PASS**: fresh temp project, `npm i @alejoamiras/aztec-accelerator@5.0.0`, import OK, `AcceleratorProver` constructs (publish-time exports mutation sound).
  - **Released-1.0.6 native smoke PASS** (the production path end-to-end): app launched from /Applications (cache had rc.2 but NO 5.0.0) → live playground detected it → prove triggered **runtime download + digest-verify of bb 5.0.0** (health `available_versions` gained "5.0.0") → native prove → account deployed on testnet in 32.6s incl. the download (tx `0x13d741…849c34`). Gotcha: Playwright's fresh Chromium needed `context.grantPermissions(["local-network-access"])` — the Chrome 142+ LNA prompt (documented in #373) blocks the localhost probe in automation.
  - **WASM pass on live site**: 16.4s deploy (tx `0x109ef2…547b78`).
  - **Safari pass (owner-verified): PASS + deploy verified.**
- **(e) Promote**: `promote-latest.yml` dispatched with `version=5.0.0` → dist-tags now **`latest: 5.0.0`, `testnet: 5.0.0`** (nightlies untouched).
- `scripts/.env` + `.env.local` **deleted** at completion, per plan.

LESSONS_FILE=implementations-plan/aztec-5.0.0-stable-2026-07-13/lessons/phase-3.md
