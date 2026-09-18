# Phase 2 — SDK migration (2026-06-18)

The SDK proved **source-compatible** with 5.0 — no code changes needed beyond a comment fix:
- `bun run --cwd packages/sdk test:lint` (tsc strict) — clean.
- `bun run --cwd packages/sdk test:unit` — **45 pass / 0 fail**.
- `bun run --cwd packages/sdk build` — clean.
- Fixed the misleading comment at `accelerator-prover.ts:382` (the "is_valid_version rejects non-alphanumeric" claim is false; the server accepts `.`/`-`/`_` per `version_policy.rs` — we only strip *leading* range prefixes, preserving the `-rc.1` suffix for the `/health` handshake).

**Silent-fallback audit (P2 item b):** the real fallback paths remain `#classifyHealth` legacy version-mismatch (`accelerator-prover.ts:240-257`) and the 403-denial path (`:312-324`) — both unchanged by this bump. The legacy-mismatch branch only fires when the server reports a single `aztec_version` (not `available_versions`); the multi-version path (which the current accelerator uses) matches `5.0.0-rc.1` against the cached set. The positive "native path used" assertion lands in P4.

**Gate:** PASS — SDK typecheck + 45 unit + build all green.

LESSONS_FILE=implementations-plan/aztec-5.0.0-2026-06-18/lessons/phase-2.md
