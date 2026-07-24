# Codex audit — Round 1 (mid, xhigh, gpt-5.6-sol, read-only)

Session: 019f5cda-c6d3-7b20-bd79-787824c6fd8e — 2026-07-13. Verdict: **reject** (7 blocking findings; all verified against the repo and folded or explicitly rejected in plan.md's decision ledger).

## Critical findings

- **[High] The migration inventory is false.** There are **five**, not three, two-argument calls. The plan omits the live deploy and batch-funding scripts: [deploy-sponsored-fpc.ts:149](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/packages/playground/scripts/deploy-sponsored-fpc.ts:149) and [batch-fund-fpc.ts:250](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/packages/playground/scripts/batch-fund-fpc.ts:250). This directly contradicts [plan.md:42](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/implementations-plan/aztec-5.0.0-stable-2026-07-13/plan.md:42) and can break P3a.

- **[High] SQLite-OPFS impact is materially understated.** The playground does not set `ephemeral: true`; it creates persistent stores at [aztec.ts:224](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/packages/playground/src/aztec.ts:224). Its recovery loop still deletes only IndexedDB [aztec.ts:129](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/packages/playground/src/aztec.ts:129), [aztec.ts:267](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/packages/playground/src/aztec.ts:267). Therefore “nothing persisted,” “orphaning harmless,” and an optional error message are unsafe. Choose explicitly: make the playground ephemeral, or implement OPFS lifecycle/recovery and migration UX.

- **[High] `latest` moves before acceptance and the workflow is non-atomic.** Publish/tag and app deployment run concurrently after E2E [publish-testnet.yml:44](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/.github/workflows/publish-testnet.yml:44), [publish-testnet.yml:56](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/.github/workflows/publish-testnet.yml:56); smoke occurs afterward. Worse, `cancel-in-progress: true` can interrupt an irreversible publish [publish-testnet.yml:12](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/.github/workflows/publish-testnet.yml:12). A failure after npm publish causes a rerun to select `5.0.0-revision.1` [get-sdk-publish-version.ts:31](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/scripts/get-sdk-publish-version.ts:31).

- **[High] Promotion lacks an enforceable security gate.** `_publish-sdk.yml` is directly dispatchable with `latest: true`, bypassing `publish-testnet` E2E [\_publish-sdk.yml:3](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/.github/workflows/_publish-sdk.yml:3). There is no protected environment, stable-semver guard, or two-person approval. Mutable action tags and `bun-version: latest` execute before the static npm token is exposed [\_publish-sdk.yml:40](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/.github/workflows/_publish-sdk.yml:40). The caller also grants `contents: write` and OIDC globally [publish-testnet.yml:16](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/.github/workflows/publish-testnet.yml:16).

- **[High] P3a fails open around a signing key.** The deployment script merely logs node version and catches every `getContract` error as “not deployed” [deploy-sponsored-fpc.ts:86](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/packages/playground/scripts/deploy-sponsored-fpc.ts:86), [deploy-sponsored-fpc.ts:197](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/packages/playground/scripts/deploy-sponsored-fpc.ts:197). Require exact node version, Sepolia chain ID, expected rollup/portal addresses, and distinguish absence from RPC failure before signing. Inject the key through a secret manager—not a literal shell-history command. “Bridgeable funds” is also misstated: the script mints test FJ; the wallet principally needs Sepolia gas [deploy-sponsored-fpc.ts:153](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/packages/playground/scripts/deploy-sponsored-fpc.ts:153).

## Assumption attack

**Facts**

- **[High]** Three call sites is wrong; there are five.
- **[Medium]** Promotion does not technically require the proposed pass-through because the reusable workflow already has direct dispatch.
- **[Medium]** The bump tool can silently skip unpublished packages [update-aztec-version.ts:55](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/scripts/update-aztec-version.ts:55). Restore the rc.1 precedent’s “zero skips and every pin exactly 5.0.0” gate.
- **[Low]** npm timestamps, live headers/node version, artifact hashes, and unpublished status are observations, not reproducible repo Facts. Capture command output and digests in release evidence.

**Inferences**

- **[High]** P1 does not typecheck playground source. Root `test:typecheck` checks only SDK [package.json:27](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/package.json:27); playground only typechecks scripts [packages/playground/package.json:18](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/packages/playground/package.json:18). Add `tsc -p packages/playground/tsconfig.json`.
- **[High]** Deployed-1.0.6 compatibility remains unproved if its smoke is merely “ideal.” Make released-app native smoke mandatory before `latest`.
- **[Medium]** `--provenance` authenticates this SDK publication; it does not validate the hours-old upstream packages. Frozen lockfiles prove reproducibility, not legitimacy.
- **[Medium]** `getContract` proves deployment, not current FPC balance or resistance to public depletion. Define a minimum funding threshold and refill/monitoring response.

**Asks**

- **[High]** Persistent OPFS or explicitly ephemeral playground?
- **[High]** Who can promote/rollback `latest`, enforced by which protected environment and exact prior tag snapshot?
- **[High]** Which immutable merge SHA and expected L1/L2 addresses are authorized for release/deployment?
- **[Medium]** Is the min-age exception compensated by upstream provenance/integrity review, or should `latest` soak until July 20?

## Approach verdict

**[High]** B as written delays `testnet` unnecessarily and its “override user-visible in the artifact chain” rationale is false. A hybrid is superior: bump and deploy now; publish **only `testnet`**; smoke the live playground, released 1.0.6 path, and a clean install of the exact registry artifact; then promote that immutable version to `latest`, optionally after the seven-day soak. This preserves prompt testnet compatibility while containing first-`latest` blast radius.

The stable plan also wrongly drops the earlier explicit zero-skip gate and honest npm irreversibility warning; moving a tag does not repair existing installs or immutable bad versions.

## Verdict

reject (with blocking findings: migrate all five account calls; resolve OPFS persistence/recovery; add playground source typechecking; split publish from protected stable-only promotion; disable cancellation/handle partial publishes idempotently; require released-1.0.6 and packed-registry smokes before latest; and make FPC deployment fail closed on chain, RPC, address, key, and funding checks)
---

# Codex final fresh-context pass (mid, xhigh, gpt-5.6-sol, read-only)

Session: 019f5ceb-34bc-7a31-9f97-3f992900974f — 2026-07-13. Saw the consolidated plan + both audit transcripts + decision ledger. Round-1 verdict: **reject** (4 findings — all verified and folded; see plan.md ledger). Resumed with the deltas for a re-verdict (appended below when received).

Not approval-ready. The fold claims all blockers were adopted, but several remain.

- **[HIGH — consolidation/security] Promotion remains bypassable.** `_publish-sdk.yml` is still directly dispatchable with arbitrary `dist_tag` and `latest: true` ([\_publish-sdk.yml](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/.github/workflows/_publish-sdk.yml:3)), and can move `latest` itself ([line 103](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/.github/workflows/_publish-sdk.yml:103)). Thus the new guarded workflow and Ask 1’s “surface-and-hold” are not enforceable. This exact prior finding was silently dispositioned as adopted ([plan.md](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/implementations-plan/aztec-5.0.0-stable-2026-07-13/plan.md:119)). Remove direct dispatch and the reusable workflow’s `latest` capability; put the tag-only workflow behind a protected environment. Give all npm-mutating workflows one fixed, non-cancelling concurrency group. The first-ever `latest` promotion is not merely “repo-wide hardening.”

- **[HIGH — implementability/release integrity] The registry artifact is never tested.** The earlier audit explicitly required a clean install of the exact registry artifact ([audit-codex.md](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/implementations-plan/aztec-5.0.0-stable-2026-07-13/audit-codex.md:42)); P3d only checks tags and the workspace-built playground ([plan.md](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/implementations-plan/aztec-5.0.0-stable-2026-07-13/plan.md:67)). That misses publish-only package mutation ([\_publish-sdk.yml](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/.github/workflows/_publish-sdk.yml:84)). Before `latest`, require a fresh temporary project to install the exact `testnet` version and successfully import its public API.

- **[HIGH — key/funding safety] P3’s claimed funding proof does not exist.** The plan says deployment prints the FPC balance ([plan.md](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/implementations-plan/aztec-5.0.0-stable-2026-07-13/plan.md:64)); the script prints only salt/address ([deploy-sponsored-fpc.ts](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/packages/playground/scripts/deploy-sponsored-fpc.ts:235)). The suggested top-up defaults to 1,000,000 FJ and bridges the wallet’s entire L1 balance ([batch-fund-fpc.ts](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/packages/playground/scripts/batch-fund-fpc.ts:47), [line 199](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/packages/playground/scripts/batch-fund-fpc.ts:199)). Add an executable fail-closed preflight/postflight, actual balance query, mandatory explicit funding amount, and owner-approved minimum/maximum exposure. Inline key assignment also contradicts “never persist” because it enters shell history; require masked prompt or secret-manager intake.

- **[MEDIUM — contradiction]** Promotion accepts only stable semver ([plan.md](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/implementations-plan/aztec-5.0.0-stable-2026-07-13/plan.md:54)), while the stated recovery promotes `5.0.0-revision.N` ([line 68](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/implementations-plan/aztec-5.0.0-stable-2026-07-13/plan.md:68)). Define and test an allowlist for both the initial stable and audited revision format; pass dispatch input through an environment variable to avoid expression-to-shell injection.

Ask 3 remains honestly unresolved. Ask 1 is surfaced but unenforced; the public-FPC exposure ceiling is a missing Ask.

reject (with blocking findings: close the unguarded latest bypass, test the exact registry artifact, and make FPC preflight, balance, funding ceiling, and key handling executable)
## Re-verdict after fold (resumed session, response-1)

Most revisions now hold, and I accept the funding pushback: testnet FJ has no meaningful economic value beyond Sepolia gas.

- **[HIGH] One promotion bypass remains.** Direct dispatch still accepts arbitrary `dist_tag` ([\_publish-sdk.yml:6](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/.github/workflows/_publish-sdk.yml:6)), which feeds `npm publish --tag` ([line 101](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/.github/workflows/_publish-sdk.yml:101)). A dispatcher can simply specify `latest`; the value is also interpolated directly into shell. Removing only the `latest` boolean therefore does not close the bypass. Make the dispatch input a strict choice/allowlist excluding `latest`, pass it through `env:`, and reject unexpected values before publishing.

- **[LOW]** `.env` intake intentionally persists the key, contradicting the seed’s “never persist” rule ([plan.md:148](/Users/alejoamiras/Projects/aztec-accelerator/.claude/worktrees/aztec-5.0.0-stable-2026-07-13/implementations-plan/aztec-5.0.0-stable-2026-07-13/plan.md:148)). Reword that rule to “never log, commit, or enter in shell history,” and require deletion after P3.

conditional approve (with conditions: prevent direct dispatch from setting or injecting `dist_tag=latest` via a strict non-latest allowlist and env indirection)
*Post-audit note (main agent): the residual dist_tag=latest bypass condition + the .env LOW are folded into plan.md P2/P3a; codex accepted the testnet-FJ exposure pushback. Final verdict on record: conditional approve, all conditions folded.*

---

# Codex post-implementation audit (mid, xhigh, read-only) — `conditional approve`, all conditions executed

Session: 019f5d81-5c90-7691-a4d5-74c5b8d98cf2 — 2026-07-13, post-ship. Audited the implementation layer + the 40-agent code-review cleanup commit as distinct artifacts, plus the two skipped-finding rationales (both judged sound — reachability 'stronger than stated').

The cleanup commit is technically sound except for two release-control gaps.

- **[HIGH — implementation layer] Nightlies can still cancel an irreversible publish.** `publish-nightlies.yml:6-8` has `cancel-in-progress: true` and calls `_publish-sdk.yml` at `:38-45`. A new nightly dispatch can therefore cancel the entire caller—including an active reusable publish—despite `_publish-sdk.yml:28-34` claiming mutations are never canceled. This can strand npm state between publish and tag/release. [GitHub confirms running workflows are canceled under this setting.](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency) Hotfix before another nightly: use `cancel-in-progress: false`; preferably enable the supported multi-run queue.

- **[MEDIUM — cleanup verification] The new TypeScript graphs are not enforced by App CI.** `packages/playground/package.json:18` runs all three graphs, but `.github/workflows/app.yml:59-63` runs only Biome and `typecheck:scripts`. Add `bun run --cwd packages/playground typecheck`. I independently parsed/typechecked commit `14f1f2e`: zero diagnostics and every tracked playground TS file is included—so there is no current silent exclusion, only a future regression gap.

Cleanup checks otherwise pass:

- Removing `workflow_call.latest` breaks no caller; testnet and nightlies pass only `dist_tag`.
- `inputs.skip_sdk_publish != true` is correct because dispatch Boolean inputs remain Booleans. [GitHub inputs reference.](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#inputs-context)
- S3 ordering is correct: sync → unconditional asset re-upload with `no-cache` → invalidation (`publish-testnet.yml:98-115`). After merge, run a playground-only deployment; merging alone does not update live S3 metadata.
- KV-resolution, Windows path resolution, and `Object.hasOwn` introduce no regression.

Both skipped rationales are sound:

- Reachability: stronger than stated—`initializeNode()` immediately calls the exact same `getNodeInfo()` method (`packages/playground/src/aztec.ts:172-178`), so a fallback probe would not make initialization viable.
- CRS derivation: safe as follow-up because the bump tool already rewrites the literal (`scripts/update-aztec-version.ts:70-76,134-136`). Derivation remains worthwhile maintenance work, not a 5.0.0 hotfix.

**Verdict:** conditional approve (with conditions: disable cancellation in `publish-nightlies.yml`, wire the full playground typecheck into App CI, and perform a playground-only redeploy after merge to activate the cache headers)