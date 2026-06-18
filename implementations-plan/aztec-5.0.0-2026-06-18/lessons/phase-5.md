# Phase 5 — Full sweep + manual v5 smoke (2026-06-18)

## Automated sweep — GREEN in CI (PR #363)
Validated across the PR's CI (all on the local 5.0 toolchain auto-detected from package.json):
- `bun run lint` exit 0, `bun run lint:actions` exit 0, `bun run test` exit 0 (lint + sdk typecheck + 73 unit + 6 scripts).
- SDK E2E (native + WASM legs, build_accelerator:true) — green (P4).
- Playground **Local Network E2E** (deploy+prove vs local 5.0 sandbox) — green after the `from: NO_FROM` deploy fix.
- Playground Mocked E2E (8) + Production Build Smoke — green.
- **WebDriver E2E on all 3 platforms** (macOS, Linux, Windows) — green (Windows after the bb-SHA pin).
- Rust Tests, Clippy, Windows Build + Prebuild Smoke, Release Smoke — green.

Two transient Playwright `install-deps chromium` timeouts (exit 124) on an earlier head cleared on re-run — the documented chronic CDN flake, not code.

## Manual v5-testnet smoke — EXTERNAL ITEM (flagged, not run autonomously)
The live-network acceptance (`bun run --cwd packages/playground dev:testnet` against `v5.testnet.rpc.aztec-labs.com`, deploy+prove+mine in a browser) is a **human** step and is **blocked on an external dependency**: the v5 testnet has **no pre-deployed canonical (salt=0) Sponsored FPC** (phase-0 recon), so it needs the v5 testnet's actual sponsored-FPC salt exported as `SPONSORED_FPC_SALT` (or a self-deploy+fund). This does not block the SDK-only release (P4 proves 5.0 proving parity on a local sandbox; the npm publish is independent). **Surfaced to the owner.**

**Gate:** automated sweep PASS; manual v5 smoke deferred to owner (external FPC dependency).

LESSONS_FILE=implementations-plan/aztec-5.0.0-2026-06-18/lessons/phase-5.md
