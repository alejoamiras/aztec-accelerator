# Phase 1 — PR-1 (SDK maintainability: F-02, F-06, F-05, F-11)

Branch `quality/pr1-sdk-q7e3`. All behavior-preserving; the dense existing unit suite (41→45 tests) is the guard.

## F-02 — move public types to `lib/types.ts`
- The 6 published types lived in the 440-LOC `accelerator-prover.ts` hotspot; `accelerator-transport.ts` back-imported two of them (the latent cycle).
- Moved verbatim to a new `lib/types.ts`; `index.ts` re-exports from there (barrel byte-identical — `public-contract.test.ts` green). `accelerator-prover.ts` imports the 5 it uses internally; transport + its test import from `types.ts`.
- **Lesson:** dropped the convenience re-export from the prover — `types.ts` is the *sole* home, which is the point. The contract test imports from the barrel (`../index.js`), not the source module, so the move is invisible to consumers.
- **Gotcha:** running `bun test src/` from the repo root skips the `bunfig.toml` preload (`test-setup.ts`, the `@aztec/foundation` equality-tester patch) → 28 spurious fails. Always `bun run --cwd packages/sdk test:unit`.

## F-06 — own the 3-state protocol pin in transport (characterization-FIRST)
- The pin rule was scattered across 3 probe exits: `set` on ok, `clear` on malformed JSON, **`keep`** on `!response.ok` (no `setProtocol` call). The audit flagged that "derive pin from the discriminant" would flatten keep→clear and silently change which endpoint `/prove` hits.
- The keep-vs-clear distinction has **no observable `/prove` effect at the prover level** (an error/malformed probe caches an *unavailable* status → `/prove` never fires off it). So the characterization lives at the **transport unit**: `commitStatus(status, transition)` with explicit `set`/`clear`/`keep`, tests written first (`"keep"` asserts an existing pin survives `!ok`; `"clear"` asserts it's dropped).
- **Lesson:** behavior-preserving because the success-path `setProtocol` moved from before the JSON parse to the `commitStatus` return, and nothing between reads `baseUrl`.

## F-05 — extract pure version-policy
- Pulled the post-parse classification (multi-version / legacy-mismatch / available) into a pure `#classifyHealth(data, protocol, sdkVer)` — no I/O, no cache, no pin. Orchestrator shrank to probe → error-guards → classify → commit.
- **Lesson:** flattened the nested legacy `if (known) { if (mismatch) }` to one `&&` condition — same branch outcomes; the existing multi-version/mismatch/needsDownload tests are the guard.

## F-11 — split createChonkProof
- Extracted `#proveRemote` (serialize → POST `/prove` → 403→denied→WASM) and `#decodeProof` (proved/duration → receive → decode), reusing the existing Q5 `#fallbackToWasm`/`#proveLocally` (did NOT shadow them).
- **Lesson:** the phase-order characterization test (`detect→…→proved→receive`) is the guard — it stayed green, confirming the split didn't reorder or drop a phase.

## Validation
`tsc --noEmit` green; `bun run --cwd packages/sdk test:unit` 45/45; `public-contract.test.ts` unchanged. biome: 1 pre-existing warning (`firstCallMs` unused in an SDK test — not in scope).

## Pending (per-PR close)
`/code-review max --fix` → codex post-impl on the SDK diff → push → PR → CI → merge.
