# Fable audit — bun-1-4-migration (mid tier), plan rev 1

Independent top-tier Claude auditor (Plan agent on Fable), full repo verification, same packet as
codex, run in parallel (separate context).

**Verdict**: `conditional approve (with conditions: (1) rescope Phase 4.1 to name
accelerator-prover.ts + errors.ts + their tests and freeze the F14 error-taxonomy semantics as the
behavior net; (2) rewrite Phase 1.4d to assert transitive @aztec/* resolution from inside the
swapped-in tarball dir under the isolated linker, and stop counting 4c as linker evidence; (3)
correct the --parallel/--isolate premise (adopt --no-isolate explicitly or canary per-file
fresh-global semantics); (4) specify local bun-version enforcement for both phase gates, add a
JEST_WORKER_ID guard test, and either name Windows coverage for the copy-bb Archive swap or cut
that item)`

| Sev | Finding | Disposition (rev 2) |
|---|---|---|
| HIGH | Linker validation aimed at wrong evidence: `packaged-e2e-swap-sdk.sh` documents its dependency on the ROOT HOIST the isolated linker eliminates; the planned assertion tested the materialized dir (never the risk); sdk-tarball-consumer.sh is npm-based non-evidence | ADOPTED — Phase 1.4c redesigned: post-swap resolution probe importing the SDK + transitive @aztec/*/ky FROM the swapped dir under the isolated tree |
| HIGH | ky blast radius: prover's `instanceof HTTPError` gates (:427,:501,:605) + errors.ts `err.data` = the F14 taxonomy; Modified-list named only the transport | ADOPTED — Arc D scope (converged with codex CRITICAL) |
| MED | `--parallel` implies `--isolate` (verified against the installed 1.4.0 binary) — "bare parallel, isolate stays out" was incoherent | ADOPTED (converged) — three-way spike |
| MED | Local toolchain enforcement silent Ask: machine is already 1.4.0; bun doesn't read .bun-version; "verify under 1.3.14" not executable as written; scratch-binary framing inverted | ADOPTED — header section documents scratch-pinned binaries per gate; lessons record `bun --version` |
| MED | JEST_WORKER_ID contract unguarded (unversioned upstream behavior) | ADOPTED — `scripts/aztec-logger-contract.test.ts` guard + A4 sunset |
| LOW-MED | Proportionality: cut the copy-bb Archive swap (works, Windows-only, v1.4.0-fresh API, ~6-line win) | ADOPTED — CUT (converged with codex MED) |
| LOW | F8 cache/store conflation (conservative direction); NO-GO must quote the exact crash signature; tsconfig vite-override note dropped | ADOPTED — F8 corrected; signature requirement in Phase 2.2; check restored as Phase 1.4e |

Verified beyond the plan's caution: F7 promoted to fact (`bun-version-file` present at the pinned
setup-bun SHA, workspace-root resolution, bare-line trim); F5/F6 exact; the "3 live CI jobs" lint
claim exact; ky confirmed in the published SDK's `dependencies`. Endorsed: phased over monolith
(publish pin can't wait on a spike; bb.js unknown would strand a monolith; ky deserves unfatigued
review); per-adoption revertable commits; F-007 KEEP for download-bb.
