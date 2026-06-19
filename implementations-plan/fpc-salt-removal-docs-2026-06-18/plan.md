# Plan — Remove `SPONSORED_FPC_SALT` + refresh docs (`/blueprint light`)

**Tier:** `light` (0/6 high-risk dimensions: docs + removal of an obsolete config; the salt=0 canonical FPC address is now intentionally public). **Status:** awaiting approval.

## Goal
1. **Remove `SPONSORED_FPC_SALT` entirely** (env var + repo secret). It existed to avoid publishing the salt=0 sponsored-FPC address; now that the canonical salt=0 SponsoredFPC is deployed+funded on v5 (`0x261366b3…7880`) and is the documented default, the salt is no longer secret or needed. Every network resolves the canonical FPC at salt=0 (local sandbox auto-deploys it; v5 has it), so "always salt=0" is correct everywhere.
2. **Refresh the docs** per the 2026-06-18 review (5 findings).
3. **Re-deploy the playground to v5** and smoke it salt-less; **delete the repo secret** last.

The `--salt` CLI flag on the FPC scripts is a **separate dev tool** (deploy a *custom* FPC at a chosen salt) and **stays** — only the `SPONSORED_FPC_SALT` env var/secret is removed.

## Removal surface (verified)
- **Code:** `packages/playground/src/aztec.ts:206-211` (`initializeFPC` reads `process.env.SPONSORED_FPC_SALT`); `packages/playground/vite.config.ts:69,125` (bakes it into the build, 2×); `packages/sdk/e2e/proving.test.ts:61-63` (reads it).
- **Scripts:** `packages/playground/scripts/deploy-sponsored-fpc.ts` — the whole `--no-secret`/secret-set surface: doc `:18`, parse `:45`, the `if (!noSecret) { gh secret set SPONSORED_FPC_SALT … } else { … }` block `:238-252` (incl. log `:248`). Remove all of it (keep `--salt`).
- **CI (8 workflows):** `_e2e.yml` (input decl `:22` + usage `:118`), `_e2e-app.yml` (`:22`,`:53`), `publish-testnet.yml:76`, `accelerator.yml:270`, `sdk.yml:109`, `app.yml:130`, `publish-nightlies.yml:67`.
- **Docs/config:** `packages/playground/README.md:39` (lists it as Required); **`.env.example:5`** (`# SPONSORED_FPC_SALT=0x...` — codex catch, was missing from the surface).
- **Secret:** repo secret `SPONSORED_FPC_SALT` (currently `0x0`).

## Docs to refresh (from the review)
- `packages/accelerator/README.md` — (a) add a **"Version model"** note: *the accelerator (desktop & headless) downloads bb at runtime, so an `@aztec` bump ships **SDK-only** — you do NOT re-release the accelerator*; (b) fix the **stale headless caveat** (it claims the headless build "still pulls Tauri in transitively" + needs `libwebkit2gtk-4.1`/`libgtk-3` — wrong post-core-extraction; the CI headless leg installs only `libssl-dev`); (c) bump the `ACCELERATOR_VERSION: "1.0.2"` example.
- `docs/RELEASE_RUNBOOK.md` — add the **SDK-only release path** + the **SDK npm publish flow** (`publish-testnet.yml`, the `skip_sdk_publish` input, `latest:false`) and the SDK-only-vs-full-accelerator-release decision.
- `README.md` (root) — refresh (March-era): headless server, runtime-bb model, 5.0-era.
- `packages/playground/README.md` — remove the `SPONSORED_FPC_SALT` row; note the canonical salt=0 FPC default; reflect the v5 host.
- `packages/sdk/README.md` — fix the stale **quick-start** example: `getSchnorrAccount(pxe, secretKey, signingKey, Fr.ZERO, prover)` (`README.md:28`, the old 5-arg form) → the 5.0 `@aztec/accounts` form (verify the exact signature against the installed `@aztec/accounts` README/types before rewriting). **Do NOT touch** `EmbeddedWallet.create("http://localhost:8080", …)` — codex-verified it still accepts `string | AztecNode` in 5.0 (`@aztec/wallets/.../browser.d.ts:8`); it is NOT stale.
- `implementations-plan/aztec-5.0.0-2026-06-18/lessons/phase-0.md:17-18` — correct the now-stale note "salt=0 canonical NOT published on v5": it IS published+funded now (we deployed it; claim mined block 1387). (Keeps the internal record honest; codex flagged the contradiction.)

---

## Phases (each ends in a real validation gate)

### P1 — Remove `SPONSORED_FPC_SALT` from code ✓
- `aztec.ts` `initializeFPC`: drop the `process.env.SPONSORED_FPC_SALT` read; always `new Fr(0)`; update the doc comment to "canonical salt=0 SponsoredFPC".
- `vite.config.ts`: remove the two `SPONSORED_FPC_SALT` env-baking lines.
- `proving.test.ts`: drop the env read; always salt=0.
- Confirm no other runtime reader remains (`grep -rn SPONSORED_FPC_SALT packages/*/src packages/*/e2e`).

**Validation gate:** `bun run lint && bun run test` (biome + sdk typecheck + unit) **and** `bun run --cwd packages/playground build`. Pass: all exit 0; build clean; grep shows no `process.env.SPONSORED_FPC_SALT` readers left. Layers: typecheck · lint · unit.

### P2 — Remove `SPONSORED_FPC_SALT` from CI + scripts + `.env.example` ✓
- Strip the `SPONSORED_FPC_SALT:` lines from the 8 workflows. **Asymmetric-edit hazard (codex):** in `_e2e.yml`/`_e2e-app.yml` remove BOTH the `secrets:` *input declaration* AND every caller passing `secrets.SPONSORED_FPC_SALT`, in the same change — passing a secret a called workflow no longer declares is a hard `workflow_call` error (an *unset* `${{ secrets.X }}` just renders `""`, so a leftover usage is harmless, but a leftover caller→removed-declaration is not).
- `deploy-sponsored-fpc.ts`: remove the whole `--no-secret`/secret-set surface (`:18` doc, `:45` parse, `:238-252` block incl. `:248` log). Keep `--salt`.
- `.env.example`: drop the `SPONSORED_FPC_SALT` line.
- Re-grep to confirm zero `SPONSORED_FPC_SALT` references remain in `.github/`, `packages/`, `.env.example`.

**Validation gate:** `bun run lint:actions` (actionlint) **and** `bun run lint` (biome on the touched script). Pass: exit 0; `grep -rn SPONSORED_FPC_SALT .github packages .env.example` (excl. the prose docs handled in P3) returns nothing. Layers: lint · actionlint.

### P3 — Refresh the 5 docs ✓
Apply the doc changes listed above. No full local paths in committed files (use `~/`/repo-relative). Verify relative links resolve. Re-verify the SDK README's 5.0 code examples against the installed `@aztec` types before committing (don't ship an example that doesn't compile).

**Validation gate:** manual review — each of the 5 docs reflects current reality (salt-less, 5.0, headless slimmed, version-model note present); the playground README no longer lists `SPONSORED_FPC_SALT`; no absolute local paths; relative links resolve. (Markdown isn't lint-gated in this repo — review is the gate.) Layers: manual-review.

### P4 — Land the PR ✓ (PR #366, squash `ba3aec0`)
Branch (`chore/remove-fpc-salt` or similar) → PR → CI green → auto-merge. `main` is branch-protected (branch + PR + auto-merge; unsigned commits via `git -c commit.gpgsign=false`). The e2e now runs salt-less (salt=0 canonical) — confirm the local-sandbox e2e stays green.

**Validation gate:** `sdk.yml` + `app.yml` + `accelerator.yml` + `actionlint.yml` green (incl. the salt-less e2e); PR auto-merges. Layers: lint · typecheck · unit · e2e (local sandbox). *(Known infra flake: the Playwright `install-deps` timeout — re-run the failed job, it's not the change.)*

### P5 — Re-deploy the playground to v5 + smoke
Dispatch `publish-testnet.yml --ref main -f skip_sdk_publish=true` (from merged, salt-less main). The build no longer bakes `SPONSORED_FPC_SALT`; the playground resolves the canonical salt=0 FPC.

**Validation gate:** `deploy-app` green; `curl` the live bundle and confirm **no `SPONSORED_FPC_SALT` baked** and the v5 host present; **manual browser smoke** at `playground.aztec-accelerator.dev` — a deploy proves+mines paying via the canonical FPC. Layers: e2e-live-network (manual). *(Human step — needs a browser click-through.)*

### P6 — Delete the repo secret
After P4 merged + P5 confirmed (no workflow references `secrets.SPONSORED_FPC_SALT`): `gh secret delete SPONSORED_FPC_SALT`.

**Validation gate:** `gh secret list` no longer shows `SPONSORED_FPC_SALT`; the next CI run on `main` is green (no missing-secret reference errors — there are none to reference). Layers: manual + CI.

---

## Security & Adversarial Considerations
- **Threat model:** removing `SPONSORED_FPC_SALT` does not expand attack surface — the salt=0 canonical FPC address was already derivable from the public artifact (`getContractInstanceFromInstantiationParams` + `SPONSORED_FPC_SALT=0` from `@aztec/constants`); the secret only added obscurity, not security. The FPC sponsors fees unconditionally by design (testnet only).
- **Least privilege / secret hygiene:** deleting an unused secret is *better* posture (one fewer secret to leak/rotate). Order matters: delete only after no workflow references it (P6 after P4 merge), so a CI run never errors on a missing secret — though with the code defaulting to salt=0, an empty value is already harmless.
- **Supply chain:** docs/config-only; no dependency changes, no `bun install`, no npm publish in this plan (the SDK is already published; P5 is `skip_sdk_publish=true`).
- **Blast radius:** the live playground is the only outward artifact touched (P5 re-deploy). The salt-less build is verified by the live smoke before P6. Rollback: re-dispatch the prior deploy / re-add the secret (reversible).
- **CI least-privilege unchanged:** no token/permission changes; only removing a passed secret.

## Assumptions
**Facts** (verified):
- The `SPONSORED_FPC_SALT` usages above (grep'd, with file:line) — now incl. `.env.example:5` and the full `--no-secret` script surface (codex catches).
- The canonical salt=0 SponsoredFPC is **auto-deployed on the local sandbox** (the SDK/playground e2e pass with salt=0).
- `initializeFPC` (`aztec.ts:204-213`) and `proving.test.ts:63-64` default to `new Fr(0)` when the env is unset → removing the env = always salt=0 (no behavior change vs the current `0x0` secret).
- Only the live-network deploy/e2e callers inject the salt (`publish-testnet.yml:73-77`, `publish-nightlies.yml:64-68`; `_e2e*` gate on `!contains(node_url,'localhost')`) — so removal is a true no-op for PR CI (codex-confirmed).
- `_e2e.yml`/`_e2e-app.yml` only pass the salt for **non-localhost** e2e (`!contains(aztec_node_url,'localhost')`); local e2e already runs salt-less.
- The accelerator README's headless caveat is stale: the CI headless leg installs only `libssl-dev` (`_e2e.yml:51`), not WebKit/GTK.
- `main` is branch-protected; commits unsigned via `git -c commit.gpgsign=false`.

**Inferences** (attack these):
- The SDK README's `EmbeddedWallet.create("http://localhost:8080", …)` / `getSchnorrAccount(pxe, …)` examples are stale for 5.0 (5.0 takes a node client). *Verify against installed types in P3 before rewriting — don't assume the exact replacement.*
- `publish-nightlies.yml` passing the salt is removable without breaking a nightlies-specific FPC (it should resolve canonical salt=0 like everything else). *Verify it doesn't target a different network/FPC.*
- No external consumer reads `SPONSORED_FPC_SALT` (it's repo-internal build/CI config, not part of the published SDK surface).

**Asks** (resolved with the user):
- Validation depth → **re-deploy to v5 + live smoke** (P5). ✓
- Delete the repo secret → **yes, last** (P6). ✓
- `--salt` CLI flag → **kept** (separate dev tool). ✓

**External preconditions** (not repo-verified Facts — codex):
- The canonical salt=0 SponsoredFPC is **funded on v5** — true because we deployed+funded it this session (claim mined block 1387), NOT a repo-documented fact (a stale lesson said the opposite; P3 corrects it). Re-confirm with `node.getContract(0x2613…)` before the P5 live smoke.
- **P5 deploy safety depends on `TESTNET_AZTEC_NODE_URL` = the v5 host** (set this session). The salt-less build is only correct if the deploy targets v5 (where the canonical FPC is funded). Verify the secret before dispatching P5.

## Codex audit (light, xhigh) — `conditional approve`
Conditions, all folded: (1) add `.env.example:5` to the removal surface [done — P2]; (2) the v5-funded salt=0 is an external precondition, not a Fact [done — reclassified + P3 corrects the stale lesson]; (3) don't rewrite `EmbeddedWallet.create(url,…)` — still valid in 5.0; the stale example is `getSchnorrAccount` [done — P3 corrected]. Also folded: the full `--no-secret` script surface (`:45`/`:238-252`); the reusable-workflow asymmetric-edit hazard (declaration+caller together). **Confirmed fine by codex:** the local no-op (`Fr(0)` default), the headless caveat is genuinely stale, removal is a no-op for PR CI. No rejected findings.

## Post-implementation hardening
Not needed — no trust-boundary/auth/publishing change; removing an obsolete secret is itself a small hardening. No `/harden` scheduled.

## Seeds
See `eli5.html` for the `/goal` (recommended) and `/loop` seeds. Finalized post-approval.
