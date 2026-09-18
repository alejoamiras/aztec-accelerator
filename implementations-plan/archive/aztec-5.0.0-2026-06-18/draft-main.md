# DRAFT (main agent) — aztec 4.3.1 → 5.0.0-rc.1 bump + release

Independent draft, written before reading codex/opus streams. Consolidated plan.md supersedes this.

## Distinctive calls / non-obvious risks I want preserved in consolidation
1. **`getTxReceipt` may be a NO-OP migration.** Playground already narrows via `isPending()`/`isDropped()` (`aztec.ts:391-394`). Verify against the 5.0 union type before assuming a break. Don't write migration code the codebase already has.
2. **The real SDK risk is the `BBLazyPrivateKernelProver` signature**, not the headline aztec.js breaks. `accelerator-prover.ts:86,94` does `super(opts.simulator ?? createLazySimulator())`; the unit test spies on `BBLazyPrivateKernelProver.prototype.createChonkProof`. If either the constructor arity or `createChonkProof` changed in 5.0, SDK source + tests break. Recon FIRST (read the 5.0 `@aztec/bb-prover/client/lazy` d.ts).
3. **Min-age hard date: 2026-06-22.** `bun install` of 5.0.0-rc.1 is refused until then by `minimumReleaseAge=604800`. Either (a) schedule the lockfile bump for the 22nd, or (b) a scoped, temporary override for that one install — which trades against the repo's own supply-chain default and must be the user's call. Code-migration work that doesn't need install can proceed earlier against a local checkout.
4. **SDK publish version derivation on an rc.** `get-sdk-publish-version.ts` derives the SDK npm version from `@aztec/stdlib` (= `5.0.0-rc.1`). Verify it produces a sane, monotonic, semver-valid npm version for a `-rc.1` upstream (and that `_publish-sdk.yml`'s `latest: true` in publish-testnet.yml is still wanted — that adds the npm `latest` dist-tag).
5. **Live-v5-testnet E2E: shared-live vs local-v5-sandbox is an unresolved ASK.** The user said "block on v5 testnet" and gave the shared RPC. But a CI gate hammering a *shared live* testnet needs funded/sponsored accounts, tolerates rate-limits/reorgs/flakiness, and pollutes shared state. A local `aztec` 5.0 sandbox of the *same protocol version* is far more reproducible and is the usual interpretation of "full E2E." Surface both; recommend: local-v5-sandbox as the BLOCKING gate + one non-blocking shared-live smoke.
6. **bb CLI surface parity (Rust side).** The accelerator shells out to the 5.0 bb. The Schnorr/domain-sep changes are in-circuit; verify the bb *CLI* (prove/write_vk/gates flags) is unchanged 4.3.1→5.0 so the Rust orchestration still works. Otherwise Rust changes too.
7. **Proving parity is the core acceptance signal**, distinct from "it compiles": same tx → native-bb proof == WASM-bb proof == accepted by v5 network.

## Phase skeleton (each ends in a real validation gate)
- P0 Recon + branch: BBLazyPrivateKernelProver 5.0 sig; getTxReceipt 5.0 type; Windows bb sha256 for 5.0.0-rc.1; min-age decision. Gate: facts in lessons, branch pushed.
- P1 Dependency bump: all `@aztec/*` → 5.0.0-rc.1 (sdk+playground, deps+devDeps), maybe `@aztec/viem`; `bun install` (≥06-22); `copy-bb.ts` writes AZTEC_VERSION + Windows sha. Gate: `bun install --frozen-lockfile` clean, copy-bb ok, `bun run lint`.
- P2 SDK migration: fix any prover-signature/import breaks; `bun run --cwd packages/sdk build` + `bun run test`. Gate: SDK builds + unit green + lint.
- P3 Accelerator/Rust+bb: verify bb CLI parity; `cargo test/fmt/clippy`; app builds with 5.0 bb. Gate: rust green, app builds.
- P4 Playground: getTxReceipt (verify), deploy/EmbeddedWallet, switch `dev:testnet` → `v5.testnet.rpc.aztec-labs.com` + v5 salt; `bun run --cwd packages/playground build` + Playwright mocks. Gate: build + 28 mock tests + lint.
- P5 Proving-parity harness: native-bb == WASM-bb on 5.0 circuits. Gate: parity test green.
- P6 Live v5 E2E (the mandated blocking gate): setup-aztec @5.0.0-rc.1 (re-verify foundry-v1.4.1 pin + forge-rename still needed for 5.0 L1 deploy), fund via sponsored FPC, deploy+prove+send real tx, assert mined. Resolve shared-vs-local. Gate: green v5 E2E.
- P7 Full local gate + WebDriver: 9 WebDriver (mac+linux), `bun run test` + `bun run lint:actions`. Gate: all green.
- P8 Accelerator rc dress-rehearsal + SDK npm publish: bump tauri version; dispatch release rc (verify stable-only steps skipped, `--prerelease`); dispatch publish-testnet (verify SDK version derivation). Gate: rc green, SDK on npm, smokes green.
- P9 Joint stable cut: stage N-1 latest.json rollback; cut stable; latest.json/S3/bump-source; live-feed verify. Gate: stable Latest, feed serves it, bump-source merged.

## Security & Adversarial (headline)
- Hard-fork/crypto risk is upstream (Schnorr→Poseidon2, PublicKeys hashes, domain seps) — accelerator's own trust surface (origin allowlist, updater Ed25519, bb-fetch SHA pin) UNCHANGED. New trust input: the 5.0 Windows bb tarball SHA (fail-closed gate — must be verified by hand).
- Supply chain: 5.0.0-rc.1 min-age gate; pin exact version (no dist-tag); frozen lockfile in CI; SDK publish OIDC + provenance.
- Release least-privilege unchanged (OIDC→S3, scoped PAT for bump-source).
- Stable-only steps untested by rc dry-run (1.0.5 latest.json incident class) — stage rollback.

## Assumptions
- Facts: the 9 ground-truth items in the brief (verified).
- Inferences (attack these): getTxReceipt is a no-op migration; BBLazyPrivateKernelProver sig unchanged; bb CLI surface unchanged; EmbeddedWallet needs no change; foundry pin still needed for 5.0.
- Asks (surface to user): (a) min-age — wait to 06-22 vs scoped override? (b) accelerator STABLE on rc-labeled npm vs wait for Aztec 5.0.0 stable? (c) live-shared-testnet vs local-v5-sandbox for the blocking E2E? (d) new SPONSORED_FPC_SALT for v5? (e) accelerator version number for this bump (1.0.7 vs 1.1.0)?
