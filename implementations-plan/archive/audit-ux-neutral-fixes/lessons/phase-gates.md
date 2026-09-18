# Validation evidence, gate by gate

Every phase ran the full local gate before its commit. Recorded here because "I ran it" is not
evidence — and because the counts show what each phase actually added.

| Gate | Rust (core) | Rust (src-tauri) | TS | clippy | Windows cross-check | lint |
|---|---|---|---|---|---|---|
| Phase 1 — F-13, F-12 | 218 | 110 → 111 | 268 | clean | clean | clean |
| Phase 2 — F-11, F-03c | 220 | 111 | 268 → 274 | clean | clean | clean |
| Phase 3 — F-08a | 224 | 111 | 274 | clean | clean | clean |
| Phase 4 — F-03 A+B | 230 | 111 | 274 | clean | clean | clean |
| Review rounds 1–8 | 231 | 114 | 275 | clean | clean | clean |

`bun run lint:actions` ran on every gate that touched a workflow (the NSIS resolver and the aztec
snappy repair); it is not re-run on gates that touch no YAML, because it would prove nothing.

## What the gates caught that local reasoning had not

- **Clippy, phase 4.** `classify` was dead code on Linux — the non-Windows branch returned early and
  never called it, so the one piece of pure decision logic was unreachable on the platform the whole
  suite runs on. Fixed by removing the `cfg` branch from the decision path rather than silencing it.
- **Clippy, round 5.** The same shape again after the `Lookup` enum gained Windows-only variants.
- **The Windows cross-check is weaker than it looks.** It proves the code COMPILES for Windows, not
  that its tests pass there. A test asserting `Lookup::Image` survived the multi-owner refactor as a
  compile-clean, always-panicking test; only reading it caught that. Recorded because I had been
  citing the cross-check as if it were stronger.

## Mutation proofs

Each fix was reverted, the named test watched to fail, and the fix restored. Nine proofs:

| Fix | Test that failed under mutation |
|---|---|
| F-12 fuse rule | `appimage_rejects_appdir_that_is_not_a_fuse_mount` |
| F-12 parentage rule | `stacked_mounts_resolve_to_the_topmost_regardless_of_record_order` |
| F-12 mount-id override | `the_resolved_mount_id_overrides_same_path_inference` |
| F-11 bounded read | 3 of 6 `readProveBody` tests (two hung, one threw) |
| F-11 retry `await` | `an unreadable body on the HTTP retry degrades to WASM…` |
| F-03 sink C cap | `oversized_health_body_is_rejected_rather_than_buffered` |
| F-03 sink B conjunct | `floor_commits_only_when_we_own_the_bind_and_our_own_version_answered` |
| F-03 sink A polarity | `bow_out_polarity_only_adds_reasons_to_stay` |
| F-08a age floor + symlink guard | `reaps_abandoned_workspaces_but_spares_a_live_one`, `reap_does_not_follow_symlinks_out_of_the_parent` |

F-13's proof is Windows-only and was confirmed on CI instead: the `windows-build` job log shows
`poisoned_system_root_cannot_redirect_schtasks ... ok` and `..._certutil ... ok` by name, so the tests
demonstrably execute rather than merely compile.
