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

## Round 2 — fresh-context final pass on rev 2

(recorded when complete)
