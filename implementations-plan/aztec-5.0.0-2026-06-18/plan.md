# Plan — `@aztec/* 4.3.1 → 5.0.0-rc.1` bump + release (Aztec 5.0 hard fork)

**Tier:** `/blueprint deep` (rubric 5/6 HIGH: blast radius, irreversibility, migration cost, external coupling, security sensitivity; novelty MED). **Status:** awaiting approval.
**Streams consolidated:** main draft (`draft-main.md`), Opus subagent, Codex (`xhigh`, session `019edae5-d0c7-7313-a05b-2e2ef4d01091`). Fable was unavailable (mythos outage) — Opus substituted for both the Fable plan and the Fable audit half.

**Treat this as a hard-fork migration, not a dependency bump.** "It compiles" is not sufficient evidence; the real gate is that the accelerator's native bb proves a 5.0 tx the network mines.

## Release shape — REVISED per user (2026-06-18): SDK-only by default
The already-deployed accelerator app fetches+runs arbitrary bb versions **at runtime**, SDK-driven: `/prove` reads the SDK's `x-aztec-version` header (`prove.rs:128`) → `resolve_version` flags uncached versions (`:75`) → `download_bb` fetches that version's tarball and integrity-checks it via `fetch_github_asset_digest` (`downloader.rs:38`) — a path **separate from** the build-time `WINDOWS_BB_CHECKSUMS` hardcode. `is_valid_version` already accepts `5.0.0-rc.1` (`version_policy.rs:195` + passing test `:231`). **So a 5.0 SDK makes the deployed accelerator transparently fetch+run 5.0 bb with no app rebuild — IFF the bb `prove` CLI + msgpack-input + proof-output interface is unchanged 4.3.1→5.0** (`bb.rs:99-114` invokes `bb prove --scheme … --ivc_inputs_path …`). That single condition is verified empirically by P4 (the existing accelerator-backed local-5.0 E2E). If P4 is green → **SDK-only release**; the accelerator rc+stable cuts are a **contingency** (Appendix C) triggered only if the bb interface broke.

## Locked decisions (from the user)
- **Release: SDK npm publish only** (default). Accelerator rc+stable = contingency if P4 fails.
- **Min-age:** documented command-scoped `--minimum-release-age=0` override (not waiting to 06-22).
- **No new live-testnet CI gate** — validate via the existing local-5.0 accelerator E2E + an optional manual v5 browser smoke.
- **Sponsored FPC:** use the **default canonical (salt=0)** FPC if the v5 testnet pre-deployed+funded it (recon P0); skip the self-hosted deploy/fund/salt-rotation + the FeeJuice-`0x05`→`0x03` script fixes off the critical path.
- **Hardening:** NO whole-repo `/harden`; rely on this plan's codex+opus audits.

## Verified ground truth (facts)
1. `@aztec/*@5.0.0-rc.1` is published (stdlib, bb.js, aztec.js, bb-prover, foundation).
2. **Min-age blocker:** `5.0.0-rc.1` published 2026-06-15; `bunfig.toml minimumReleaseAge=604800` refuses `bun add/update` until **2026-06-22**. Inert under `--frozen-lockfile` (CI unaffected).
3. `5.0.0-rc.1` is a **bare version** — npm `rc` dist-tag → `4.3.0-rc.1` (a *downgrade*), `latest` → `4.3.1`. **Pin the exact string everywhere; never a dist-tag; never auto-detect.**
4. v5 testnet is live and runs the 5.0 protocol: `node_getNodeInfo` → `classRegistry=0x..01`, `instanceRegistry=0x..02`, `feeJuice=0x..03`; `l1ChainId=11155111` (Sepolia).
5. **No Noir contracts in this repo** → the entire `[Aztec.nr]/[Protocol]/[L1]` changelog bulk is not code we migrate; it matters only as runtime protocol compat through bb + prebuilt `@aztec/noir-contracts.js`.
6. **Runtime bb path = the SDK-only enabler.** `_e2e.yml` with `build_accelerator=true` runs the SDK against a real accelerator sidecar + local sandbox — after the bump that sandbox is 5.0, so this *existing* harness is the parity check (no new gate). `deploySchnorrAccount` (`e2e-helpers.ts:43`) currently returns without `.wait()` and tests assert only `toBeDefined()` — a false-green to fix so P4 actually asserts mining.

---

## Phases (each ends in a real validation gate)

### P0 — Recon + decision resolution (no code changes) ✓
The break surface is whatever the 5.0 `.d.ts` says, **not** what the changelog/brief says (two brief claims were already falsified — see ledger). In a throwaway dir **outside the repo** (min-age + lockfile untouched), capture the 4.3.1→5.0 signature diff for every imported symbol:
- `BBLazyPrivateKernelProver` (`@aztec/bb-prover/client/lazy`) — constructor arity + `createChonkProof` sig. SDK does `super(simulator)` (`accelerator-prover.ts:94`) and the unit test spies `createChonkProof`. Codex/Opus infer 5.0 added an *optional* 2nd ctor arg (source-compatible) — **compile-verify, don't assume**.
- `WASMSimulator`/`CircuitSimulator` (`@aztec/simulator/client`) — `new WASMSimulator()` still default-constructs (`accelerator-prover.ts:33`).
- `serializePrivateExecutionSteps`/`PrivateExecutionStep` (`@aztec/stdlib/kernel`) — msgpack wire format; a change silently breaks `/prove` byte-compat.
- `ChonkProofWithPublicInputs` (`@aztec/stdlib/proofs`) — `fromBuffer`/`toBuffer` + byte layout vs native bb output.
- Playground: `getTxReceipt` union (`isPending`/`isDropped`/`isMined`), `EmbeddedWallet.create`, `TokenContract.deploy`, `SponsoredFPCContract`. (`deploy-sponsored-fpc.ts`/`batch-fund-fpc.ts` are contingency-only — see P3/Appendix C.)
- Cross-check upstream migration notes + `lazy.ts`/`browser.ts` (codex's source links).
- **v5 canonical FPC check (decides Ask #3):** query the v5 node for the salt=0 `SponsoredFPCContract` instance (derived from the 5.0 artifact) — is it published+funded? If yes, the default FPC path needs no deploy/fund/salt-rotation.
- **bb-CLI-parity note:** the SDK-only-vs-accelerator-release decision is NOT resolved by `.d.ts` reading — it is decided empirically by P4 (does the deployed accelerator's `bb prove --scheme … --ivc_inputs_path …` against runtime-fetched 5.0 bb produce a mined tx). P0 only records the hypothesis; P4 is the verdict.
- Resolve remaining Asks (§Assumptions) with the user.

**Validation gate:** a written signature-diff table — every imported symbol classified `none | change X`, zero "unknown"; the v5 canonical-FPC check answered; Asks resolved. Recorded in `lessons/phase-0.md`.

### P1 — Mechanical bump (package.json + lockfile) ✓
Do the bump **locally**, commit the resolved `bun.lock`, let CI run `--frozen-lockfile` only (keeps min-age protection on CI; avoids the bare-`bun install` leak in `_aztec-update.yml`/`deploy-app`).
- `bun scripts/update-aztec-version.ts 5.0.0-rc.1` → patches sdk + playground `package.json`. **Assert zero skipped packages** (`findMissingPackages` silently leaves un-found packages at 4.3.1 — partial-bump landmine).
- Regenerate lockfile per the chosen min-age strategy (Ask #1): wait to 06-22 (`bun install`) **or** command-scoped `bun install --minimum-release-age=0` (local only, documented in the PR body; never edit committed `bunfig.toml`).
- **Skip-oracle vs min-age split (audit, verified):** `update-aztec-version.ts findMissingPackages` uses `npm view`, which queries the registry directly and **bypasses min-age**. Before 06-22 it reports **zero skips** (package exists since 06-15) and rewrites every `package.json`, after which `bun install` fails the min-age check. So the "zero skips" gate passes while the install fails — different availability oracles. Whoever runs P1 before 06-22 must expect this; it is the concrete signal that strategy A (wait) or the scoped override is required.
- Pin exact `5.0.0-rc.1` (no `^`/`~`); verify the playground `viem` override `npm:@aztec/viem@2.38.2` still resolves under 5.0.
- Update stale hosts/salts now: `packages/playground/package.json:8` `dev:testnet` host → `v5.testnet.rpc.aztec-labs.com` **and** its inline `SPONSORED_FPC_SALT=0x2a0f…` (a v4 salt) → the v5 salt; FPC scripts' testnet defaults; the `TESTNET_AZTEC_NODE_URL` secret (Ask #3).
- **Do not run `aztec-stable.yml` in auto-detect mode** (would resolve `4.3.0-rc.1`). The auto-update bot's bare `bun install` (`_aztec-update.yml:120`) is itself min-age-blocked for `5.0.0-rc.1` until 06-22 — a correctness fact, not just a security aside.

**Validation gate:**
```
grep -rn '"@aztec/' packages/*/package.json   # all == 5.0.0-rc.1, none 4.3.1
bun install --frozen-lockfile                  # clean, no min-age error
bun run lint && bun run lint:actions
```
Pass: no 4.3.1 remains; frozen install clean; update script reported zero skips.

### P2 — SDK migration (`@alejoamiras/aztec-accelerator`) ✓
Drive from the P0 table. Likely small if the prover ctor/`createChonkProof` are stable.
- Apply each migration action; fix imports if moved.
- **Version-handshake — corrected by audit (NOT a Rust bug):** the SDK comment `accelerator-prover.ts:382` ("the server's `is_valid_version` rejects non-alphanumeric characters") is **false** — `version_policy.rs:187-196` explicitly allows `.`/`-`/`_`, and `version_policy.rs:228-238` is a **passing test** asserting `is_valid_version("5.0.0-rc.1")==true`. So there is **no validator fix to make**; instead (a) fix the misleading comment at `accelerator-prover.ts:382`, and (b) re-aim the silent-WASM-fallback audit at the *real* fallback paths: `#classifyHealth` legacy version-mismatch (`accelerator-prover.ts:240-257`, normalized-form mismatch → `available:false` → WASM) and the 403-denial path (`:312-324`). The positive "native path was used" assertion in P4 is what actually catches a silent fallback.

**Validation gate:**
```
bun run --cwd packages/sdk test:lint && bun run --cwd packages/sdk test:unit && bun run --cwd packages/sdk build
```
Pass: green; `dist/index.d.ts` exports the public types intact.

### P3 — Playground + scripts migration
- **`aztec.ts:391` reverted-mined fix (codex):** today any non-pending/non-dropped receipt = success. v5 has **mined-but-reverted** as a distinct state — reject reverted mined receipts, don't silently pass. Re-derive `isPending`/`isDropped`/`isMined` from the 5.0 union (the brief's "add `.isMined()`" was wrong; the code already narrows — verify the methods still exist).
- **Use the default canonical FPC (salt=0) — no deploy/fund:** `initializeFPC` (`aztec.ts:178-189`) uses salt=0 when `SPONSORED_FPC_SALT` is unset and only *registers* the instance (doesn't deploy). If P0 confirms the v5 testnet pre-deployed+funded its canonical SponsoredFPC, **drop the inline `dev:testnet` salt entirely** and rely on salt=0. The FeeJuice-`0x05`→`0x03` fixes in `deploy-sponsored-fpc.ts:110`/`batch-fund-fpc.ts:212` are then **contingency-only** (Appendix C — only if we must self-host a private FPC). Re-derive any deploy call shape that 5.0 moved to construction-time (`contractAddressSalt`/`universalDeploy`).
- Confirm vite proxy COOP/COEP (`credentialless`) still permits bb.js workers + the new RPC origin.

**Validation gate:**
```
bun run lint && bun run test
bun run --cwd packages/playground build
bun run --cwd packages/playground test:e2e     # 28 Playwright mocks (network-free)
```
Pass: all green. (Mocked E2E cannot prove the testnet path — that's the manual v5 smoke in P5.)

### P4 — Proving-parity = the SDK-only decision gate (crypto heart of the fork)
This phase answers the user's "to be checked": does the **deployed** accelerator's native bb prove a 5.0 tx the network mines? Verified **empirically** by the *existing* accelerator-backed E2E against a **local 5.0 sandbox** — no new infra. **Audit-blocking precondition: the current harness proves nothing about mining.** `deploySchnorrAccount` (`e2e-helpers.ts:31-43`) calls `.send(...)` and returns **without `.wait()`**; the "Accelerated"/"Local (WASM)" tests (`proving.test.ts:79-104`) assert only `toBeDefined()`. So:
- **First** add a mined+revert-aware `.wait()` to `deploySchnorrAccount` (assert non-pending, non-dropped, **not reverted**). Until this exists, the gate asserts nothing.
- Run **both legs against the local 5.0 sandbox**: native via the real accelerator sidecar (`build_accelerator=true`, which exercises the runtime `download_bb`→`bb prove --scheme … --ivc_inputs_path …` path on 5.0 bb); forced-WASM via `setAcceleratorConfig({ port: 1 })` (offline→fallback path at `:198,277`) — **not** `setForceLocal(true)` (short-circuits before `/health`). Assert both mine.
- **Positive "native path was used" assertion (codex — the key catch):** a mined tx does NOT prove the accelerated leg used native bb (it could have silently fallen back to WASM and still mined). The accelerated leg must assert it hit the native accelerator path (spy/observe the `/prove` round-trip to `:59833`, or assert `/health` classified `available` + the proof came from the sidecar). **This is what proves the bb CLI/msgpack/proof interface is parity-stable** — i.e. that SDK-only is valid.
- Byte-for-byte `ChonkProofWithPublicInputs` equality stays a **non-blocking diagnostic** (IVC proofs may carry randomness).

**Validation gate:** the accelerator-backed local-5.0 E2E (`_e2e.yml build_accelerator=true`, or local `bun run --cwd packages/sdk test:e2e` with the sidecar) — **the accelerated leg mines a 5.0 tx AND provably used the native `:59833` path; the WASM leg mines too.** 
- **Green → SDK-only release is valid** (the deployed accelerator handles 5.0 bb transparently). Proceed to P5–P7.
- **Red (bb CLI/input/output broke) → escalate to Appendix C** (the accelerator must be rebuilt+rereleased). Surface to the user before doing so.

### P5 — Full local sweep + manual v5 smoke (no new CI gate)
```
bun run lint && bun run lint:actions && bun run test
bun run --cwd packages/sdk test:lint && bun run --cwd packages/sdk test:unit && bun run --cwd packages/sdk build
bun run --cwd packages/playground test:e2e          # 28 mocked
# WebDriver (mac+linux): the 9 tauri-plugin-webdriver tests via _e2e-webdriver.yml (accelerator unchanged → should stay green)
```
**Manual v5 acceptance smoke (human, not a built gate — per user):** `bun run --cwd packages/playground dev:testnet` against `v5.testnet.rpc.aztec-labs.com` with the **default salt=0 FPC**; deploy + prove (native accelerator) + send; confirm it **mines** in the browser. This is the live-network acceptance the user wanted, run by hand. **Gate:** every command green + the manual v5 smoke mines.

### P6 — Land the bump PR on `main`
`main` is branch-protected (branch + PR + auto-merge; unsigned via `git -c commit.gpgsign=false`). CI runs `--frozen-lockfile` (min-age inert). **Gate:** `sdk.yml` + `accelerator.yml` + `app.yml` + `actionlint.yml` green; PR auto-merges. (Same-SHA-across-three-cuts machinery is **not needed** for SDK-only — there is one release artifact, the SDK publish, dispatched from merged `main`.)

### P7 — SDK npm publish (the release)
Dispatch `publish-testnet.yml` from merged `main`. **Corrections that still apply:**
- **Default `_e2e.yml` is local-sandbox + no accelerator** (`publish-testnet.yml:33-36`). For an SDK-only cut that's acceptable *because P4 already empirically proved 5.0 proving parity on this code* — but for stronger CI evidence, pass `build_accelerator=true` to that `_e2e.yml` call so the publish gate exercises the accelerator path on the local 5.0 sandbox (no v5 wiring → not "building the live gate"; just turning on the existing accelerator leg).
- **`publish-testnet.yml` is NOT npm-only — it also redeploys the playground** (`:48-84`: S3 + CloudFront) against `TESTNET_AZTEC_NODE_URL`/`SPONSORED_FPC_SALT`. Rotate `TESTNET_AZTEC_NODE_URL` → v5 host first; with the default salt=0 FPC, `SPONSORED_FPC_SALT` can be **unset/removed** (Ask #3). Or split SDK publish from the app deploy for this cut.
- **`latest: true → false`** while deps are rc-labeled (Ask #2b): publish on `testnet`, do NOT claim npm `latest`. `get-sdk-publish-version.ts` derives the version from `@aztec/stdlib` (`5.0.0-rc.1`; dot-appends if taken). `_publish-sdk.yml` always `gh release --latest=false`.
- **Irreversibility:** an npm publish can't be unpublished — if something's wrong post-publish, fix-forward to the next dot-appended revision.

**Gate:** the (accelerator-on) local-5.0 E2E green; package on npm at the resolved version; Sigstore provenance attached; dist-tag `testnet` (not `latest`); GH release `--latest=false`; playground redeploy points at v5 with the default FPC.

---

## Appendix C — Contingency: accelerator rc+stable release (only if P4 is RED)
Triggered **only** if P4 shows the bb `prove` CLI / msgpack-input / proof-output interface changed in 5.0 such that the deployed accelerator can't serve a 5.0 SDK. Then the accelerator must be rebuilt and re-released, and the full deep-tier release machinery (audited earlier) comes back:
- **Windows bb pin:** add the `barretenberg-amd64-windows.tar.gz` SHA-256 to `WINDOWS_BB_CHECKSUMS` (`copy-bb.ts`), keyed on the *resolved* `@aztec/bb.js` version, fail-closed, **dual-sourced** by a named human from the `aztec-packages` release tag.
- **Same-SHA enforcement:** add a `ref`-SHA input to `release-accelerator.yml`/`publish-testnet.yml` so rc/SDK/stable share one verified SHA.
- **rc dress-rehearsal** (`version=1.0.x-rc.N`, `--prerelease`) → **joint stable** (first real `is_prerelease=='false'` run: latest.json/S3/verify-live-feed/bump-source). **Stage the N-1 latest.json rollback** before the stable cut (re-upload N-1 + CloudFront invalidation = ≤5-min exposure); the rc does NOT de-risk the stable-only steps (1.0.5 incident class) — add a no-upload latest.json preflight on rc.
- Whichever bb-CLI change broke parity must also be fixed in the accelerator's Rust invocation (`bb.rs:99-114`).

---

## Security & Adversarial Considerations
*(SDK-only path. The auto-update-feed / Windows-bb-pin / updater items below are **contingency-only**, relevant if Appendix C fires.)*
- **Threat model:** for SDK-only, the npm supply chain (bump window + the publish) is the primary surface; the runtime bb fetch is the prover trust root. The auto-update feed only enters scope under Appendix C.
- **Runtime bb fetch (now in-scope, was build-time):** the deployed accelerator fetches 5.0 bb at runtime and verifies it via `fetch_github_asset_digest` (`downloader.rs:38`) — GitHub-control-plane trust, not a publisher signature (pre-existing property, unchanged by this bump). P4's parity gate is the runtime backstop that the fetched 5.0 bb actually produces network-valid proofs.
- **Supply chain (highest leverage):** min-age (604800s) is the worm-window defense. The scoped override is **not "safe," it weakens posture for the override window** (a too-fresh malicious tarball gets locked into `bun.lock` permanently) — accepted only because: command-scoped (`--minimum-release-age=0`), local-only, documented in the PR body, never committed, never a CI/org variable; CI stays `--frozen-lockfile` (frozen ignores min-age); the exact `5.0.0-rc.1` pins are manually reviewed in the PR diff; and the parity gate (P4b) independently verifies the prover bytes against the network. The bare `bun install` in `_aztec-update.yml`/`deploy-app`/`publish` is the leak vector — confirm none runs the override.
- **bb.exe fetch:** Windows bb has no upstream checksum — the in-repo SHA-256 is the sole anchor. **A named human** (the PR author) downloads `barretenberg-amd64-windows.tar.gz` from the `aztec-packages` GitHub release tag `v<resolved-bb.js-version>` (confirm RCs are tagged `v5.0.0-rc.1`-style; a tag mismatch fails the fetch, not silently), computes the SHA-256 **on two independent machines/networks**, confirms they match, and pins that. Fail-closed on unknown version/mismatch; the `bb.exe`-only canary catches a DLL-dropper. macOS/Linux bb rides the npm tarball (min-age + provenance), but it's brand-new fork-day code → the parity gate (P4b) is the runtime backstop.
- **Crypto / hard fork:** Schnorr→Poseidon2 + domain separators make a 4.x proof invalid on 5.x. The danger is **silent WASM fallback masking incompatibility** (user sees "slower," not "broken"). Mitigate: strict `/health` version match; parity gate asserts both legs *mine on-chain*; release notes state the hard requirement 5.0 accelerator ⇔ 5.0 network ⇔ 5.0 SDK. Audit the fallback path so a 5.0 SDK never silently consumes a 4.x accelerator's proof.
- **Updater (least privilege):** keep the Ed25519/minisign gate + pre-flight size cap; keep the `update-smoke` **negative** legs BLOCKING (prove a tampered artifact is rejected). `bump-source` uses a short-lived repo-scoped App token — verify scope hasn't widened.
- **Release pipeline:** `contents: read` at root; `actions: write` only on the watchdog; `id-token: write` only where OIDC/provenance is needed — don't broaden during the bump.
- **Release integrity (contingency-only):** if Appendix C fires, rc/SDK/stable must share one SHA via a `ref` input. For SDK-only there is a single artifact, so this doesn't apply.
- **Accelerator app surface (unchanged by this bump but must still pass):** loopback Host-allowlist (anti-DNS-rebinding), deny-by-default origin approval, Origin-tiered `/health` — the WebDriver gate (P5) must confirm green post-bump.

## Assumptions
**Facts** (verified on disk): the 6 ground-truth items above; `update-aztec-version.ts findMissingPackages` uses `npm view` (bypasses min-age — skip/install oracle split); `_publish-sdk.yml:136` always `--latest=false` on the GH release; `get-sdk-publish-version.ts` dot-appends prerelease revisions; stable-only steps gated `is_prerelease=='false'`; FPC scripts hard-code FeeJuice `0x05` (`deploy-sponsored-fpc.ts:111`, `batch-fund-fpc.ts:212`; v5=`0x03`); `aztec.ts:391-397` treats non-pending/non-dropped as success (mined-reverted slips through); `dev:testnet` (`playground/package.json:8`) hard-codes host + a v4 `SPONSORED_FPC_SALT`; `publish-testnet.yml` calls `_e2e.yml` with no v5 URL (defaults local sandbox) AND redeploys the playground (`:48-84`); **`version_policy.rs:195` accepts `.`/`-`/`_` and a passing test (`:231`) asserts `is_valid_version("5.0.0-rc.1")==true`** — the SDK comment `accelerator-prover.ts:382` claiming otherwise is false; `deploySchnorrAccount` (`e2e-helpers.ts:43`) returns without `.wait()` and tests assert only `toBeDefined()`.
**Inferences** (attack these in audit): `BBLazyPrivateKernelProver` added an *optional* 2nd ctor arg → SDK source-compatible (promising, not dispositive — compile-verify against the published artifact); msgpack wire + ChonkProof byte layout stable enough for the unchanged transport (a fork is exactly when this breaks — treat byte-equality as diagnostic only); `EmbeddedWallet` auto-preload → no playground change; the foundry-v1.4.1 pin + forge-rename are local-L1-only and skipped for a remote v5 node — but they **still matter for the Sponsored-FPC bootstrap tooling**, so don't treat "local-only" as "irrelevant."
**Asks** — RESOLVED by the user 2026-06-18:
1. **Min-age:** ✅ documented command-scoped `--minimum-release-age=0` override (local-only, PR-documented). [P1]
2. **Release shape:** ✅ **SDK-only**; accelerator rc+stable = Appendix C contingency (fires only if P4 RED). So "stable-on-rc-deps" (old 2a) and "accelerator version #" (old 5) are **moot** unless Appendix C fires. SDK npm dist-tag (old 2b): ✅ `testnet`, **not** `latest` (flip `publish-testnet.yml latest:true→false`). [P7]
3. **Sponsored FPC:** ✅ use the **default canonical salt=0** FPC — *pending the P0 v5-node check* that it's published+funded. If confirmed: no deploy/fund, unset `SPONSORED_FPC_SALT` (script default + inline `dev:testnet` salt + CI secret), and the FeeJuice-`0x05`→`0x03` fixes drop off the path. Only rotate `TESTNET_AZTEC_NODE_URL`→v5. [P0/P3/P7]
4. **No new live-testnet CI gate:** ✅ removed. Live-network acceptance is the **manual v5 smoke** in P5; automated parity is the existing local-5.0 accelerator E2E (P4). Same-SHA `ref` machinery is **not needed** for SDK-only (one artifact). [P4/P5]

**Remaining open item (recon, not a user decision):** the P0 v5 canonical-FPC check, and the **P4 verdict** (SDK-only confirmed vs Appendix C). Both resolve during implementation, not at the gate.

## Decision ledger
- **Structure** = main's recon-first phasing, extended with Opus's "build the missing live-E2E as a phase" and codex's same-SHA/SDK-publish-first ordering.
- **getTxReceipt:** all three independently rejected the brief's "add `.isMined()`" — code already narrows. Codex added the sharper point: handle **mined-but-reverted** (adopted, P3).
- **Parity dispute (codex byte-equality vs opus on-chain-mine):** resolved → on-chain-mine is the blocking gate; byte-equality is a non-blocking diagnostic. Rationale: IVC proofs may be randomized; bit-equality isn't guaranteed and would produce false release-blocks.
- **Min-age:** kept as a user Ask (override is *technically* safe with the constraints, but it trades against the repo's own default — user's call).
- **Auto-detect downgrade** (Opus), **is_valid_version rejects rc string** (Opus), **FeeJuice 0x05→0x03** (codex), **update-aztec-version skips** (Opus): all adopted as concrete P1–P3 items.
- **Rejected:** byte-equality as a *blocking* gate (would false-block); editing committed `bunfig.toml` min-age (supply-chain regression); using `aztec-stable.yml` auto-detect (wrong version).
- **`is_valid_version` reversal (honest correction):** the first Opus stream *inferred* the server might reject the rc string (from the false SDK comment at `accelerator-prover.ts:382`); I surfaced it as a finding. The fresh-context audit *verified it false* by reading `version_policy.rs:195` + its passing test `:231`. The inferred finding was **withdrawn**; P2 now does verify-only + fixes the comment + re-aims the fallback audit at the real paths (`:240`, `:312`). Lesson: a stale source comment is not ground truth.
- **Audit round outcomes (codex resume + fresh Opus, both adversarial + assumption-attack):**
  - **Codex:** *conditional approve* — conditions: narrow the testnet override to "same-SHA already green" (folded, P5); enforce same-SHA via `ref` input not just freeze (folded, P7); add a positive "native path used" parity assertion (folded, P4b). Plus min-age-wording, Windows-SHA-verifier, latest.json-runbook, SDK/stable-fail recovery, split the rc asks — all folded.
  - **Fresh Opus:** *conditional approve* — 5 blocking conditions, all folded: (1) strike the false `is_valid_version` premise; (2) the parity gate is `toBeDefined()` today → add a mined+revert-aware `.wait()` to `deploySchnorrAccount` first; (3) P9 publishes on local-sandbox evidence → wire `publish-testnet.yml`'s `_e2e.yml` to v5 or gate on P5; (4) `publish-testnet.yml` also redeploys the playground → rotate v5 secrets first; (5) note the skip-oracle/min-age split in P1.
  - **Final fresh-context codex pass (Phase 5, on the revised plan):** *conditional approve* — confirmed P2/P4b/P5/P9 folds landed; two tightenings folded: same-SHA `ref` input is now a **hard prerequisite** (P7, dropped the human-freeze fallback), and P9's v5 gate is now **concrete inside `publish-testnet.yml`** (its own `e2e` job → v5 + `build_accelerator` + `ref`, with `publish-sdk`+`deploy-app` depending on it). Plus a non-blocking 403-denial regression test (P9). No remaining blocking findings.
- **SDK-only pivot (user, 2026-06-18, post-audit):** the user observed the deployed accelerator downloads bb at runtime, so an SDK-only release may suffice. **Verified in code:** `/prove` is version-driven (`prove.rs:128`), `download_bb` fetches+digest-verifies arbitrary versions at runtime (`downloader.rs:38`) — separate from the build-time Windows-SHA hardcode — and `is_valid_version` already accepts `5.0.0-rc.1`. So the plan pivoted from "rc→stable accelerator + SDK" to **SDK-only**, conditioned on P4 (the existing accelerator-backed local-5.0 E2E) empirically proving the `bb prove` CLI/msgpack/proof interface is parity-stable. The full accelerator-release machinery (Windows SHA, same-SHA `ref`, latest.json/S3, rollback runbook) moved to **Appendix C** (fires only if P4 RED). The user also dropped the new live-testnet CI gate (→ manual v5 smoke) and confirmed the min-age override + default salt=0 FPC. This shrinks the effort from ~deep to ~mid in practice; the deep audit trail still stands behind the migration + parity rigor.
- **Honest deviation from `deep` spec:** Fable was down → Opus substituted for the Fable plan + audit. The contradiction-check (Phase 3) and double-audit (Phase 4) were **compressed into one cross-family review round** (codex resume + fresh Opus), followed by the mandated **fresh-context final codex pass** (Phase 5, on this revised plan). Cross-family coverage + anchor-free final pass preserved.

## Seeds
See `eli5.html` for the `/goal` (recommended) and `/loop` implementation seeds. Finalized post-approval.
