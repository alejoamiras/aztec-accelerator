# Consolidated findings — 2026-06-10-max-q7e3 (deduped by root cause)

15 findings across 6 clusters (9 raw inputs: 6 Fable + 3 Codex). Dedup collapsed ~24 raw → 15. Convergence = primary confidence signal.

| ID | Smell | Impact | Found by | Priority |
|----|-------|--------|----------|----------|
| F-01 | Temporal Coupling + Dup — Safari-HTTPS bring-up | structural/HOT | both ×3 | ★ top (proven failure: M1) |
| F-02 | Misplaced-Shared-Types + Cyclic Dep (SDK types) | structural/HOT | both | high |
| F-03 | Stringly error channel `(StatusCode,String)`+json_error×11 | structural/HOT | both | high |
| F-04 | Long Method — `.setup()` 154 lines | structural/HOT | both | high |
| F-05 | Long Method + missing Factory — `#probeAndParseHealth` | structural/HOT | both | high |
| F-06 | Temporal Coupling — SDK protocol pinning | structural/HOT | both | high |
| F-07 | Divergent Change — `versions/mod.rs` hub | architectural | both | med-high |
| F-08 | Primitive Obsession — AztecVersion + CanonicalOrigin | structural | both | med-high |
| F-09 | Temporal Coupling — AuthorizationManager dual-map | structural | both | med |
| F-10 | Temporal Coupling/Intimacy — updater↔crash_recovery | structural | both | med |
| F-11 | Divergent Change — `createChonkProof` | structural | both | med |
| F-12 | Misplaced tests stranded in server.rs (~715 LOC) | structural | claude (codex partial) | med |
| F-13 | Duplicate Code — lock-mutate-save ×3 diverged policy | local | claude+map (codex disagreed→resolved) | low-med |
| F-14 | Duplicate Code — loopback-literal sets ×3 | local | map | low |
| F-15 | Global Data — config.rs hardcoded path | local | claude | low |

Refuted (8): server.rs Large Class, versions Large Class, crash_recovery parallel-impl, tls.rs, tier Switch, bb ladder, mutate_config Middle Man, config Data Clump (as stated).
