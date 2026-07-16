# Codex audit — Round 1 (mid, xhigh, gpt-5.6-sol, read-only)

Session: 019f6b8c-a5a0-74f2-9764-bb0c705a6e62 — 2026-07-16. Verdict: **reject** (5 HIGH + 2 MEDIUM; recommends strengthened-B/hybrid). Post-audit verification notes: the npm attestations ARE retrievable (2: npm-publish v0.1 + SLSA provenance v1 — codex's sandbox couldn't fetch); the skipped token tests cite CI time (~7 min WASM flow), not flakiness. Dispositions in plan.md ledger.

Paths: `P501` = `implementations-plan/aztec-5.0.1-2026-07-16/plan.md`; `P500` = preceding stable plan.

- **[HIGH — Facts / supply chain]** Provenance is overstated as equivalent to `@aztec/*` trust (`P501:79,82`). npm explicitly says provenance links an artifact to source/build instructions; consumers must still audit them. It does not prove code review, tag legitimacy, workflow safety, reproducibility, or bytecode semantics. The attacker targets the new scope’s OIDC trust, attested workflow/ref, generated JS—which executes in the playground origin—and embedded contract bytecode. “Zero dependencies” does not mitigate those. “Audited” is unsupported; current [Aztec documentation](https://docs.aztec.network/developers/docs/aztec-nr/standards) still describes the implementation as Wonderland-maintained and distinct from Aztec’s reference token. Before install, record the attestation subject digest, repository/commit/workflow, reviewed release tag, tarball integrity, and evidence that packaged artifact/bytecode derives from that commit. The live npm attestation itself was not independently retrievable in this environment, so `P501:92` currently rests only on session testimony. [npm provenance limitations](https://docs.npmjs.com/generating-provenance-statements/)

- **[HIGH — Facts / min-age]** `--minimum-release-age=0` disables the policy for the entire resolution, not two named packages (`P501:42`). Exact root pins and “one new root entry” do not prevent newly selected transitives from changed `@aztec` manifests. “Flag anything after July 9” also permits unexplained older lock changes. Generate without lifecycle scripts, review every changed resolution and integrity—not merely non-`@aztec` roots—verify signatures/provenance for the intended young artifacts, then perform the frozen install.

- **[HIGH — Gates]** The claimed automated token coverage is false (`P501:45`). Production smoke only loads UI/modules and checks resources (`packages/playground/e2e/demo.production-smoke.spec.ts:12,29`); testnet smoke is explicitly deploy-only (`demo.smoke.spec.ts:2-6`); both local token tests are skipped (`demo.local-network.spec.ts:42-46,69-73`). P3b is therefore the sole behavioral check, and P3d never reruns token flow on the deployed CloudFront bundle (`P501:60-65`). Add a non-skipped standards-token integration test plus a live post-deploy 500/500 token flow before deprecation.

- **[HIGH — Inferences]** Deep-path resolution, constructor semantics, and authorization remain unresolved (`P501:20,40,103-105`). Absence of `exports`/`main` neither proves the proposed path is the only option nor validates browser behavior. `deployments.json` is not authoritative for `auth_contract=ZERO`; the fallback constructor materially changes the narrative and may not authorize the later mint. Require source/tests at the attested commit proving ZERO, nonce `0`, and minter rules; add negative assertions that Bob cannot mint or spend Alice’s balance. Any fallback requires an owner Ask.

- **[HIGH — Inferences / compatibility]** “Node upgrade optional” supports protocol/node compatibility, not all client-state/message compatibility (`P501:22,102`). A only tests fresh 5.0.1 client + 5.0.1 standards bytecode. It does not test old 5.0.0 token notes/handshakes after the new domain separator. Preserve and run the old-token flow under the 5.0.1 client, including reload/resync if state persistence is claimed, before swapping.

- **[MEDIUM — Change assessment]** The release inventory is incomplete. It omits future-dated-message rejection #24645 and fee-payer phase enforcement #24479; the former belongs in the hostile-input threat model, while the latter must be classified against the precompiled standards/FPC artifacts. #24636/#24662/#24629/#24579/#24476 appear N/A but should be explicitly dispositioned. Also, identical kv-store file layout does not prove #24631 inert, and the inherited lesson says ephemeral mode still boots SQLite (`lessons/phase-1.md:7,14`). [v5.0.1 release notes](https://github.com/AztecProtocol/aztec-packages/releases/tag/v5.0.1)

- **[MEDIUM — P4]** `npm whoami` proves identity, not package ownership or credential suitability (`P501:70`). Add `npm owner ls`, use interactive OTP rather than command-line OTP, enumerate all old versions, and verify every version received the exact warning—not only 5.0.0 (`P501:73`). Deprecation messages are install-time social-engineering surface, not merely harmless metadata. [npm deprecate](https://docs.npmjs.com/cli/v11/commands/npm-deprecate/)

**Approach:** strengthened B/hybrid beats A: first ship the pure 5.0.1 bump, old-token interoperability smoke, SDK publish, and promotion; then independently verify/integrate standards, smoke before and after playground-only deployment, and finally deprecate. This ships the security patch sooner and provides a compatibility matrix A discards.

**Verdict: reject (with blocking findings: provenance/source-to-bytecode verification, resolution-wide min-age audit, authoritative constructor/auth semantics, automated plus post-deploy token coverage, and old-token 5.0.1-client interoperability must be added; adopt the strengthened B/hybrid sequencing).**
---

# Codex final fresh-context pass (mid, xhigh, read-only) — `conditional approve`, all 5 conditions folded

Session: 019f6b9c-e640-7483-a18d-9369a3ae21cd — 2026-07-16. Saw the consolidated plan + both prior transcripts + ledger.

Consolidation result: every prior item is mentioned, but four are not operationally folded.

- **[HIGH — contingency]** A′ does not yet capture B’s isolation. P3b occurs after merge; publishing is explicitly from `main`, so reverting requires another branch/PR/CI/merge. More importantly, the fallback can publish `testnet`, but P3d/e still unconditionally require the standards flow before promoting `latest`; an unfixable swap therefore still holds the main security channel hostage ([plan.md:61](implementations-plan/aztec-5.0.1-2026-07-16/plan.md:61), [plan.md:63](implementations-plan/aztec-5.0.1-2026-07-16/plan.md:63)). The `skip_sdk_publish` lever is real and deploys the dispatch SHA ([publish-testnet.yml:48](.github/workflows/publish-testnet.yml:48), [publish-testnet.yml:60](.github/workflows/publish-testnet.yml:60), [publish-testnet.yml:67](.github/workflows/publish-testnet.yml:67)). Specify: revert PR → merge → fresh old-token flow → publish, accept, and promote 5.0.1 → later revert-the-revert PR → merge → playground-only dispatch → standards production smoke → P4.

- **[MEDIUM — provenance fold]** Rejecting full reproducible compilation is proportionate for throwaway demo assets with no user funds and pre/post live execution. But Codex’s crucial subject-binding condition was silently weakened: P1 records provenance metadata and lock integrity separately, without requiring the attestation subject digest to match the installed tarball ([audit-codex.md:7](implementations-plan/aztec-5.0.1-2026-07-16/audit-codex.md:7), [plan.md:42](implementations-plan/aztec-5.0.1-2026-07-16/plan.md:42)). Require that comparison and review the pinned release workflow before acceptance.

- **[MEDIUM — false gate]** `bun run test` does **not** execute `scripts/update-aztec-version.test.ts`, contrary to P1 ([plan.md:48](implementations-plan/aztec-5.0.1-2026-07-16/plan.md:48), [package.json:24](package.json:24)). App CI runs only playground unit tests ([app.yml:84](.github/workflows/app.yml:84)). Add an explicit scripts test command and CI wiring/path trigger.

- **[MEDIUM — fold honesty]** Codex’s automated/negative authorization assertions are claimed “adopted,” yet the spec may remain skipped and P1 never specifies the negative cases ([plan.md:44](implementations-plan/aztec-5.0.1-2026-07-16/plan.md:44), [plan.md:112](implementations-plan/aztec-5.0.1-2026-07-16/plan.md:112)). Either make one integration test mandatory—including Bob-cannot-mint/spend—or mark that demand rejected and rely honestly on source verification plus live gates.

- **[LOW — assumptions/completion]** `Contract.at` alone is not a deployment fallback ([plan.md:101](implementations-plan/aztec-5.0.1-2026-07-16/plan.md:101)). Also, the goal treats a P4 auth hold as completion despite “Done” requiring deprecation ([plan.md:136](implementations-plan/aztec-5.0.1-2026-07-16/plan.md:136)). Mark it blocked, not ✓.

The old-state interoperability rejection is sound: this playground has no persistent cross-version client state.

**Verdict: conditional approve (with conditions: repair the contingency ordering and merge mechanics; cryptographically bind provenance to the installed tarball; make the bump-tool test a real gate; resolve the falsely-adopted automated/negative coverage; and correct fallback/completion semantics).**
---

# Post-implementation audit (fresh context, xhigh, 2026-07-16) — session 019f6c18-5648-7992-bc2e-b4a987ca6edb

Verdict: the current PR still needs fixes. I found no critical issue, one high live supply-chain gap, five medium current defects, and several lows.

The branch changed during the audit from requested `1550cea` to `a6668a0`; I audited both. The force-update fixed one high regression described below.

## Findings

1. **High — `1550cea` made every full-stack E2E fail before reaching the token flow; fixed in `a6668a0`.**  
   File: `1550cea:packages/playground/e2e/fullstack.helpers.ts:55`, [main.ts:95](packages/playground/src/main.ts:95), [current helper:54](packages/playground/e2e/fullstack.helpers.ts:54)  
   Failure: `1550cea` expected the token button enabled immediately after wallet initialization, while `main.ts` intentionally disables it until a session account exists. `beforeAll` therefore failed before `ensureSessionAccount` could deploy one; neither re-enabled token spec actually ran.  
   Fix: Already fixed at `a6668a0` by expecting the button disabled initially.

2. **High — the live standards dependency remains vulnerable to a validly attested malicious release.**  
   File: [plan.md:85](implementations-plan/aztec-5.0.1-2026-07-16/plan.md:85), [aztec.ts:20](packages/playground/src/aztec.ts:20), [bun.lock:186](bun.lock:186)  
   Failure: provenance plus matching integrity proves which workflow produced which tarball—not that the authorized commit, generated wrapper, or embedded bytecode is benign. A compromised upstream maintainer/workflow could publish arbitrary browser-origin JS or a token that preserves the happy-path 500/500 result while allowing unauthorized mint/spend. The plan’s “the code executing is ours” and “same trust class as `@aztec/*`” claims are unsafe: the standards wrapper and contract artifact execute too. This limitation is explicitly consistent with [npm’s provenance documentation](https://docs.npmjs.com/generating-provenance-statements/). There is no evidence the present package is malicious, but the gap affects the already-live bundle.  
   Fix: independently rebuild `c74541f7…` with a pinned toolchain and byte-compare the published wrapper/artifact, review the source delta, and add Bob-cannot-mint/spend integration tests.

3. **Medium — “lockstep” is fail-open and conflates package absence with registry failure.**  
   File: [update-aztec-version.ts:49](scripts/update-aztec-version.ts:49), [update-aztec-version.ts:130](scripts/update-aztec-version.ts:130), [check-aztec-update.ts:20](scripts/check-aztec-update.ts:20)  
   Failure: any nonzero `npm view` result—including 401, 429, DNS failure, timeout, or malformed registry response—is treated as “unpublished” and skipped. The standards package is absent from the preliminary availability list, and a lockstep skip only warns, producing the exact mixed-version graph the comments call unsafe.  
   Fix: require an authenticated, validated exact-version response; treat only a confirmed 404 as absent, abort stable/RC bumps on any skip, and require an explicit nightly-only override.

4. **Medium — an npm dist-tag rollback is treated as an upgrade.**  
   File: [check-aztec-update.ts:41](scripts/check-aztec-update.ts:41), [check-aztec-update.ts:77](scripts/check-aztec-update.ts:77), [_aztec-update.yml:235](.github/workflows/_aztec-update.yml:235)  
   Failure: `needsUpdate` means merely `current !== latest`. A stale, compromised, or intentionally rolled-back dist-tag can generate a dependency downgrade PR; workflows configured with `auto_merge: false` immediately attempt `gh pr merge` rather than arming auto-merge. Branch protection may stop it, but the workflow itself does not.  
   Fix: require monotonic semver for auto-detected versions, reserve downgrades for the explicit `version` input, and always use check-gated auto-merge.

5. **Medium — the release bot discards two outputs of the bump script.**  
   File: [update-aztec-version.ts:79](scripts/update-aztec-version.ts:79), [update-aztec-version.ts:160](scripts/update-aztec-version.ts:160), [_aztec-update.yml:138](.github/workflows/_aztec-update.yml:138)  
   Failure: the script updates `aztec.ts`’s CRS version and `copy-bb.ts`’s Windows checksum, but CI stages only `packages/*/package.json` and `bun.lock`. The workspace is then destroyed, leaving a stale CRS cache version and no checksum for the new Windows binary. The repaired auto-insert anchor is therefore ineffective in the bot workflow.  
   Fix: stage both derived files and fail if any unstaged diff remains after the commit.

6. **Medium — automatic Windows checksum pinning trusts the same source for bytes and truth.**  
   File: [update-aztec-version.ts:91](scripts/update-aztec-version.ts:91), [copy-bb.ts:8](packages/accelerator/scripts/copy-bb.ts:8), [release_metadata.rs:73](packages/accelerator/core/src/versions/release_metadata.rs:73)  
   Failure: an upstream GitHub release compromise can provide a malicious executable and the hash that gets committed as its “integrity anchor.” The implementation also buffers the entire response without a size limit. The Rust code already documents the same-control-plane weakness.  
   Fix: make generation print-only, enforce a size cap, and verify a publisher signature/TUF-style independent trust root when upstream offers one.

7. **Medium — the new root script typecheck is not a CI gate for script-only changes.**  
   File: [package.json:27](package.json:27), [tsconfig.scripts.json:9](tsconfig.scripts.json:9), [sdk.yml:27](.github/workflows/sdk.yml:27)  
   Failure: `sdk.yml` runs Bun tests but only typechecks the SDK. Bun transpilation can accept a root script type error, and changing only `tsconfig.scripts.json` does not trigger the workflow. Thus the new `process.argv` type hardening is protected locally but not by its designated CI pipeline.  
   Fix: run `tsc --noEmit -p tsconfig.scripts.json` in the SDK typecheck job and add `tsconfig.scripts.json` to the path filter.

8. **Low — wallet retry reset is incomplete and non-atomic.**  
   File: [aztec.ts:232](packages/playground/src/aztec.ts:232)  
   Failure: retries clear address arrays but do not stop or clear the old `EmbeddedWallet`, wallet, fee method, node, or prover. If wallet creation succeeds and FPC registration fails, later attempts can leak the old PXE; a final early failure can leave state fields referring to different initialization attempts. UI gating currently limits reachability, so this is low rather than a live functional failure.  
   Fix: stop the prior embedded wallet, clear all wallet-bound state, and commit a fully initialized state atomically.

9. **Low — Bob timing skew invalidates cross-mode token timing comparisons.**  
   File: [aztec.ts:586](packages/playground/src/aztec.ts:586), [aztec.ts:605](packages/playground/src/aztec.ts:605), [demo.local-network.spec.ts:30](packages/playground/e2e/demo.local-network.spec.ts:30)  
   Failure: the first token run includes Bob’s deployment inside `totalDurationMs`; later mode runs reuse Bob. Accelerated and local result cards therefore measure different work. “Pre-existing” explains provenance, not correctness.  
   Fix: deploy Bob before starting the token timer, or reset accounts so every compared mode includes identical setup.

10. **Low — script-triggered CI does not declare least-privilege token permissions.**  
    File: [sdk.yml:1](.github/workflows/sdk.yml:1), [sdk.yml:49](.github/workflows/sdk.yml:49)  
    Failure: PR-controlled scripts now execute in several jobs with checkout credentials and repository-default `GITHUB_TOKEN` permissions. Fork PRs are normally constrained, but same-repository PRs inherit repository settings; a permissive default creates unnecessary write capability. [GitHub recommends explicitly minimizing workflow permissions](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#permissions).  
    Fix: declare `permissions: { contents: read }` and use `persist-credentials: false` in jobs that never push.

11. **Low — the plan’s “Facts” section mixes historical baseline with current truth.**  
    File: [plan.md:95](implementations-plan/aztec-5.0.1-2026-07-16/plan.md:95), [plan.md:100](implementations-plan/aztec-5.0.1-2026-07-16/plan.md:100), [plan.md:102](implementations-plan/aztec-5.0.1-2026-07-16/plan.md:102)  
    Failure: it still says there are zero standards references, token specs are skipped, the updater lacks lockstep support, and npm tags are 5.0.0. Those were valid planning baselines, but are now presented beneath “Facts” in a post-implementation record.  
    Fix: label them “pre-implementation snapshot” and add an explicit final-state facts block.

## Explicit category results

**Correctness attack:** The standards call sites are clean. The installed declaration confirms constructor argument order; Alice is the minter and sender; `auth_contract=ZERO` disables hooks; nonce `0` is correct for the Alice self-call; and balance simulations use Alice/Bob scopes correctly. Mode changes preserve session accounts, and `proofsRequired` does not undermine `sessionAddresses`. The current occurrence-count assertion cannot pass vacuously and catches failures after a 500/500 log. The remaining correctness issues are findings 8 and 9.

**Facts:** API signatures, ZERO semantics, attestation-to-tarball binding, and release evidence check out. Misstatements are the stale baseline in finding 11 and the claim that only “our code” executes.

**Inferences:** Unsafe inferences are “same trust class,” happy-path execution as authorization evidence, and the noir-contracts deep-import analogy. The npm-auth inference was disproved by the 401, but its fail-closed hold worked.

**Asks:** Clean. I found no silently assumed product decision in the surviving ledger; dependency age, same-day promotion, standards migration, FPC funding, and the P4 auth hold were explicit.

**Skipped residuals:** Bob timing’s skip justification is not sound—see finding 9. The deep-path skip is sound for exactly 5.0.1 because the file exists, there is no `exports` restriction, typecheck/build passed, and the live bundle executed it. Its noir-contracts precedent is weaker than claimed because noir-contracts has an exports map; future package bumps must continue treating the path as unstable.

**What the 40-agent review missed:** chiefly the cross-file interactions in findings 1, 3, 5, 7, and 9: button gating versus E2E initialization; availability checks versus lockstep skipping; updater writes versus workflow staging; root typecheck versus SDK CI; and shared session state versus comparative timing.

Current `a6668a0` passed root typecheck, 20 root script tests, and the playground unit suite (73 passed, 1 skipped). I could not rerun local-network E2E because the sandbox and Vite services were unavailable.

**Verdict: FIX-BEFORE-MERGE (#397)**