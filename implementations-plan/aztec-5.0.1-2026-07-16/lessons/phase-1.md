# Phase 1 lessons — bump 5.0.1 + standards integration (2026-07-16, in progress)

## Attestation verification (required by the gate) — ✅ MATCH

- Re-fetched `registry.npmjs.org/-/npm/v1/attestations/@aztec-foundation%2Faztec-standards@5.0.1`: 2 attestations (npm-publish v0.1 + SLSA provenance v1).
- **Subject digest (sha512, hex): `8be3bbbfb42abb63bd85a14576e731efee360c8a713948fe24d594a88684fa5699276c7e95efd7117b932ebeb48020d07e8da6ddbf7bcb01ff40b9397a783000` — byte-identical to npm `dist.integrity` (base64→hex) AND to the `bun.lock` entry's integrity after install.** Same artifact end-to-end.
- SLSA binding: workflow `AztecProtocol/aztec-standards` `.github/workflows/release.yml` @ `refs/tags/v5.0.1`, source commit `c74541f7cf2bb23b704e96fd326ea95d98252669` (matches packument gitHead).
- `release.yml` skim at that commit: tag-driven publish, SHA-pinned actions, `contents: read` + `id-token: write` only, protected `Production` environment, tag↔package.json version cross-check, toolchain pinned via `config.aztecVersion`. No anomalies.

## Bump + tool + gates

- `aztec:update 5.0.1`: 24 pins, zero skips (grep: 0 non-5.0.1); CRS auto-bumped; Windows checksum soft-failed auto-insert again → inserted manually, `f7a2d6b1…` **matches the GitHub v5.0.1 asset digest** (independently fetched).
- Bump tool extended for lockstep: `isAztecManagedDep()` with an exact-allowlist `LOCKSTEP_PACKAGES = {@aztec-foundation/aztec-standards}` (deliberately NOT a scope prefix); new test asserts the companion bumps and an unrelated `@aztec-foundation/*` package does NOT. New root `test:scripts` (`bun test scripts/` — 26 tests) wired into root `test:unit` AND `sdk.yml` (unit step + `scripts/**` path trigger).
- Lockfile via ONE local `--minimum-release-age=0` (4th documented use): diff = uniform `@aztec/*` 5.0.0→5.0.1 swaps + exactly 3 non-@aztec lines, all the standards entry (`{}` deps, integrity as above). Full-resolution review: no other integrity or version changed. Frozen install clean.

## Token swap + gate — ✅ green

- Swap as its own commit (`6805c59`; bump/tooling = `358d4aa`): deep-path import (biome reordered it below the @aztec block — the only lint hit), `constructor_with_minter(…, alice, AztecAddress.ZERO)`, `transfer_private_to_private(alice, bob, 500n, 0)`. tsc clean FIRST TRY against the deep path — fable's noir-contracts-pattern precedent held exactly.
- CI token-spec disposition: local-network job ships NO accelerator (`_e2e-app.yml` has zero accelerator references) → accelerated variant self-skips; BOTH specs un-skipped ("~7 min WASM" note predates 5.0), the WASM one runs in this PR = the measurement; helper strengthened with the `Balances — Alice: 500, Bob: 500` assertion. **Negative authorization cases (Bob-mints-reverts): REJECTED for this harness** — the demo UI cannot express them (fixed flow, no Bob-initiated actions); covered by the source-verified `#[authorize_once]`/minter semantics + the live gates. Ledger updated.
- kv-store 5.0.1 installed: worker + ordered-binary paths + imports map all present, byte-identical expectations hold.
- Full gate: `bun run test` exit 0 (lint + 3-graph typecheck + scripts-tsc + all units + NEW test:scripts 26 green); build ✓; mocked 8/8; production smoke 2/2 (re-run after the biome reorder — gates re-run after ANY source change, per the 5.0.0 lesson); CRS `5.0.1`.

LESSONS_FILE=implementations-plan/aztec-5.0.1-2026-07-16/lessons/phase-1.md
