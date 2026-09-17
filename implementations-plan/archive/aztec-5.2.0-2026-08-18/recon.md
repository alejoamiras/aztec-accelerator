# Recon — @aztec/* 5.0.1 → 5.2.0 bump (2026-08-18)

Phase 0.4 consolidated findings from four read-only recon agents (bump tooling, SDK API surface,
playground + testnet smoke, CI gates) plus direct verification. Recon initially read local main
(`5d0efa4`); origin/main had advanced to `b98352b` (v2-release-train, 42 commits). Verified against
`b98352b` (this worktree's base): all @aztec pins still 5.0.1, CRS_CACHE_VERSION still "5.0.1",
updater tooling unchanged, SDK @aztec import surface unchanged (only a new local `errors.js`
import). CI-gate findings were validated against live GitHub data (`gh`), so they reflect current
state. Recon findings carry over cleanly.

## External facts (verified 2026-08-18)

- `@aztec/aztec.js@5.2.0` published to npm **2026-08-18T10:14Z (today)**; it is the npm `latest` tag.
- `@aztec/bb.js@5.2.0` also published today. `@aztec/bb-prover@5.2.0` depends on `@aztec/bb.js@5.2.0`.
- `@aztec-foundation/aztec-standards` (lockstep package): **no 5.2.0**. Available: `5.0.1-rc.1`, `5.0.1`, `5.1.0-rc.1`. No stable 5.1.0.
- v5.2.0 GitHub release has the `barretenberg-amd64-windows.tar.gz` asset (Windows bb pin feasible).
- Live testnet (`v5.testnet.rpc.aztec-labs.com`) `node_getNodeInfo`: `nodeVersion: 5.2.0-nightly.20260815`, `l1ChainId: 11155111`, `realProofs: true`, `rollupVersion: 1821665230`, protocol `feeJuice` address still `0x03`. **Testnet is already on the 5.2.0 line** — the bump is well-timed.
- Release notes: v5.1.0 breaking changes are aztec.nr-side (note property selectors from packed layout; HandshakeRegistry address change). v5.2.0 bumps Noir beta.22→beta.25 (contract-side `pub` on note types) and claims **no breaking TS API changes**; "protocol constants are unchanged, so v5.1.0 and v5.2.0 nodes interoperate".
- Bun min-age exclusion key: `[install] minimumReleaseAgeExcludes = [...]` in bunfig.toml. Docs show exact package names (scoped OK); wildcard `@aztec/*` support undocumented — verify empirically.

## What exists (reuse map)

### Bump tooling (reuse as-is)
- `scripts/update-aztec-version.ts` (alias `bun run aztec:update <version>`) — rewrites `@aztec/*` + lockstep pins in `packages/sdk/package.json` and `packages/playground/package.json`, bumps `CRS_CACHE_VERSION` in `packages/playground/src/aztec.ts`. Does `npm view` per package first (network); **skips-and-warns** (exit 0) on unpublished packages → can produce a partially-bumped tree. Deliberately does NOT touch the Windows bb pin (F-008).
- `scripts/check-windows-bb-pin.ts` — post-`bun install` informational report on whether the live `@aztec/bb.js` version has a reviewed Windows pin. Always exit 0; the enforcing gate is CI.
- `packages/accelerator/scripts/copy-bb.ts` — `WINDOWS_BB_CHECKSUMS` (pins end at `"5.0.1"` — **no 5.2.0 entry**), `resolveAztecBb()` (reads installed `@aztec/bb.js` from node_modules), `resolveWindowsBbChecksum()` (throws without a pin — fail-closed). Writes gitignored `src-tauri/AZTEC_VERSION` during prebuild (self-healing; never hand-edit).
- `LOCKSTEP_PACKAGES = {"@aztec-foundation/aztec-standards"}` — explicit allowlist, not prefix-match. Its generated JS has undeclared runtime imports of `@aztec/aztec.js` resolved against the playground's pins.
- Rust/Tauri runtime needs **zero code changes**: the multi-version bb cache (`packages/accelerator/core/src/versions/`) downloads bb per-version dynamically keyed off the `x-aztec-version` header; `check_version_selectable` is denylist-only, no version floor.

### SDK @aztec surface (drift-risk list)
Production `src/lib` coupling is minimal:
- `BBLazyPrivateKernelProver` (`@aztec/bb-prover/client/lazy`) — **extended**; only `createChonkProof(executionSteps): Promise<ChonkProofWithPublicInputs>` overridden, `super.createChonkProof()` is the WASM fallback. Constructor called with one positional arg (`options` never passed).
- `CircuitSimulator` (type), `WASMSimulator` — `@aztec/simulator/client`; WASMSimulator via **dynamic import** (runtime-only failure mode if the subpath/export moves; deliberately not a hard dep).
- `PrivateExecutionStep`, `serializePrivateExecutionSteps` — `@aztec/stdlib/kernel`.
- `ChonkProofWithPublicInputs` (`.fromBuffer`) — `@aztec/stdlib/proofs`.
- `x-aztec-version` header value read from the SDK's own package.json `dependencies["@aztec/stdlib"]`.
- `@aztec/foundation`, `@aztec/noir-acvm_js`, `@aztec/noir-noirc_abi`, `@aztec/accounts`, `@aztec/pxe` are pinned but never imported — transitive-resolution pins for consumers; the updater's scope-prefix match handles them.
- History: `tsc --noEmit` (`test:lint`) was the decisive drift arbiter on all four prior cycles; SDK src needed zero changes every time. **But** all prior cycles were rc/patch steps — 5.0.1→5.2.0 is the first multi-minor jump; real breaks historically landed in `packages/playground` (e.g. `AztecAddress.fromBigInt` removal, `DeployMethod.address` → `getAddress()`, `proverOrOptions` shape).

### Playground surface
- `packages/playground/src/aztec.ts` — all @aztec usage: `createAztecNodeClient`, `EmbeddedWallet.create`, `SponsoredFeePaymentMethod`, `SponsoredFPCContract`, `getContractInstanceFromInstantiationParams`, `TokenContract` (deep import from `@aztec-foundation/aztec-standards/dist/src/artifacts/Token.js`), tx-receipt polling, `CRS_CACHE_VERSION = "5.0.1"` (line ~168).
- `main.ts` renders an **informational** SDK-vs-node version-mismatch amber warning (string compare; `5.2.0` vs `5.2.0-nightly.20260815` may still render amber post-bump — check the comparison, don't treat amber as failure).
- Scripts: `deploy-sponsored-fpc.ts --salt 0x0` (derives FPC address from artifact, checks `node.getContract`, deploys + bridges 1000 FJ if missing — needs funded Sepolia `L1_PRIVATE_KEY`; `--fund-only` to top up), `batch-fund-fpc.ts`. `FeeJuiceContract` canonical address hardcoded `AztecAddress.fromBigIntUnsafe(3n)` — verified still `0x03` on live testnet.
- Live smoke tooling (never run by CI): `AZTEC_NODE_URL=... bun test packages/playground/src/aztec.test.ts` (live-node describe block); `bun run --cwd packages/playground dev:testnet` (manual UI); `AZTEC_NODE_URL=... ACCELERATOR_URL=... bun run --cwd packages/playground test:e2e:smoke` (Playwright `smoke` project, deploy-only, both modes, accelerated leg self-skips without `ACCELERATOR_URL`, 15-min timeout, 2 retries).

### CI gates on the bump PR (validated against live PR #421 data)
Touched set (`packages/*/package.json`, `bun.lock`, `packages/playground/src/aztec.ts`, `packages/accelerator/scripts/copy-bb.ts`, `bunfig.toml`) fires **all three** package workflows in full:
- `sdk.yml`: lint, typecheck, unit (incl. `test:scripts` + `typecheck:scripts` — gates the bump scripts themselves), e2e via `_e2e.yml` with `build_accelerator: true` (headless accelerator + real local sandbox — the accelerated proving path IS exercised in CI).
- `app.yml`: lint + 3-graph typecheck (the designed @aztec drift catcher), unit, mocked Playwright, production build smoke, local-network e2e.
- `accelerator.yml`: clippy, Rust tests, Cert Trust ×3 OS, updater feed e2e, lint (incl. copy-bb unit tests), smoke, release smoke, desktop UI, e2e, WebDriver ×3+1, **Windows Prebuild Smoke + Windows Build Smoke — fail closed on the missing 5.2.0 bb pin**.
- Required checks (GitHub ruleset `Main branch protection`): `SDK Status`, `App Status`, `Accelerator Status`, `Actionlint Status`. `required_approving_review_count: 0` — CI is the only hard gate; the "manual-review" Windows pin is process discipline, technically only format-checked (`/^[0-9a-f]{64}$/`, `provenance === "manual-review"`, non-empty note).
- Realistic wall clock: **~20–25 min**, long pole Windows Build Smoke (~20 min).
- Min-age: CI always `bun install --frozen-lockfile` → never blocked by `minimumReleaseAge`. The 7-day gate bites only at local lockfile regen.

## Conventions to match
- Bump order: `bun run aztec:update 5.2.0` → `bun install` → `typecheck:scripts` → full `bun run test` → FPC drift check → smoke.
- Same-commit file set (from `_aztec-update.yml`'s canonical `git add` + guard): `packages/*/package.json bun.lock packages/playground/src/aztec.ts packages/accelerator/scripts/copy-bb.ts`. Replicate the "no unstaged updater output" check locally.
- Commit/PR title: `chore(aztec): bump @aztec/* 5.0.1 → 5.2.0`. Worktree-style branch is established precedent (`worktree-aztec-5.0.1-2026-07-16`).
- Windows pin entries: `{ sha256, provenance: "manual-review", note }` keyed by the **live-resolved bb.js version** (historically == the aztec tag, verify after install).
- Lockfile-diff discipline after a min-age override/exception: review every changed resolution+integrity line — only @aztec/* (+ lockstep) lines may move.
- Prior-cycle lesson: a gate pass is only valid for the code state it ran against — re-run affected suites after any post-finding edit.
- Every bump cycle gets its own `implementations-plan/aztec-<version>-<date>/` dir (this one).

## Risks a naive plan would miss
1. **Windows bb pin fail-closed**: no `"5.2.0"` entry exists; both Windows CI jobs fail until a human-reviewed pin lands in the same PR. Nothing in the local flow forces this — `check-windows-bb-pin.ts` is informational only.
2. **Lockstep skew**: `@aztec-foundation/aztec-standards` has no 5.2.0 → updater warn-skips, leaving 5.0.1 pinned while `@aztec/*` moves — exactly the mixed-version state lockstep exists to prevent. Known fail-open follow-up from the 5.0.1 audit (any `npm view` failure reads as "unpublished"). Decision needed: hold 5.0.1 vs pin 5.1.0-rc.1; testnet smoke is the empirical arbiter.
3. **SponsoredFPC salt=0 address drift**: moved on all 3 prior cycles (artifact recompile). Invisible to all CI suites; only the live smoke catches it. Redeploy+fund needs a funded Sepolia L1 key (owner-gated).
4. **Min-age wall**: 5.2.0 published today; bare `bun install` refuses resolution for 7 days. Decision (user, 2026-08-18): add `@aztec/*` to `minimumReleaseAgeExcludes` in bunfig.toml (wildcard support unverified — fall back to exact names; the install itself is the test).
5. **Runtime-only drift**: tsc can't see the untyped base-class contract (constructor `options` arg), the dynamic `import("@aztec/simulator/client")`, or `node_getNodeInfo` response-shape changes (`checkAztecNode` only checks `rpc.ok`; `l1ChainId: undefined` would silently disable proofs-required detection). The sandbox e2e (CI) + live smoke cover these.
6. **CRS format**: updater bumps `CRS_CACHE_VERSION` automatically — but only if the updater path is used end-to-end; don't drop it if hand-resolving the lockstep decision.
7. **Version-mismatch banner**: string-compare vs testnet's `5.2.0-nightly.20260815` may stay amber post-bump — informational, not a failure signal.
8. **No hidden pins**: full-repo scan found no third manifest with @aztec deps; remaining `"5.0.1"` strings are comments/fixtures/docs. Rust needs no edits.
