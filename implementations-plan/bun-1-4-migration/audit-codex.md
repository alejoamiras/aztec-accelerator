# Codex audits — bun-1-4-migration (mid tier)

## Round 1 — plan rev 1 (gpt-5.6-sol @ xhigh, fresh session `codex-Ermg2pVd`)

**Verdict**: `conditional approve (with conditions: fix parallel/JEST semantics; split and fully
specify ky removal; add a clean 1.4 frozen-install gate; honor minimumReleaseAge; make NO-GO block
overall completion; correct supply-chain/linker claims; retain bounded archive safety)`

| Sev | Finding | Disposition (rev 2) |
|---|---|---|
| CRITICAL | ky removal under-scoped: `HTTPError` drives the F14 network-vs-HTTP taxonomy in accelerator-prover.ts (~:420+) and errors.ts consumes ky's `err.data`; a naive swap collapses witness retry/downgrade behavior | ADOPTED — split to standalone Arc D with the internal response-error contract, bounded error-body reads, redirect/timeout parity, F14 assertions frozen as the net |
| HIGH | "setup-bun verifies its own downloads" is FALSE (verified in pinned action source — no digest/signature check) | ADOPTED — claim removed; stated as accepted residual (F13); `.bun-version` = drift control only |
| HIGH | Phase 1 mislabeled "no behavior change" (logger destination + node_modules topology change) | ADOPTED — renamed "1.3-compatible wave", deltas named |
| HIGH | I2 contradicts bun semantics: `--parallel` implies `--isolate` (`--no-isolate` is the opt-out); `= "1"` would clobber real worker IDs | ADOPTED — `??=`, three-way spike (baseline / parallel / parallel+no-isolate), F15 |
| HIGH | No clean-install gate under the 1.4 package manager | ADOPTED — Phase 2.1 pristine `bun install --frozen-lockfile` + full suite |
| HIGH | `@types/bun@1.4.0` published 2026-08-21 — inside OUR 7-day min-age window; do not exempt | ADOPTED — trailing commit after ~08-28 eligibility; I6 corrected; F16 |
| HIGH | Seeds' NO-GO wording was a completion loophole (all-phases-✓ demanded even on NO-GO) | ADOPTED — dual-path /goal; Phases 3–5 never ✓ on NO-GO |
| MED | Bun.Archive validates after write, no decompressed-output cap — keep System32 tar | ADOPTED — copy-bb swap CUT (jointly with fable) |
| MED | Retries/parallel batched with the bump can mask a bump-caused regression | ADOPTED — Phase 3 is bump-only; tooling moved to Phase 4 |
| MED | F8 conflates download cache and virtual store; 7× claim needs mechanisms not being enabled | ADOPTED — F8 corrected; benefits to be measured, not promised |
| MED | Silent defaults: sunset criteria, packaged-e2e mandatory, who files the issue | ADOPTED — A2 resolved (packaged-e2e mandatory), A3 explicit (agent drafts/owner files), A4 added (sunset) |
| LOW | F7/I3 duplicate; F11 overbroad | ADOPTED — I3 promoted into F7; F11 reworded |

Also: chose the phased outline over the monolith; pushed the split FURTHER (bump-only isolated
from adoptions) — adopted as the Phase 3/4 boundary. Flagged the dropped ETag-parity recon check —
restored in Phase 4.4.

## Round 2 — fresh-context final pass on rev 2 (NEW session `codex-H5tnT7Tl`, per mid protocol)

**Verdict**: conditional approve ×6 consolidation gaps — all folded into rev 3:
NO-GO bound to the bb.js-leg execution (pino repro can never qualify); Arc D classification
invariant (unreadable bounded non-2xx bodies keep HTTP status — never demote to network failure,
which would trigger HTTPS demotion — + adversarial stalled/oversize/malformed tests both paths;
"tests construct HTTPError" corrected); serve-static needs its OWN contract test (packaged-e2e
runs the playground's Vite server, never serve-static — the A2 rationale was factually wrong);
JEST_WORKER_ID not cosmetic (process-global; undici/Playwright branch on it → containment scope +
consumer inventory + branch-semantics guard resolved isolated-linker-aware); isolated-linker exit
rule (measure, cut-on-failure, never strand the publish pin; Arc A commit order); ledger/assumption
hygiene (F9–F12 explicit, F15→BUN_TEST_WORKER_ID, A2→ledger, @types/bun manifest step, Arc D
branches from B: topology A → B → {C, D}).

## Round 3 (resume) — rev 3 consistency check

**Verdict**: conditional ×3 (all consistency): NO-GO provenance INTO the /goal seed text; Phase-1
gate reconciled with the exit rule (two valid outcomes, 4a–4f evidence); stale rev-2/serve-static/
phase-range/ledger/file-inventory wording purged. Applied.

## Round 4 (resume) — final

**Verdict**: **approve** — "Rev 3 is internally consistent. The terminal conditions,
retain-or-cut linker gate, Arc D classification invariant, serve-static coverage, JEST
containment, assumptions, and branch topology now align without material loopholes. No further
conditions."
