# Plan — Bump `@aztec/* 5.0.0-rc.1 → 5.0.0-rc.2` + release the SDK (`/blueprint mid`)

**Tier:** `mid` (rubric: 0 HIGH at draft; the dual audit then surfaced a real MODERATE→HIGH on **migration cost** — the testnet SponsoredFPC must be redeployed — vindicating the tier). **Status:** awaiting approval. **Both auditors: `conditional approve`; all conditions folded below.**

## Goal
Track Aztec testnet's **rc.2** upgrade (executed 2026-06-30 12:00 UTC) by bumping the `@aztec/*` pins `5.0.0-rc.1 → 5.0.0-rc.2`, re-publishing the SDK to npm on the **`testnet`** dist-tag (never `latest`), redeploying the playground, **and redeploying+funding the rc.2-derived SponsoredFPC on v5 testnet**. **SDK-only** model: the deployed accelerator (1.0.6) downloads `bb` at runtime keyed by `x-aztec-version`, so no accelerator re-release.

**Done =** SDK published as the rc.2-derived version on `testnet` (latest still 4.x); rc.2 salt=0 SponsoredFPC deployed+funded on testnet; live playground on rc.2 + browser-smoked; CI green; no accelerator release.

## Breaking-change assessment (the user's "double-check me")
187 commits rc.1→rc.2, **8 breaking-flagged (`!`)**. The user's "no breaking changes" is **wrong in one load-bearing way** (the FPC recompile, below); right about the TS import surface.
- **TS import surface — safe (verified):** our code uses only `AztecAddress.fromBigInt` (×2) and zero `*Unsafe`/`registerSender`/`TaggingSecret` calls, so `stdlib!: AztecAddress *Unsafe rename` (#24230) and `pxe!: TaggingSecretSource` (#24280) don't reach us. `tsc --noEmit` against rc.2 confirms — but tsc is **necessary, not sufficient** (it sees type/API breaks only).
- **Precompiled-artifact runtime change — the real hit:** the **SponsoredFPC bytecode changed** (`@aztec/noir-contracts.js` artifact sha256 `d7cd8d05…`→`fc86f9b5…`, verified by tarball diff). Bytecode → contract-class-id → **instance address**, so the **salt=0 FPC address moved** off the funded rc.1 deployment. tsc can't see this; the local-sandbox e2e can't (it auto-deploys from the *same* rc.2 artifact → self-consistent). Only a **live testnet check** catches it → drives P3.
- **Out of our surface:** Noir/aztec-nr (`for_each` order #24021, `msg_sender` ctx #24062, oracle structs #24284) and node/prover-ops (#24189, #24008) — we don't author contracts; the one that *could* bite via the precompiled artifact (a bytecode shift) is handled by the P3 redeploy.

---

## Approach A — Track-now (primary, recommended)
Bump immediately; override the 7-day npm min-age **locally only** to regenerate the lockfile; ship through the native-`bb` e2e + the gated download test; redeploy the testnet FPC; publish + redeploy playground. **Why now:** testnet is on rc.2 today — an rc.1 client against an rc.2 node risks protocol-version rejection, so the live playground should track rc.2 promptly.

## Approach B — Wait-for-min-age (competing outline, rejected — both auditors concur)
Wait until rc.2 is ≥7 days old (~2026-07-06), bump with no override. **Rejected:** (1) leaves the live playground on rc.1 against an rc.2 testnet for ~7 days (credible protocol-mismatch breakage — the real cost); (2) the override is *local-only* on official AztecProtocol packages, not a random transitive; (3) a hybrid ("deploy now, delay npm publish 7d") does **not** reduce exposure — once P1 regenerates the lock, both the deploy and the publish run via `--frozen-lockfile` with no further override (the override is a single local event). The **Ask** below surfaces A-vs-B for you to overrule; both auditors note A's edge over B is contingent on rc.1 actually being at-risk against rc.2 testnet (likely, unproven).

---

## Phases (each ends in a real validation gate)

### P1 — Bump pins + CRS bump + regenerate lockfile (local min-age override) + compile-verify ✓
- Edit **all `@aztec/*` version strings** `rc.1 → rc.2` in **`packages/sdk/package.json` (11 lines)** + **`packages/playground/package.json` (13 lines — incl. the SDK-absent `@aztec/kv-store` + `@aztec/protocol-contracts`)**. 24 lines total; missing the two playground-only packages desyncs the lock. Accelerator has no `@aztec` pins.
- **Bump `CRS_CACHE_VERSION` `packages/playground/src/aztec.ts:156` `"5.0.0-rc.1"→"5.0.0-rc.2"`** — same one-line guard whose absence caused the WASM `SrsInitSrs` crash on the *last* bb bump (commit `0fc6ffd`); harmless if rc.2's CRS format is unchanged.
- Regenerate the lockfile with the **local-only** override (rc.2 ~1 day old < the 7-day `bunfig.toml:10` gate): `bun install --minimum-release-age=0`. **Never** edit `bunfig.toml`; **never** put the override in CI.
- **Lock-diff scrutiny (codex):** `--minimum-release-age=0` lifts the gate for the *whole* resolution. Diff `bun.lock`; confirm only `@aztec/*` versions changed; for any **new/changed non-`@aztec`** package, check it isn't a suspicious <7-day-old publish.
- **CI-parity proof:** `bun install --frozen-lockfile` (NO override) must exit 0 (precedent: rc.1 was ~3 days old when #363's CI passed; frozen installs don't re-resolve/re-check age — **authoritatively re-confirmed by P2's CI on bun-`latest`**).

**Validation gate:** `bun install --minimum-release-age=0` then `bun install --frozen-lockfile` both exit 0; `bun.lock` diff shows only `@aztec` version changes (no unexpected fresh transitives); `bun run lint` + `bun run --cwd packages/sdk test:lint` (tsc) + `bun run --cwd packages/playground build` + `bun run test` all exit 0; `CRS_CACHE_VERSION` reads `5.0.0-rc.2`. Layers: typecheck · lint · unit.

### P2 — Land the bump PR: native-`bb`-rc.2 e2e + runtime-download gate + auto-merge ✓
- **Native-`bb`-rc.2 e2e** (`sdk.yml`, `build_accelerator: true`): builds the accelerator from HEAD (prove code **byte-identical to the deployed 1.0.6** — verified: `git diff accelerator-v1.0.6..HEAD` over `core/src/server*`,`bb.rs`,`versions/` is empty), runs the SDK e2e against a local sandbox with the accelerator serving native `bb` rc.2. Asserts `"transmit"` (native `/prove`) + not `"fallback"`. **Proves: the prove/msgpack/proof interface is compatible with bb rc.2.**
- **Runtime-download gate (codex C2 + opus C1):** the e2e bundles bb=rc.2 (`BB_BINARY_PATH`), so `needs_download=false` — it does **NOT** exercise the path production takes (deployed 1.0.6 bundles bb **4.3.1** → *downloads* rc.2 at runtime). Close it: run `ACCELERATOR_DOWNLOAD_TEST=1 AZTEC_BB_VERSION=5.0.0-rc.2 cargo test download_and_verify -- --nocapture` **from `packages/accelerator/core`** (final-codex: the test lives in the core crate; running from `src-tauri` won't execute it) — exercises download rc.2 → SHA-256 verify → extract on the **current arch**. (Opus verified the rc.2 `barretenberg-*` assets + GitHub digest endpoint are HTTP-200 on all platforms, and the download code is byte-identical/rc.1-proven.) **Scope (final-codex):** the local test proves *this arch* only; cross-platform coverage rests on that byte-identical/reachable-assets evidence + the P3 released-1.0.6 smoke on the target platform — don't oversell it as full cross-platform proof.
- Branch → PR → CI green → auto-merge. `main` branch-protected; unsigned commits via `git -c commit.gpgsign=false`.

**Validation gate:** `sdk.yml` (native-bb-rc.2 e2e: asserts `transmit`, not `fallback`) + `app.yml` + `accelerator.yml` + `actionlint.yml` green; the gated rc.2 **download_and_verify** test passes locally (download + digest + extract); PR auto-merges. Layers: typecheck · lint · unit · **e2e (native bb rc.2) · download+digest**. *(Known flake: Playwright `install-deps` timeout — re-run; not the change.)*

### P3 — Redeploy+fund rc.2 FPC → publish SDK (testnet) → redeploy playground → smoke ✓
- **(a) REQUIRED — redeploy+fund the rc.2 SponsoredFPC on v5 testnet.** The rc.2 bytecode moved the salt=0 address. Derive the rc.2 salt=0 `SponsoredFPCContract` address; `node.getContract(addr)` on `v5.testnet.rpc.aztec-labs.com` — if undefined (expected, since the address moved), run `bun run packages/playground/scripts/deploy-sponsored-fpc.ts --salt 0x0` (bridge fee juice from Sepolia → deploy → claim, the rc.1 session's flow). **`--salt 0x0` is mandatory (final-codex footgun):** the script defaults to `Fr.random()`, which would deploy a *random-salt* FPC at an address the playground never derives. **Precondition:** the disposable Sepolia wallet (`L1_PRIVATE_KEY`) has gas + bridgeable funds — check first; re-fund if needed.
- **(b) Publish SDK + redeploy playground.** Dispatch `publish-testnet.yml --ref main` (NO `skip_sdk_publish`) from merged main: `publish-sdk` publishes the rc.2-derived version (auto from the `@aztec/stdlib` pin via `get-sdk-publish-version.ts`) on **`testnet`**, `--provenance`, `latest:false`; `deploy-app` rebuilds the playground → S3 → CloudFront.
- **(c) Verify + smoke.** `npm view … dist-tags` → `testnet` = rc.2-derived, `latest` unchanged (4.x); live bundle baked `VITE_AZTEC_SDK_VERSION=5.0.0-rc.2`. **Live browser smoke** — deploy+transfer paying via the (now-redeployed) rc.2 FPC. *Strongest form (opus C1):* run the smoke against the **released 1.0.6 desktop app** (bundles 4.3.1 → genuinely downloads rc.2 at runtime), not a HEAD build — that's the only check that exercises the production download path end-to-end. Minimum: WASM-path browser smoke (still proves live-testnet rc.2 + the FPC fee path).

**Validation gate:** rc.2 FPC `node.getContract(addr)` defined on testnet (= **deployed**; funding proven *separately* by the deploy script's claim-receipt mining + the smoke paying via it — `getContract` is not funding proof, per final-codex); `publish-sdk` + `deploy-app` green; `npm view` shows `testnet`→rc.2-derived & `latest` still 4.x; `curl` live bundle = `5.0.0-rc.2` + HTTP 200; **manual browser smoke passes** (ideally vs the released 1.0.6 app → exercises the rc.2 download). Layers: e2e-live-network (manual). *(Human acceptance — browser click-through; + an L1 bridge tx for the FPC redeploy.)*

---

## Security & Adversarial Considerations
- **Supply chain (headline):** installing `@aztec/*@5.0.0-rc.2` ~1 day old, inside the 7-day gate. Mitigations: (1) override is **local-only + command-scoped** (one `bun install`), never in `bunfig.toml`, never CI (opus verified `bun.lock` embeds no `minimumReleaseAge` — the override cannot leak into committed state); (2) **but it lifts the gate for the whole resolution** (codex) — P1's lock-diff scrutiny is the compensating control for fresh non-`@aztec` transitives; (3) exact pins + committed `bun.lock` → CI `--frozen-lockfile` installs only what we vetted; (4) our publish uses `--provenance` (Sigstore). Residual: a compromised AztecProtocol rc.2 publish — the same trust we extend to every `@aztec` release; the 7-day gate would only delay, not prevent, adopting a version we intend to ship.
- **Least privilege:** no token/permission/secret change vs rc.1. *Standing gap (NOT introduced here; ledger):* `_publish-sdk.yml` authenticates with a static `NPM_TOKEN` rather than npm trusted-publisher OIDC (your global pref) — flag for a future hardening pass.
- **Protocol/version (Aztec-domain):** rc.1-client-vs-rc.2-node mismatch is the live risk; tracking rc.2 promptly *reduces* it. The **FPC-address shift** is the concrete instance — handled by the P3 redeploy.
- **bb prove-interface trust:** SDK-only trusts bb rc.2's CLI/msgpack/proof interface matches the deployed 1.0.6 prove code — gated by P2's native e2e (interface) **and** the download test (the runtime path). If either breaks → escalate to an accelerator release (out of scope; surface + hold).
- No new trust boundary/auth/input surface → `/harden` skipped (user-confirmed).

## Assumptions
**Facts** (verified this session):
- `@aztec/{stdlib,aztec.js}@5.0.0-rc.2` published to npm (~1 day old; npm `modified 2026-06-30T05:32Z`). Bump surface = exactly `packages/sdk/package.json` (11) + `packages/playground/package.json` (13); accelerator has no `@aztec` pins.
- Our code: only `AztecAddress.fromBigInt`; zero `*Unsafe`/registration APIs → the 2 TS-surface `!` commits miss us.
- **Accelerator prove code byte-identical `accelerator-v1.0.6`→HEAD** (empty diff over `core/src/server*`/`bb.rs`/`versions/`) → the HEAD-built e2e binary's prove path == the deployed 1.0.6 binary's.
- **SponsoredFPC bytecode CHANGED rc.1→rc.2** (artifact sha256 `d7cd8d05…`→`fc86f9b5…`) → salt=0 address moved → testnet FPC redeploy required (P3a).
- `get-sdk-publish-version.ts` derives the publish version from the `@aztec/stdlib` pin (`_publish-sdk.yml:69-81`); ships `testnet`/`latest:false`/`--provenance`. `bun.lock` records no `minimumReleaseAge` (override can't leak).
- rc.1 precedent: published 2026-06-15, #363 merged 2026-06-18 (~3d<7) green → frozen install doesn't re-apply min-age.

**Inferences** (attack these):
- `tsc` compiles clean against rc.2 (the `!` commits miss our import surface). *P1 confirms; a break is named before proceeding.*
- bb rc.2's prove CLI/msgpack/proof + download interfaces are compatible with the deployed 1.0.6. *P2 native e2e + download test confirm; contingency = accelerator release (out of scope).*
- The rc.2 salt=0 FPC isn't already on testnet (rc.1 one was self-deployed). *P3a's `getContract` confirms; deploy if absent.*

**Asks** (user approves at the gate — A is the chosen path, not an open question; final-codex reword):
- **Approve the Approach-A exception:** the local-only 7-day min-age override (vs waiting ~7 days = B). Both auditors + the final pass concur A; surfaced for sign-off, not re-litigation.
- **Approve the expanded scope:** P3 now includes **redeploying+funding the rc.2 testnet FPC** (an L1 Sepolia bridge tx, disposable wallet) — beyond the original "bump + republish." Confirm the Sepolia wallet is funded (or you'll fund it).

## Decision ledger
- **Chosen: Approach A** (track-now, local min-age override). **Rejected: B** (wait 7d) — both auditors concur; leaves the playground broken vs rc.2 testnet meanwhile; a deploy-now/publish-later hybrid doesn't reduce exposure (post-regen both use frozen lock). **Disputed/unresolved:** none material — both auditors `conditional approve` with overlapping conditions.
- **Conditions folded:** codex — (1) CI-`latest` frozen-install proof [P2 CI], (2) HEAD==1.0.6 prove code [verified Fact], (3) lock-diff for fresh transitives [P1 gate], (4) live FPC smoke [P3c]. opus — (1) correct the "e2e proves deployed binary" overstatement + add the runtime-**download** gate [P2], (2) FPC-address go/no-go + redeploy [P3a — **confirmed required** via bytecode diff], (3) `CRS_CACHE_VERSION` bump [P1]. Plus edit-surface = 24 lines, tsc reframed necessary-not-sufficient, transitive-scoping correction.
- **Out-of-scope landmines noted:** `copy-bb.ts` `WINDOWS_BB_CHECKSUMS` has no rc.2 SHA → the *next* accelerator release bundling rc.2 fails Windows prebuild until added (harmless here — SDK-only, Linux e2e prebuild). `_publish-sdk.yml` static `NPM_TOKEN` → future OIDC-trusted-publisher hardening.

## Codex audit (mid, xhigh) — `conditional approve`
Conditions (1) CI-latest frozen-install proof, (2) HEAD==1.0.6 prove parity, (3) lock-diff fresh transitives, (4) live FPC smoke — all folded (see ledger). Full transcript: `audit-codex.md`.

## Fable audit (mid, Opus 4.8 1M — Fable-role) — `conditional approve`
Conditions (1) correct e2e-proves-deployed-binary overstatement + add runtime-download gate, (2) FPC-address go/no-go + redeploy contingency [**confirmed triggered**], (3) `CRS_CACHE_VERSION` bump — all folded. Verified-fine: override local-only scoping, prove-code parity, rc.2 bb assets reachable, dist-tag/provenance. Full transcript: `audit-fable.md`.

## Final fresh-context codex pass (mid, xhigh) — `conditional approve`
5 refinements, all folded: (1) **P3a `--salt 0x0` mandatory** (script defaults to `Fr.random()`); (2) download gate runs **from `packages/accelerator/core`** (not `src-tauri`); (3) `getContract` proves deployment, **not funding** (claim receipt + smoke do); (4) local download test = **current-arch only**, not full cross-platform proof; (5) Asks reworded as A-approval, not A-vs-B-open. Confirmed the folded conditions are internally consistent + correctly sequenced (FPC redeploy before playground deploy; byte-identical prove code + separate download-path validation reconcile). Session `019f1a06-…`.

## Post-implementation hardening
Skipped (user-confirmed). Ledger flags two future-hardening items (NPM_TOKEN→OIDC; Windows bb checksum) for a later pass, not this bump.

## Seeds (finalized post-approval — drive ALL phases P1–P3, no stop; user lifted the P3 hold)
**Recommended `/loop`:**
```
/loop 15m Drive implementations-plan/aztec-5.0.0-rc.2-2026-06-30 to FULL completion (P1–P3); never idle. Each firing: read plan.md + lessons/; git status + log; PR open → gh pr view --json statusCheckRollup (no --watch); CI in-flight → gh run watch up to 10m (re-run the Playwright install-deps flake). No task? take the next plan.md step. P3 (after bump merges): (a) Sepolia wallet L1_PRIVATE_KEY funded? else SURFACE AND HOLD; (b) derive rc.2 salt=0 FPC, if node.getContract undefined on v5 testnet run `bun run packages/playground/scripts/deploy-sponsored-fpc.ts --salt 0x0` (--salt 0x0 MANDATORY); (c) dispatch publish-testnet.yml --ref main (NO skip_sdk_publish); (d) verify npm dist-tags + live bundle 5.0.0-rc.2 + FPC funded; (e) smoke the rc.2 fee path. Stuck? /codex xhigh, log it. Phase green = its plan.md gate passes → mark ✓, file lessons, print LESSONS_FILE=…, advance. 5× fail → reassess w/ codex. All ✓ → /code-review max --fix → codex post-impl audit → address high/critical → wrap-up → stop. HARD LIMITS: never push to main (branch+PR+auto-merge); never print the L1 key; never expand scope; Sepolia-empty is the only hold.
```
**Alternative `/goal`:** see chat / eli5.html (P1–P3 all ✓ + published + FPC deployed+funded + playground smoked + reviews clean; never push main; never print the L1 key; Sepolia-empty → hold).
