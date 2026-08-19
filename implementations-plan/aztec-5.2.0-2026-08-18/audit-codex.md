# Codex audit — aztec-5.2.0 bump plan (blueprint light)

Model: gpt-5.6-sol @ xhigh, read-only sandbox. Session `01a01554-829e-7900-bfc2-ffa26d2c99a6`.

## Round 1 (2026-08-18) — plan rev 1

**Verdict**: `conditional approve (with conditions: resolve the separate CI npm min-age gate;
replace the FPC check with a read-only preflight; correct F1/I1/I4 and the package counts; make
the full SDK native-path command explicit; resolve A1 against the token-flow gate)`

### Findings and disposition (all verified against the repo before adoption)

| # | Finding | Verified? | Disposition |
|---|---|---|---|
| 1 | **High/blocking — F6 false**: `setup-aztec/action.yml` installs the Aztec CLI unlocked with `npm_config_min_release_age=7`; same-day 5.2.0 blocks the sandbox e2e legs regardless of bunfig | TRUE — empirically reproduced: npm 11.16 `ETARGET … before 8/11/2026` on exact-pinned 5.2.0. npm 12.0.2 ships `min-release-age-exclude` (globs; env-var form works — tested, resolves 281 pkgs). npm 11.16 lacks the key (npmrc/flag/env all tested) | ADOPTED → new Phase 2 (npm@12.0.2 + `@aztec/*` exclude + cache-salt bump + local installer validation); F6 corrected; Ask A4 added (alternative: wait to ~Aug 25) |
| 2 | **FPC preflight impossible as written**: `deploy-sponsored-fpc.ts` exits without L1 creds before deriving; always bootstraps+funds after | TRUE — verified lines ~45-58 | ADOPTED → Phase 4.2 replaced with a secret-free read-only `bun -e` derivation + `node.getContract` check; F8 amended |
| 3 | F1 overbroad (playground carries `viem: npm:@aztec/viem@2.38.2`, inside the waived scope) | TRUE | ADOPTED → F1 reworded; waiver scope note added to Phase 1.1 + Security |
| 4 | Package counts wrong: 13 distinct manifest keys (not 14); lock has ~31 @aztec-scoped names; min-age filters transitives, so an exact-name fallback from manifests is insufficient | TRUE (recounted: 11 sdk + 2 playground-only = 13) | ADOPTED → I1 fallback = generate the list from the post-updater bun.lock |
| 5 | I4 double-duty (Phase 2 cited I4 for asset↔bb.js mapping; I4 is defined as TS drift) | TRUE | ADOPTED → new I6 (asset mapping), I7 (installer env under npm 12) |
| 6 | A1 mis-arbitrated: the Playwright `smoke` project is deploy-only, no token flow | TRUE (spec grep) | ADOPTED → F10 added; A1 arbiters = CI local-network token spec + new Phase 4.6 manual testnet token flow |
| 7 | SDK e2e command imprecise: `test:e2e:remote` ≠ the native-path proof; phase-trail asserts live in `proving.test.ts` | TRUE (package.json scripts) | ADOPTED → Phase 4.5 exact `bun run --cwd packages/sdk test:e2e` with both env vars |
| 8 | Windows pin note is a change detector, not provenance; owner must independently verify | Agreed (framing) | ADOPTED → wording fixed; A3 now requires the owner's independently computed digest as a PR comment pre-merge; pin note records GitHub asset id + API digest too |
| 9 | Lockfile criterion "or its direct consequence" too elastic | Agreed | ADOPTED → every non-@aztec change individually explained from a changed dependency edge |
| 10 | Merge Phases 1+2 (double full-suite run adds nothing) | Agreed | ADOPTED → merged into Phase 1, single gate |
| 11 | Min-age waiver removes the only observation window (High, inherent) | Agreed — inherent to the owner's decision | ACCEPTED AS RESIDUAL, documented in Security (not a plan change; owner decision 2026-08-18) |

Rejected: nothing material.

### Notable addition from verification (not in codex's list)
npm 12 blocks some packages' install scripts by default (`allowScripts` gating — observed on
`msgpackr-extract`/`leveldown` in dry-run) — could break the installer's native bindings the same
way snappy did. Added Phase 2.3: validate the installer end-to-end locally under npm 12 before
pushing; the action's snappy probe is the CI backstop.

## Round 2 (2026-08-18) — rev 2

**Verdict**: `conditional approve (with conditions: make the npm 12 allowScripts branch fail
closed; make the Bun exact-name fallback target-graph-aware or verify and rely on the Bun 1.3.14
wildcard; add the required account deploy and underfunded-FPC contingency to the live token-flow
gate)`

| # | Finding | Verified? | Disposition |
|---|---|---|---|
| 1 | "Resolve the allow-scripts knob if needed" gives discretion to broadly enable install scripts | Agreed (wording) | ADOPTED → Phase 2.3 fail-closed: stop + report blocked packages; exceptions only as minimal per-package allowlist via plan revision, revalidated |
| 2 | Bun fallback seeded from the OLD lock may miss new 5.2.0 transitives; wildcard still unverified | RESOLVED EMPIRICALLY (Bun 1.3.14 = latest stable = CI pin): exact names WORK (excluding `@aztec/foundation` moved the block to transitive `@aztec/bb.js`), wildcard silently IGNORED, min-age filters transitives | ADOPTED → Phase 1.1/1.3: iterative fail-closed exact-name list (seed 31 lock names, add each named blocked @aztec package, non-@aztec block = stop, prune to final graph); F11 added, I1 rewritten |
| 3 | Manual token flow omits required Deploy Test Account first; preflight proves deployment, not Fee Juice balance | TRUE (UI gating) | ADOPTED → Phase 4.6 prerequisite added; A2 extended to underfunded-FPC (`--fund-only`); fee-failure vs compat-failure disambiguation noted |
| 4 | Non-blocking: Phase 1 gate redundant (root `bun run test` includes playground typecheck:scripts + accelerator test:unit) | TRUE (root package.json verified) | ADOPTED → gate = `bun run test` + pin-check |

## Round 3 (2026-08-18) — rev 3

**Verdict**: `approve` — "rev 3 is implementation-ready. The three prior blockers are fully
addressed... No new blocking issue appeared. A1–A4 are appropriately surfaced
approval/contingency decisions rather than hidden implementation assumptions."

Three optional editorial cleanups, all APPLIED: header rev reference fixed; Security waiver-scope
wording tightened to "the final resolved @aztec/* graph, not arbitrary future package names";
pin verification command changed to download-to-file (`curl -fsSL -o … && sha256sum …`) with
required equality against the GitHub API digest.

---

# POST-IMPLEMENTATION audit (new session `01a01a71-b616-7d42-990f-968783af923e`, 2026-08-19)

Ran after `/code-review max --fix` (commit `ae8dd4c`, 7 of 9 findings applied, 2 report-only).

## Round 1 — verdict: conditional approve
| Sev | Finding | Disposition |
|---|---|---|
| Med (condition) | `sdk.yml` changes filter omitted `bunfig.toml` → a bunfig-only exemption edit could skip the parity guard | ADOPTED (`d9ebd28`): filter entry + comment |
| Low | $HOME/.npmrc allowScripts approvals persistent + name-only (self-hosted-runner hazard; future versions auto-approved) | ADOPTED (`d9ebd28`): step-scoped mktemp userconfig via NPM_CONFIG_USERCONFIG + version-pinned six identities; revalidated locally (bcrypto builds+loads, zero blocked) |
| Low | Tarball pin guard checked presence, not exactness | ADOPTED (`d9ebd28` shape guard → `96348a4` canonical semver.org regex; verified accepts 5.2.0, rejects `^5.2.0` / `5.2.0-alpha..x` / `01.2.3` / npm-alias) |
| Low | lessons/phase-3 cache-save rationale wrong (post-save skips on failed jobs) | ADOPTED (`d9ebd28`): rationale corrected, salt bump stands on the recipe-change rule |

## Round 2 (resume, fix diff `ae8dd4c..d9ebd28`) — verdict: conditional approve
Two smalls: canonical semver validation; EXIT trap for the temp userconfig. Both ADOPTED (`96348a4`).

## Round 3 (resume, diff `d9ebd28..96348a4`) — verdict: **approve**
"No remaining material findings… no new trust widening or correctness regression found."
**Fix loop CONVERGED in 2 rounds** (< the 3-round scope-smell threshold).
