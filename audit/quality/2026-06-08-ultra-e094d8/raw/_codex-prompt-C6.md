You are a QUALITY-audit finder (cross-model finder in a map-reduce /harden quality run). Maintainability only — NOT correctness/security. This cluster is WEIGHTED (SDK public API surface) — be thorough on the consumer contract.

Read your full method, the 8-field certificate format, and the negative list from:
  audit/quality/2026-06-08-ultra-e094d8/raw/_finder-instructions.md

Then audit cluster **C6 sdk-prover** — the public TypeScript SDK `@alejoamiras/aztec-accelerator`. Read in full:
  - packages/sdk/src/lib/accelerator-prover.ts
  - packages/sdk/src/index.ts
  - packages/sdk/src/lib/logger.ts
And compare DOCUMENTED vs ACTUAL exports for drift:
  - packages/sdk/README.md
  - packages/sdk/MIGRATION.md
  - packages/sdk/.claude/skills/aztec-accelerator/SKILL.md

Context: `AcceleratorProver extends BBLazyPrivateKernelProver` (@aztec), overrides proving to use the local native accelerator with WASM fallback; exposes an `AcceleratorStatus` discriminated union (narrowable on `available`); probes over BOTH native `fetch` (`/health`) and `ky` (`/prove`) with a status cache. Find (public-API lens): Primitive Obsession / Long Parameter List / Data Clumps in exported types + signatures; Long Method (`#probeAndParseHealth`, `createChonkProof`); Duplicate Code / Divergent Change (two HTTP stacks, divergent timeout/retry/error semantics); error-as-control-flow (bare catch → `offline`); Temporal Coupling (`#statusCache` ↔ `#acceleratorProtocol`); doc-contract drift (README/MIGRATION/SKILL vs the barrel exports). Be independent; named smells only, file:line + full certificate. One-line cluster verdict first.