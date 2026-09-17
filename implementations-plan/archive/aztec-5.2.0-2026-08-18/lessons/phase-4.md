# Phase 4 lessons — live testnet proving smoke (2026-08-24)

**Outcome: ✓ ALL GATES GREEN. The owner's Sepolia key was never needed.**

## The FPC resolved itself
Owner pointed at `~/Projects/nulo` for the key (`packages/bridge-core/.env`: `PRIVATE_KEY` +
`SEPOLIA_RPC_URL` — mapped, never printed). Before using it, the read-only preflight re-ran:
the 5.2.0-derived salt=0 FPC (`0x2ece607a…e7315b`) was **deployed by a third party** in the
intervening five days, and the accelerated smoke paying fees through it proved it FUNDED too.
A2's whole contingency evaporated — re-checking blocked state before acting on it paid for
itself.

## Smoke evidence (all vs live testnet `5.2.0-nightly.20260815`, local headless accelerator)
- Node pre-flight: `test:live` 9/9 (earlier run; node reachable, nodeVersion defined).
- Playwright `smoke` project: WASM deploy PASS; Accelerated deploy PASS on retry (attempt 1 hit
  the 3-min wallet-init timeout cold; within the project's retry budget). 1.9m total.
- SDK e2e (full suite, `AZTEC_NODE_URL` + `ACCELERATOR_URL`): **10/10** in 32.6s — connectivity,
  proving with the `transmit`-present/`fallback`-absent phase-trail asserts (the native-path
  discriminator), and the testnet-only remote-network block.
- Token flow (A1's live arbiter): local-network spec's Accelerated group against the testnet dev
  server — **4/4 in 3.1m**, incl. "runs full token flow" (1.7m) and both mode-switch deploys.
  **aztec-standards@5.0.1 works with the 5.2.0 stack on the real network** — A1's hold vindicated.
- Run-isolation honored: ports 59833/5173 claimed in `~/.agents/ports.md`, services spawned
  detached, torn down by owned pgid, registry rows removed.

## Environment drift battles (5 days idle = 3 upstream drifts)
1. **Machine Bun 1.3.14 → 1.4.0**: playground's aztec.test.ts crashes under 1.4.0 inside Bun's
   worker_threads messaging (`port.on is not a function`). CI pins 1.3.14 (unaffected). Local
   remedy: pinned 1.3.14 into the scratchpad and ran all gates with it on PATH.
2. **Rebase over #468–#472**: index.md append-collision again (same resolution).
3. **foundryup rewritten in Rust upstream**: `FOUNDRY_DIR/bin` entries are now SYMLINKS into its
   `versions/` store (verified in foundry-rs/foundryup `src/install.rs::activate_bin`). The aztec
   installer `mv`s the symlink, `rm -rf`s its target, and its symlink check fails closed
   ("bundled binary 'forge' missing"). Failed attempt first: pre-seeding internal-bin from the
   pinned foundry-toolchain — the installer's `mv` CLOBBERS seeds with the doomed symlinks.
   Root-cause fix: download the installer script and patch its foundry `mv` → `cp -L`
   (dereference before cleanup), fail-open with a warning if the pattern disappears (upstream
   fix). Green first try. Cache salt bumped per recipe-change rule (now `…-cpl`).

## Final CI
All five Status checks SUCCESS on the cp-L head. A final comment-provenance cleanup pass
(global comment policy: no workflow references) + this close-out ship as the last commit.

LESSONS_FILE=implementations-plan/aztec-5.2.0-2026-08-18/lessons/phase-4.md
