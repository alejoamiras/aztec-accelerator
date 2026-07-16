# Fable audit (mid — Fable 5, Plan subagent, independent context) — 2026-07-16

Verdict: **conditional approve** (5 conditions — all folded; see plan.md ledger). Independently re-derived every load-bearing Fact (npm packuments/tarballs, decoded SLSA attestation, live testnet RPC, GitHub release + AztecProtocol/aztec-standards Noir source). Notable original findings: repo lineage (3-day-old AztecProtocol fork of defi-wonderland/aztec-standards; old package's registry metadata → alejoamiras/ecosystem-tooling), auth_contract=ZERO verified valid from source ("zero address to disable", hook guarded by !auth.eq(zero)), undeclared @aztec/aztec.js runtime imports behind the "zero dependencies" claim, the bump-tool blind spot for the new scope, and the A+contingency hybrid resolving the A-vs-B dispute.

---

## 1. ADVERSARIAL / SECURITY

**A1 [MEDIUM]** — Trust chain real but stated on wrong anchors. Decoded npm attestation: SLSA v1 provenance binding the publish to github.com/AztecProtocol/aztec-standards, workflow `.github/workflows/release.yml`, `refs/tags/v5.0.1`, commit `c74541f7cf2bb23b704e96fd326ea95d98252669` (matches packument gitHead). Repo created 2026-07-13 — a fork of defi-wonderland/aztec-standards (created 2024-12-30). Plan misstatements: "scope 1 day old" (it's HOURS old); "same repo the user's own package was built from" (old package's registry repo = alejoamiras/ecosystem-tooling, packages/aztec-standards); "audited" (no evidence). Mitigation holds on the verifiable anchors: canonical org + attested CI publish + fork lineage + tarball integrity pinned in lock. Condition: restate + record attestation binding in P1 lessons.

**A2 [MEDIUM]** — "Zero dependencies" true install-time, but dist/src/artifacts/Token.js imports @aztec/aztec.js/{abi,contracts,fields} — undeclared runtime peers resolved via hoisting. Works this cycle (both 5.0.1; package records config.aztecVersion 5.0.1); latent version-skew trap → see P-4.

**A3 [LOW, accept]** — Two-package min-age override: compensating controls real and exercised; standards adds zero transitives (confirmed). Keep the hard rule: any other lock delta = stop + `npm view time` everything.

**A4 [LOW]** — P4: `npm whoami` necessary not sufficient (granular/read-only tokens; 2FA OTP interactive). Verify step catches failure either way. Residual for the ledger: deprecated package remains publishable by the personal account.

**A5 [LOW, accept]** — Standards token = same risk class as current noir-contracts token. Verified from Noir source: auth_contract=zero disables authorization hooks (no external call path); transfer carries #[authorize_once("from","_nonce")], nonce-0 self-call standard. Artifact smaller than current (5.3 vs 7.3 MB). No new trust boundary; no-/harden defensible.

**A6 [MEDIUM]** — No gate executes the token flow on the NEW surface's production bundle: local-network token specs test.skip'd (demo.local-network.spec.ts:43,:70); mocked project asserts a disabled button; production smoke is load-time only; _e2e.yml runs SDK e2e only. Standards token executes exactly once before latest: P3b on the DEV server. Last cycle's sqlite3.wasm 404 proves dev/prod divergence is real here. Condition: one standards-token pass on the live deployed site (WASM) in P3d before promote (~90s).

Inherited pipeline verified on main: publish-testnet.yml, _publish-sdk.yml (choice-typed dist_tag + guard :45-52, env indirection), promote-latest.yml (allowlist + published-version check + tag-only).

## 2. ASSUMPTION ATTACK

**Facts — verified exactly** (independently re-derived): stdlib 5.0.1 time; release framing verbatim + 17 bb assets; live node 5.0.0/11155111/1821665230; standards packument (provenance, maintainer, versions, no deps/main/exports, deep path); all four Token.d.ts signatures verbatim + deployWithOpts; kv-store 5.0.1 layout + imports map byte-identical to what vite.config.ts:51-52,138-148 expects; FPC sha 648b856a→15cf50ce recomputed; call sites :589/:686/:708/:726/:744-745 (plan's line drift immaterial); 24 pins counted; zero aztec-standards matches; dist-tags latest=testnet=5.0.0, 5.0.1 free; app 1.0.6; all inherited hardening on main.

**Facts — misstated** (trust-chain cluster, per A1): scope age; "same repo"; "audited". None load-bearing once restated.

**Inferences:** deep-path import — STRENGTHENED (noir-contracts Token uses the byte-identical pattern through this exact vite7+tsc pipeline; Bundler resolution permits deep paths precisely because no exports map); auth_contract=ZERO — NOW VERIFIED from source, but the plan's designated sources would have FAILED (tarball README lacks semantics; deployments.json omits auth_contract from constructorArgs) → repoint P1+seed at the GitHub Noir source; the stated deploy(...) fallback is incoherent (same parameter) — drop; "P1 unit/mock layer confirms behaviorally" — half false (no unit/mock executes token methods; first behavioral execution is P3b) → fix wording; 5.0.1-client/5.0.0-node — sound, P3b proves; tsc-clean — plausible; npm auth — fine w/ A4.

**Asks:** none missing; pre-approvals cover the sensitive decisions; the rest are plan-structure fixes.

## 3. PLAN-STRUCTURE

**P-1 [MEDIUM]** — A vs B: A right ONLY with a decoupling contingency. Under A, a P3b standards-token failure holds the security patch hostage behind demo debugging. Hybrid = A + pre-authorized fallback: revert the (cleanly separable) swap commit → re-run account+FPC smoke → publish pure 5.0.1 → re-land swap via skip_sdk_publish deploy (lever verified at publish-testnet.yml:6-10,:53,:60). Costs nothing now; removes B's only real advantage. B as drafted unjustified; A unmodified = avoidable hostage scenario.

**P-2 [MEDIUM]** — P3d acceptance never runs the token flow on the deployed bundle; pre-publish smoke was dev-server. Add one live-production-bundle token pass (WASM) before (e).

**P-3 [MEDIUM]** — P1 auth_contract verification source wrong (README not shipped; deployments.json omits the field) → repoint at the Noir source; fold this audit's verification as evidence.

**P-4 [MEDIUM-LOW]** — Bump-tool blind spot: update-aztec-version.ts:27 rewrites only `@aztec/`-prefixed keys; zero-skip grep likewise. Standards must stay in lockstep (A2). Next `aztec:update 5.0.2` silently strands it. Extend the tool this cycle or ledger as named follow-up.

**P-5 [LOW]** — Changelog otherwise complete. Missing one line: #24403 HANDSHAKE_FORGERY_PROTECTION = cross-client-version tagging discontinuity; exposure negligible by construction (single ephemeral 5.0.1 PXE; no persisted client state; the reused on-chain 5.0.0 state is only the FPC — no private notes, being redeployed anyway). Also inert: #24479, #24645, #24629, #24636 — disposition explicitly.

**P-6 [LOW]** — Gate-wording honesty (plan.md:45, :104). **P-7 [LOW]** — P4 OTP nuance; message + all-versions semantics + reversibility correct.

Sequencing otherwise sound on the proven rails; one-queued-run caveat, merge-SHA, .env lifecycle, BRIDGE_AMOUNT all correctly inherited.

## 4. VERDICT

**conditional approve** (conditions: 1. decoupling contingency in P3 + seeds; 2. live-bundle token pass in P3d before promote; 3. fix P1 verification source + drop incoherent fallback; 4. restate trust chain on verifiable anchors + record attestation binding + note undeclared runtime imports; 5. close the bump-tool blind spot this cycle or ledger as named follow-up. Non-blocking: #24403 one-liner; gate wording; OTP nuance; old-package publishability residual.)

A-vs-B: **A beats B, but only as the condition-1 hybrid.** No finding rises to reject: every load-bearing Fact survived re-derivation; several strengthened.
