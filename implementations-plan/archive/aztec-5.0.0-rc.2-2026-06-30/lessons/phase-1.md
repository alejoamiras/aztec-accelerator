# Phase 1 — Bump pins + CRS + lockfile + compile-verify (2026-06-30)

- **24 `@aztec` pins** bumped rc.1→rc.2 (sdk=11, playground=13) via `perl` scoped to `@aztec/` lines only (verified `5.0.0-rc.1` appears nowhere else in those files first). `CRS_CACHE_VERSION` (`aztec.ts:156`) → `5.0.0-rc.2`.
- **Min-age override worked, local-only:** `bun install --minimum-release-age=0` resolved rc.2 (~1 day old < the 7-day `bunfig` gate), exit 0.
- **Lock-diff scrutiny (codex condition) — PASS:** diffed pre/post `bun.lock`; **only `@aztec/*` entries changed** (all ~30 incl. transitives rc.1→rc.2). Zero fresh non-`@aztec` transitives pulled by the whole-resolution override.
- **CI-parity (frozen) — PASS:** `bun install --frozen-lockfile` (no override) → exit 0, "no changes". Confirms CI won't re-block on min-age (matches the rc.1-at-3-days precedent).
- **Breaking-change arbiter — PASS:** `bun run --cwd packages/sdk test:lint` (tsc --noEmit) exit 0, no errors → #24230 (AztecAddress *Unsafe) / #24280 (pxe TaggingSecretSource) / #24007 don't reach our import surface (we use only `AztecAddress.fromBigInt`).

**Gate — PASS:** override-regen + frozen both exit 0; lock-diff only `@aztec`; `bun run lint` + sdk `test:lint` + playground `build` + `bun run test` (6 tests, 0 fail) all exit 0; `CRS_CACHE_VERSION` = rc.2.

LESSONS_FILE=implementations-plan/aztec-5.0.0-rc.2-2026-06-30/lessons/phase-1.md
