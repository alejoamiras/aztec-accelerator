# Phase 2 — Remove SPONSORED_FPC_SALT from CI + scripts + .env.example (2026-06-18)

- **8 workflows** stripped. The reusable-workflow asymmetric hazard held: `_e2e.yml`/`_e2e-app.yml` had `SPONSORED_FPC_SALT` as the ONLY `secrets:` child, so the whole `secrets:` block was removed (empty mapping = invalid); same for the callers (`accelerator.yml`/`sdk.yml`/`app.yml`). Declaration + callers removed together (one commit) so no `workflow_call` "secret not declared" error. Direct env usage (`publish-testnet.yml`/`publish-nightlies.yml` build step) — just the env line. Did the YAML removals with `perl` (exact-match) + **actionlint as the verifier** (exit 0 = valid).
- **`deploy-sponsored-fpc.ts`**: removed the whole `--no-secret`/`gh secret set` surface (doc + parse + the `if(!noSecret){…}else{…}` block) AND the now-unused `execSync`/`node:child_process` import. Kept `--salt`.
- **`.env.example`**: dropped the comment + line.

**Gate:** PASS — `bun run lint:actions` exit 0 (YAML valid post-removal); biome clean on the script; no script tsc errors; `grep SPONSORED_FPC_SALT .github packages .env.example` (excl node_modules + prose .md) → none; `bun run lint` + `bun run test` exit 0.

LESSONS_FILE=implementations-plan/fpc-salt-removal-docs-2026-06-18/lessons/phase-2.md
