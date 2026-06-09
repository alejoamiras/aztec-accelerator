# Phase 4 — Verifier pass (independent Opus re-read against source)

Method: for each consolidated finding I re-read the cited source before trusting the claim, and corrected any instance/LOC inflation. **9/9 confirmed as real smells; 0 refuted.** Two framing corrections (F-01 instances, F-04 LOC). Confidence + corrections below.

| ID | Verdict | Confidence | Correction |
|----|---------|-----------|------------|
| F-01 | CONFIRMED | high | 5 of the `server.rs` instances (641–1035) are TEST helpers (tests start L210), not prod. Real prod sites: `server/src/main.rs:62-75`, `src-tauri/src/main.rs:345-367`, struct def `server.rs:83-119`. Test-helper proliferation is *corroborating* evidence (a new field edits tests too), not separate prod debt. |
| F-02 | CONFIRMED | high | Verified live: `server/src/main.rs:50-51` writes raw trimmed `ALLOWED_ORIGINS` env strings into `approved_origins` with no `url::Url` canonicalization → real invariant leak. Strongest finding. |
| F-03 | CONFIRMED | high | `.setup` closure `main.rs:260-462` (203 lines) confirmed by map + finder + both rebuttals. |
| F-04 | CONFIRMED | med-high | **LOC corrected: ~582 prod LOC, not 1209** (inline tests start L582). Large Class still valid: 18 prod fns across 8+ responsibilities. Severity recalibrated high→med-high (well-tested, cohesive domain, moderate prod size). |
| F-05 | CONFIRMED | high | `README:88-101` documents the obsolete flat `interface AcceleratorStatus` (vs the shipped union `accelerator-prover.ts:56-92`); barrel `index.ts:1-8` omits `AcceleratorProtocol` (used at L46, MIGRATION says exported). The "skill omits `denied`" sub-claim is about `AcceleratorPhase` (a separate type) — lower-confidence minor sub-item, not load-bearing. |
| F-06 | CONFIRMED | med | Two HTTP stacks confirmed (native `fetch`+`Promise.any` for `/health`; `ky` for `/prove`); both C6 finders converged. |
| F-07 | CONFIRMED | med | Cert-path triplet Data Clump; both C5 finders independently. Folds the (over-asserted) `rotate()` temporal-coupling item. |
| F-08 | CONFIRMED | med | Verified in source: `prove()` sets `Proving` (prove.rs:160), `resolve_version()` clobbers to `Downloading` (66-68) then restores `Proving` (94-96). Hidden cross-fn status contract. `server.rs:617-685` instance is correctly labeled characterization (test). |
| F-09 | CONFIRMED | low | Both sites confirmed: `main.rs:72+86` and `commands.rs:156+161` each `load_rustls_config()` then `spawn(start_https(...))`. Small but real dup. |

## NOT-pursued — spot-checked, agree with the drops
- `compute_threads` Feature Envy: read prove.rs:104-113 — it's a single-use config→`Option<usize>` adapter calling existing `Speed::is_full/to_threads`; not duplicated domain logic. **Correctly dropped.**
- `CrashRecovery` trait Speculative Generality: ground-truth grep confirmed 1 impl, 0 polymorphic uses, no mock. Real but minor/local. **Correctly demoted to a one-line note (not a standalone finding).**
- SDK `catch ⇒ offline`: Codex verified non-OK + bad-JSON already split to `"error"` (accelerator-prover.ts:291/305); the `:370` catch is the legitimate "both probes failed" bucket. **Correctly dropped (false alarm).**
- xattr/codesign + security-CLI + popup-label + doc-marker dups, boxed-error downcast: real but below final-report bar. **Correctly dropped/folded.**

## Verifier's net read
Findings are well-grounded and cross-model-converged. The codebase is healthy post-refactor; remaining debt is concentrated in a handful of shared seams (state bag, origin type, startup closure, SDK contract duplication), not broad rot. No Critical/Blocker-equivalent quality debt. The cheapest high-value wins are **F-05** (hours — fix doc drift + add a sync test) and **F-07** (hours — `CertPaths` parameter object).
