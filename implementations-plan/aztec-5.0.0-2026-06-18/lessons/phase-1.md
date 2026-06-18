# Phase 1 — Mechanical bump (2026-06-18)

- `bun scripts/update-aztec-version.ts 5.0.0-rc.1` → rewrote `packages/sdk/package.json` + `packages/playground/package.json`. **Zero skipped packages** (all `@aztec/*@5.0.0-rc.1` resolve via `npm view`).
- `grep` confirms **no `4.3.1` remains** — all `@aztec/*` at `5.0.0-rc.1`.
- Lockfile regenerated with the approved **command-scoped min-age override**: `bun install --minimum-release-age=0` (local only; committed `bunfig.toml` untouched). 46 packages installed, lockfile saved (54 `5.0.0-rc.1` refs).
- `bun install --frozen-lockfile` → **clean, no changes** (CI will pass frozen; min-age inert under frozen).
- `viem` override `npm:@aztec/viem@2.38.2` resolved fine under 5.0 (no conflict surfaced).

**Gate:** PASS — no 4.3.1 left; frozen install clean; zero skips.

LESSONS_FILE=implementations-plan/aztec-5.0.0-2026-06-18/lessons/phase-1.md
