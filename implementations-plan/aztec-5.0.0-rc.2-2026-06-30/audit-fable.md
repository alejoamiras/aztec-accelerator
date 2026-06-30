# Fable-role audit — rc.2 bump plan (Opus 4.8 1M, fresh independent context)

33 tool-uses / ~10 min / all load-bearing claims verified against the repo + live npm/GitHub.

**Verdict: conditional approve** — conditions: (1) correct the overstated "e2e proves the deployed binary handles rc.2" claim (the e2e never exercises the runtime bb download) + add a real download gate or smoke vs the released 1.0.6 binary; (2) add a P3 pre-smoke check that the rc.2-derived salt=0 SponsoredFPC address is live on testnet, with a redeploy contingency; (3) decide `CRS_CACHE_VERSION` (bump to rc.2 or record why not).

## Key findings (all folded into the plan)

**Security/supply-chain**
- Min-age override scoping is **airtight** — `bun.lock` embeds no `minimumReleaseAge` (grep: 0 matches); it records resolved versions + integrity only, so `--minimum-release-age=0` can't leak into committed state. `bunfig.toml:10` is the sole persistent gate; every CI install uses `--frozen-lockfile`. APPROVE this dimension.
- The 7-day gate's threat model (`bunfig.toml:2-6`) is a hijacked popular transitive (the `@tanstack` worm), NOT single-publisher `@aztec` — this *strengthens* Approach A.
- `_publish-sdk.yml` least-privilege fine (`id-token: write` provenance-only, `--provenance` set). Standing gap (not introduced here): static `NPM_TOKEN` vs npm trusted-publisher OIDC — future hardening.

**Assumptions**
- ✅ rc.2 live on npm; only 2 files carry `@aztec` pins; code uses only `AztecAddress.fromBigInt`; `get-sdk-publish-version.ts` derives version + `--tag testnet`/`latest:false`/`--provenance`.
- ⚠️ Edit surface undercounted: "13 distinct packages" but **24 version-string lines** (SDK=11, playground=13; playground adds `@aztec/kv-store` + `@aztec/protocol-contracts`).
- ✅ **Prove code byte-identical `accelerator-v1.0.6`→HEAD** (9 commits; only Cargo/tauri version strings; `bb.rs`+`server/prove.rs`+`versions/` `git diff --quiet` exits 0). Stronger than the plan claimed.
- ❌ **"P2 e2e proves the *deployed* accelerator handles rc.2" — OVERSTATED.** e2e bundles bb=rc.2 (`BB_BINARY_PATH` step-0 priority, `_e2e.yml:54-64`) → `prove.rs:75` `needs_download=false` → download skipped. Deployed 1.0.6 bundles bb **4.3.1** (`src-tauri/AZTEC_VERSION`) → production *downloads* rc.2 (`prove.rs:149-188`). `ACCELERATOR_DOWNLOAD_TEST` is set only on the bun e2e step (never runs `cargo test`) → inert; `accelerator.yml` runs `cargo test` but doesn't set it → `download_and_verify_bb` skips. **No CI job downloads bb rc.2.** Mitigating (verified): rc.2 `barretenberg-{amd64-linux,arm64-darwin,amd64-darwin}` assets + the GitHub digest endpoint all HTTP-200; download code byte-identical/rc.1-proven → high-confidence-fine but ungated.

**Phases/gates**
- `tsc` necessary but oversold as "the breaking-change arbiter" — blind to (a) precompiled-Noir runtime (#24021), (b) **contract-address shifts from recompiled bytecode**, (c) bb prove/download interface, (d) CRS format. 3 of the 4 threats that matter here are invisible to tsc.
- **SponsoredFPC address risk real + ungated** — address is bytecode-derived (`aztec.ts:211-216`, `proving.test.ts:62-67`, no hardcoded address). If rc.2 recompiled SponsoredFPC, salt=0 moves off the rc.1 deployment (phase-0, block 1387). P2 e2e can't catch (sandbox auto-deploys from the same rc.2 artifact → self-matches); only P3 can. Add explicit P3 go/no-go + redeploy. *(Main agent then confirmed via tarball bytecode diff: `d7cd8d05`→`fc86f9b5` — address DID move; redeploy is required, not contingent.)*
- `CRS_CACHE_VERSION` (`aztec.ts:156`) still `"5.0.0-rc.1"`; the preceding commit `0fc6ffd` was exactly the stale-CRS WASM-crash bug. Bump it.
- A-vs-B sound; reject-B stands; hybrid doesn't reduce exposure (post-regen both deploy + publish use frozen lock).

**Out-of-scope landmine:** `copy-bb.ts:56-64` `WINDOWS_BB_CHECKSUMS` has no rc.2 entry → `resolveWindowsBbChecksum` throws; the next accelerator release bundling rc.2 fails Windows prebuild until the SHA is added.

**Looks fine (verified):** override local-only; prove code parity; rc.2 bb assets reachable all platforms; 2-files pin surface; 2 TS `!` commits miss our surface; SDK on `testnet`/`latest:false`/`--provenance`.
