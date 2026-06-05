# Research — sdk + config/auth/frontend (accelerator-prover.ts, config.rs, authorization.rs, verified_sites.rs, frontend, copy-bb.ts) · Q5, Q12, Q13, Q14, Q9-config + minors

## Public surface (Q12 breaking-change boundary)
Exported (sdk/src/index.ts): `AcceleratorPhase` (9-string union: detect|serialize|transmit|proving|proved|receive|fallback|downloading|denied), `AcceleratorPhaseData{durationMs}`, `AcceleratorConfig{port?,httpsPort?,host?}`, `AcceleratorProverOptions`, `AcceleratorStatus{available,needsDownload,acceleratorVersion?,availableVersions?,sdkAztecVersion?,protocol?}`, class `AcceleratorProver`.
- **IN-REPO CONSUMERS (must bump in lockstep):** `packages/playground/src/aztec.ts` (imports AcceleratorPhase/PhaseData/Prover; setOnPhase/setForceLocal) + `packages/playground/src/ascii-animation.ts` (AnimationPhase = AcceleratorPhase | ... ; pattern-matches all 9 phases L277-302). Also grep the aztec-accelerator skill for imports.

## Invariants
- SDK↔server: reads `/health` {aztec_version, available_versions}; `/prove` 200 `{proof}`+`x-prove-duration-ms` header (else client-timed); error `{error,message}` (403 origin_denied→denied phase+WASM). **Q8 (server error shape) ↔ SDK parser coupled.**
- Phase ORDERING (test.ts pins): offline detect→fallback→proving; available detect→serialize→transmit→proving→proved→receive; download adds `downloading`; 403 detect→serialize→transmit→proving→denied→fallback→proving.
- `is_auto_approved` (authorization.rs:126-147) matches EXACTLY localhost / 127.0.0.1 / [::1] (any port) — pinned by tests; NOT 192.168.* nor *.localhost.com.
- `canonicalize_origin` (21-58): scheme+lowercase host+non-default-port; ext schemes; idempotent. Used config migration + server auth + verified lookup.

## Tests (SDK 29)
Proving ×9 (fallback, legacy mismatch, downloading phase, x-aztec-version header, **proved-emitted-even-without-duration-header**, 403-denied, multi-version no-fallback), checkAcceleratorStatus ×14 (multi/legacy, offline, non-ok, **protocol-not-cached-on-error**, HTTPS-fallback, dual-fail, TTL cache + expiry, 1s-retry, protocol persistence/reset, **setAcceleratorConfig invalidates cache**), Constructor ×6 (defaults, env ports, phase order). authorization.rs canonicalize/auto-approve tests pin the localhost set.
**GAPS:** exact phase SEQUENCE from #probeHealth/#parseHealthResponse internals; exact AcceleratorStatus field COMBINATIONS (discriminant validity) — add before Q12.

## Safe seams
- **Q5**: private `#probeAndParseHealth()->{status,protocol}` (L218-315, no phase emits) + `checkAcceleratorStatus` = cache+call+return; `PhaseReporter` iface wrapping the callback (createChonkProof calls reporter.detect() etc.). Public API unchanged.
- **Q12** (MAJOR bump v2): discriminated unions for AcceleratorStatus (`{available:true,needsDownload,...,protocol}` | `{available:false,protocol?,reason?}`) + phase events (`{phase:"proved",durationMs}` | `{phase:...}`). HTTP contract UNCHANGED (TS-only). **Migrate playground in lockstep.**
- **Q13** copy-bb.ts: `PLATFORM_MATRIX` table (darwin/linux/win32 × arch) replacing the if-ladder; extract `copyUnixBb` to match `fetchWindowsBb`.
- **Q14** is_auto_approved: reuse `canonicalize_origin` then match canonical host — **set MUST stay {localhost,127.0.0.1,[::1]}** (tests pin it; verify post-refactor).
- **Q9-config** server.rs:382 approved-origin persist also uses the lock-mutate-save (warn-swallow) pattern — fold into mutate_config too.
- minors: VerifiedSite 3 dead fields (collapse Entry/Site); default_config_version serde fn; frontend popup scaffolding dup + global-bridge coupling (Q codex sdk#7); SDK WASM-fallback dup (#fallbackToWasm); copy-bb twin size guards.

## Behavior-change risks
- **Q12 intentional break** (major bump) — but playground compile must move with it (search `@alejoamiras/aztec-accelerator` consumers repo-wide + skill).
- **Q8+Q12 coupling**: Q8 holds the error wire shape (safe); Q12 is TS-only. They don't force a coordinated *server↔npm* release IF Q8 keeps `{error,message}`. Confirm.
- Q14 could change auto-approve set if canonicalize parses hosts differently → tests are the guard.
