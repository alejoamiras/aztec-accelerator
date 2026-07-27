# Phase 3 lessons — FPC → smokes → publish → promote (2026-07-16, in progress)

## (a) Pre-flight + deploy + fund — ✅ complete

- Pre-flight (fail-closed): `nodeVersion=5.0.0` (EXPECTED this cycle — 5.0.1 is operator-optional), `l1ChainId=11155111`, `rollupVersion=1821665230` unchanged; derived 5.0.1 salt-0 SponsoredFPC `0x1441491b59934ec64f8c98f17c91f23c01ca2a45dbb35caf123146ec76f9970c`; `getContract` → explicit **ABSENT** (not an RPC error). Wallet `0xFcc2…F6F5`: 8.52 ETH gas. `scripts/.env` recreated from the canonical clone (gitignored verified; delete at P3 end).
- Deploy+fund with explicit `BRIDGE_AMOUNT=1000000000000000000000`: deployed; bridged; **claimed in block 3918** (tx `0x1e083e2c…ac978098`, claim fee ~1.29 FJ paid by the deployer).
- **Post-flight (explicit): getContract → DEPLOYED; FeeJuice public balance = exactly 1000 FJ.**

## P2 interlude: the newly-enabled token spec found a REAL pre-5.0 break (not a standards regression)

- First #395 CI run: local-network token flow failed at the token DEPLOY with `Failed to get a note` — failing selector `0x9d57a239` = the ACCOUNT ENTRYPOINT, i.e. alice herself, before the token contract even matters.
- Root cause: the flow picked `registeredAddresses[selectedAccountIndex]` — on the sandbox that's a GENESIS test account, whose account-contract notes were created by another PXE at genesis; our ephemeral wallet never discovers them. This path was `test.skip`'d since the 4.x nightlies ("~7 min WASM" note) — it was never exercised in ANY 5.x cycle, and the sandbox demo's token flow was broken the whole time.
- Fix (`14b5faf`): `state.sessionAddresses` tracks accounts deployed THIS session; senders (alice AND bob) come only from it, with a clear error otherwise — uniform with the live-network path, which already required a prior deploy. Sandbox ready-copy updated. All local gates re-run green.
- The CI spec order (deploys-account before token-flow, shared page) supplies the session account naturally.

## (b) Pre-publish smoke — ✅ FULL PASS (WASM, dev:testnet, port 5281)

- Session alice deployed 31.8s (tx `0x03b28fae…69bac02`) via the new FPC.
- **Standards token first live execution**: Bob 13.4s (tx `0x1e3c1a3b…4bd21ba1`) → TokenContract deployed 20.8s at `0x2bea03a5…1a8bbf89` (tx `0x007e18e7…6ca927ed`) → mint 1000 (tx `0x2d524f7e…936ec6fc`) → private transfer 500 (tx `0x24881fb6…c63205a9`) → **Balances — Alice: 500, Bob: 500** ✓. Total 83.1s. `constructor_with_minter` + `auth_contract=ZERO` + nonce-0 `transfer_private_to_private` all behave exactly as source-verified. **No contingency needed.**
- Kept the smoke WASM-only deliberately: preserves the released app's pristine bb cache so P3d's runtime-download test stays genuine.

## (c) Publish dispatch — in flight

- Dispatched `publish-testnet.yml --ref main` at **main=36994ec** (#395 squash); confirmed `in_progress` (one-queued-run caveat satisfied; only unrelated PRs open). Run `29516936140`.

## (d)+(e) Acceptance + promote — ✅ ALL GREEN (2026-07-16 ~17:20 UTC)

- Publish run `29516936140` green end-to-end (native-bb e2e gate → publish → deploy). npm `testnet` = **5.0.1** (clean base); live bundle `index-Dy8TPl3E.js` bakes 5.0.1; unhashed sqlite assets still `no-cache`/`application/wasm` after the redeploy.
- **Registry-artifact test PASS** (fresh temp install of 5.0.1; `AcceleratorProver` constructs).
- **Released-1.0.6 native smoke PASS**: cache had no 5.0.1 → runtime-downloaded it (health `available_versions` gained "5.0.1") → native deploy 28.6s, `fellBack: false`.
- **LIVE-BUNDLE STANDARDS TOKEN FLOW (required-before-promote): PASS** — 76.7s native, token at `0x0f11e293…22663e13`, **Balances — Alice: 500, Bob: 500**.
- WASM pass: 17.6s deploy. **Safari (owner-verified): PASS + flow verified.**
- Promote run `29519222516` success → dist-tags `latest: 5.0.1`, `testnet: 5.0.1` (one npm-registry propagation lag of ~30s between the run's own confirmation and the public read — the run log is the truth, wait before panicking).
- `scripts/.env` + `.env.local` deleted; smoke port released earlier.

LESSONS_FILE=implementations-plan/aztec-5.0.1-2026-07-16/lessons/phase-3.md
