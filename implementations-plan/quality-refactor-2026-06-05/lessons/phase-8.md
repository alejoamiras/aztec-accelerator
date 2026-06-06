# Phase 8 — Q12 SDK type break (discriminated unions) — SCOPED, execution plan

Branch: `refactor/q12-sdk-break` (off main). Owner decision (Phase 0 #2 + Phase-8 memory): clean breaking
type change on the dev line + MIGRATION.md + publish-guard; NO heavy standalone-semver prerequisite
(Ask A resolved — SDK version stays derived from `@aztec/stdlib` at publish). Claude builds + cuts the SDK rc;
only the STABLE SDK publish is joint.

## Break targets (packages/sdk/src/lib/accelerator-prover.ts)
1. **`AcceleratorStatus`** (L46-59) — flat boolean-soup interface: `available`+`needsDownload`+4 optionals
   (`acceleratorVersion?`, `availableVersions?`, `sdkAztecVersion?`, `protocol?`). Illegal combos are
   representable (e.g. `available:false` with a version). → **discriminated union** mirroring the REAL
   reachable states. **NEXT TURN FIRST: read `checkAcceleratorStatus` (L202) + `#probeAndParseHealth`
   (L219-~300)** to enumerate the exact states the SDK produces, then model the union (likely:
   `{ kind:"unavailable" } | { kind:"available", acceleratorVersion, availableVersions, sdkAztecVersion, protocol }
   | { kind:"needs-download", ... }` — confirm against the Phase-0 sdk-contract-characterization fixtures
   so it mirrors today's combinations exactly; HTTP WIRE CONTRACT stays unchanged — this is TS-only).
2. **`AcceleratorPhase`** (L11-20, 9-string union) + **`AcceleratorPhaseData`** (L23-25, `{durationMs}`) +
   **`onPhase(phase, data?)`** (L42, L182 setOnPhase, L129 field). The `data` is only meaningful for
   `"proved"`. → fold into ONE **discriminated phase-event union**, e.g.
   `type AcceleratorPhaseEvent = { phase:"proved"; durationMs:number } | { phase: Exclude<...,"proved"> }`
   and change `onPhase(event)` so the payload can't be silently dropped/mismatched.

## Consumers to migrate IN THE SAME PR (free in-repo typecheck = the safety net)
- `packages/playground/src/aztec.ts` — ~5 `onPhase`/`setOnPhase` passthrough sites (L410,423,517,526,537,574,583,595,727).
- `packages/playground/src/ascii-animation.ts` — the phase→animation mapping (the real `onPhase` consumer).
- `AcceleratorStatus` consumers: grep `.available`/`.needsDownload` across playground (checkAcceleratorStatus callers).
- The `aztec-accelerator` skill doc (mentions the types) — update in the same PR (Ask A).

## Guardrails
- **publish-guard (Ask E):** the SDK auto-publishes on upstream Aztec bumps (`_aztec-update.yml` +
  `get-sdk-publish-version.ts`). A half-migrated break must NOT auto-ship → **freeze `_aztec-update.yml`
  for the Q12 window** (or land everything atomically in one PR). Confirm before merge.
- **MIGRATION.md** at packages/sdk root: before/after for both types + a codemod sketch.
- Split Q5·2 (PhaseReporter) is LOW-VALUE churn + a likely CUT (Q6 precedent) — assess/skip, don't gate Q12 on it.

## Execution order (next fresh-context turn)
1. Read checkAcceleratorStatus + #probeAndParseHealth → enumerate reachable states.
2. Define the two unions; keep a thin back-compat note in MIGRATION.md.
3. Update accelerator-prover.ts internals to construct the union states + emit the phase events.
4. Migrate playground (aztec.ts + ascii-animation.ts) + skill doc — `bun run --cwd packages/playground build` + monorepo typecheck is the net.
5. Add MIGRATION.md + the publish-guard.
6. `bun run test` + `bun run lint` + (SDK build) green. PR. Then Claude cuts the SDK **rc** (autonomous-allowed).
