Cluster verdict: 3 maintainability findings: 1 architectural hotspot in `versions.rs`, 1 cross-module temporal-coupling problem in version acquisition, and 1 medium hot-path long method in `bb::prove`.

## Finding 1 — `versions.rs` is a multi-responsibility hotspot
1. **Title** — `versions.rs` centralizes unrelated version, network, cache, install, and eviction logic.
2. **Smell** — **Large Class** (Rust-module equivalent): the same module owns several distinct reasons to change instead of one cohesive responsibility.
3. **Maintenance impact** — `architectural`; blast radius: 1 core module with 4 direct consumers (`bb.rs`, `server/prove.rs`, `server.rs`, `src-tauri/src/tray.rs`); change frequency: high because every versioning/cache/platform change lands here.
4. **Concrete evidence** — `packages/accelerator/core/src/versions.rs:6` HTTP client policy, `:18` tier/version domain rules, `:132` cache path + binary naming, `:160` platform mapping, `:186` release URL layout, `:213` eviction policy, `:255` cache directory scan, `:284` GitHub digest lookup, `:342` download/install orchestration, `:497` atomic install, `:527` tar extraction, `:559` cleanup.
5. **Why it harms future change** — changing release naming, retention policy, cache layout, installer behavior, or digest-fetch behavior all require reopening the same file and retesting unrelated paths, so the module becomes the default collision point for independent work.
6. **Smallest safe refactoring** — **Extract Class / Extract Module**: split into `version_id`, `artifact_layout`, `bb_cache`, and `bb_downloader` responsibilities first, without changing public behavior.
7. **What disappears** — the single-file edit hotspot and the need to mentally load version parsing, network I/O, filesystem install, and retention rules at once.
8. **Instances** — `packages/accelerator/core/src/versions.rs:6,18,132,160,186,213,255,284,342,497,527,559`.

## Finding 2 — Version acquisition is encoded as ordered cross-module steps
1. **Title** — Cache-miss handling depends on call order spread across the HTTP handler and cache module.
2. **Smell** — **Temporal Coupling** (analog): this maps to a change-preventer because the workflow only works when several functions are called in a precise order across modules.
3. **Maintenance impact** — `structural`; blast radius: 2 core modules plus cache-state consumers; change frequency: medium-high because any download/cleanup/status-flow change must preserve sequencing.
4. **Concrete evidence** — `packages/accelerator/core/src/server/prove.rs:64` cache-miss check, `:70` `download_bb` call, `:75` spawned cleanup/notification, `:94` status transition back to proving; `packages/accelerator/core/src/versions.rs:350` cached short-circuit, `:358` download, `:364` verify, `:369` install, `:371` chmod, `:383` macOS xattr/codesign tail, `:573` eviction loop.
5. **Why it harms future change** — adding one more lifecycle step such as manifest recording, retries, progress callbacks, or deferred eviction means editing both the request handler and `versions.rs` while preserving hidden ordering assumptions.
6. **Smallest safe refactoring** — **Extract Class** or **Move Function** into a single `ensure_version_available`/`VersionManager` workflow that owns download, post-install finalization, cleanup, and notifications.
7. **What disappears** — ordering knowledge leaked into `server/prove.rs`, plus the ad hoc `tokio::spawn` sequencing around cleanup and UI refresh.
8. **Instances** — `packages/accelerator/core/src/server/prove.rs:64,70,75,94`; `packages/accelerator/core/src/versions.rs:350,358,364,369,371,383,573`.

## Finding 3 — `bb::prove` is a long orchestration method
1. **Title** — `bb::prove` mixes staging, process setup, timeout handling, logging, and proof post-processing.
2. **Smell** — **Long Method**.
3. **Maintenance impact** — `local`; blast radius: 2 modules (`bb.rs` and its caller in `server/prove.rs`); change frequency: high because this is on the proof hot path.
4. **Concrete evidence** — `packages/accelerator/core/src/bb.rs:75` method entry, `:83` temp-file staging, `:95` command construction, `:109` thread-count env wiring, `:119` timeout handling, `:127` stderr truncation/logging, `:132` exit-status mapping, `:139` proof read + header re-encoding.
5. **Why it harms future change** — a change to CLI flags, temp-file layout, timeout policy, or proof output shape forces edits in one method that also owns error handling and logging, so small changes require revalidating too much surface area.
6. **Smallest safe refactoring** — **Extract Method** into `stage_inputs`, `build_prove_command`, `run_with_timeout`, and `load_proof`.
7. **What disappears** — the monolithic control flow and the need to trace five concerns to understand one edit.
8. **Instances** — `packages/accelerator/core/src/bb.rs:75,83,95,109,119,127,132,139`.