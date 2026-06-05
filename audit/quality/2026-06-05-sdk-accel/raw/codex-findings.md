# Phase 2 raw — Codex (xhigh) cross-model passes

## Rust modules

**Ranked Findings**

1. **`AppState` is carrying two runtimes at once.**  
Smell: `Temporary Field`.  
Impact: architectural; 3 production files today and any new router feature; very high frequency because every server capability threads through this context.  
Evidence: optional fields in [server.rs:33](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:33), [server.rs:37](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:37), [server.rs:38](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:38), [server.rs:39](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:39), [server.rs:40](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:40), [server.rs:42](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:42); partial constructors in [main.rs:347](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:347), [main.rs:377](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:377), [server main.rs:43](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/server/src/main.rs:43), [server main.rs:62](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/server/src/main.rs:62); `None`/fallback branches in [server.rs:226](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:226), [server.rs:250](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:250), [server.rs:292](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:292), [server.rs:324](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:324), [server.rs:334](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:334), [server.rs:382](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:382), [server.rs:432](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:432), [server.rs:475](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:475), [server.rs:513](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:513).  
Why: adding a new server concern, like metrics, a second UI callback, or another runtime mode, forces edits in both constructors and in many `Option` branches.  
Refactor: `Extract Class` plus `Introduce Null Object`, or split `AppState` into invariant server state plus a desktop adapter.  
What disappears: `Option` checks, `unwrap_or` fallbacks, and headless-vs-desktop branching inside request code.

2. **`/prove` is still one distributed procedural script.**  
Smell: `Long Method` with helper extraction, but one change unit remains spread across functions.  
Impact: architectural; `server.rs` plus auth UI wiring; very high frequency because this is the app’s core request path.  
Evidence: auth flow in [server.rs:288](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:288), duplicated auth timeout in [server.rs:361](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:361) and [windows.rs:72](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/windows.rs:72), version resolution in [server.rs:409](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:409), main handler in [server.rs:487](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:487), popup creation in [windows.rs:41](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/windows.rs:41), response path in [commands.rs:100](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:100).  
Why: a change like “queue proves per origin”, “show auth countdown”, or “support cancellation/progress” cuts across HTTP parsing, auth, popup lifecycle, status updates, and proving.  
Refactor: `Extract Class`/workflow object such as `ProveWorkflow` or `AuthorizeAndProve`.  
What disappears: interleaving of auth, body buffering, semaphore handling, status changes, version download, and response serialization.

3. **The update flow is a temporal state machine spread across four modules.**  
Smell: `Temporal Coupling` (close to `Shotgun Surgery`: one transition requires synchronized edits in several files).  
Impact: architectural; `main.rs`, `updater.rs`, `commands.rs`, `windows.rs`, config state; medium/high frequency because updater UX tends to evolve.  
Evidence: shared mutable state type in [commands.rs:17](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:17), poll/store/show in [main.rs:150](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:150), [main.rs:157](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:157), [main.rs:168](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:168), prompt window in [windows.rs:88](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/windows.rs:88), user response logic in [commands.rs:206](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:206), policy/install logic in [updater.rs:14](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/updater.rs:14) and [updater.rs:61](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/updater.rs:61).  
Why: adding “skip this version”, retry, or download progress requires coordinated edits to state storage, UI, command handling, and updater policy.  
Refactor: `Extract Class`/`Introduce State Object` for an `UpdateCoordinator`.  
What disappears: `PendingUpdate` leaking through Tauri state and the cross-file sequencing knowledge.

4. **Crash-recovery semantics leak out of the platform adapter.**  
Smell: `Divergent Change`.  
Impact: architectural; `crash_recovery.rs`, `commands.rs`, `main.rs`, `updater.rs`; medium frequency, high blast radius when autostart or updater behavior changes.  
Evidence: different platform APIs in [crash_recovery.rs:23](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/crash_recovery.rs:23), [crash_recovery.rs:76](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/crash_recovery.rs:76), [crash_recovery.rs:93](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/crash_recovery.rs:93), [crash_recovery.rs:163](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/crash_recovery.rs:163), [crash_recovery.rs:216](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/crash_recovery.rs:216), [crash_recovery.rs:281](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/crash_recovery.rs:281); caller-side policy in [commands.rs:31](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:31), [main.rs:275](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:275), [main.rs:294](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:294), [updater.rs:92](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/updater.rs:92), [updater.rs:99](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/updater.rs:99), [updater.rs:109](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/updater.rs:109), [updater.rs:117](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/updater.rs:117), [updater.rs:125](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/updater.rs:125).  
Why: if Linux/macOS later need verification semantics like Windows, every caller changes instead of only the backend.  
Refactor: `Introduce Facade`/adapter with uniform operations like `arm_for_autostart`, `disarm_for_quit`, `disarm_for_update`.  
What disappears: Windows-only recovery rules and re-arm decisions from app-level code.

5. **Safari/HTTPS support is split between startup repair logic and settings commands.**  
Smell: `Shotgun Surgery`.  
Impact: architectural; `main.rs`, `commands.rs`, `certs.rs`; medium frequency because cert/trust/startup behavior evolves as one feature.  
Evidence: startup preflight/start/repair in [main.rs:54](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:54), [main.rs:71](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:71), [main.rs:83](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:83), [main.rs:95](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:95), [main.rs:105](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:105), [main.rs:373](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:373); settings enable/disable in [commands.rs:134](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:134), [commands.rs:141](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:141), [commands.rs:143](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:143), [commands.rs:152](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:152), [commands.rs:157](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:157), [commands.rs:170](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:170).  
Why: “stop HTTPS immediately”, “retrust CA”, or “rebuild listener after rotation” currently means changing both launch code and command handlers.  
Refactor: `Extract Class` such as `SafariSupportManager` or `HttpsSupport`.  
What disappears: duplicated TLS bring-up, config toggling, and repair/reset logic.

6. **Config writes repeat the same lock-mutate-save script.**  
Smell: `Duplicate Code`, which drives `Shotgun Surgery`.  
Impact: structural; `commands.rs`, `server.rs`, `main.rs`; high frequency because every new setting or remembered origin repeats it.  
Evidence: repeated sequences in [commands.rs:45](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:45), [commands.rs:56](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:56), [commands.rs:145](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:145), [commands.rs:171](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:171), [commands.rs:194](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:194), [commands.rs:216](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:216), [server.rs:382](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:382), [main.rs:105](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:105).  
Why: if persistence gets validation, events, telemetry, or batching, each mutator must be updated by hand.  
Refactor: `Move Function` into a config-store API with named operations.  
What disappears: repeated `write()`, field mutation, `config::save`, and ad hoc error/logging branches.

7. **The HTTP contract is stringly typed.**  
Smell: `Primitive Obsession` mapped to a stringly typed protocol.  
Impact: structural; mostly `server.rs` today, but any client/test change fans out; medium frequency for API evolution.  
Evidence: raw header names in [server.rs:206](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:206), [server.rs:208](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:208), [server.rs:216](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:216), [server.rs:526](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:526), [server.rs:572](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:572); error payload assembly in [server.rs:283](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:283), [server.rs:315](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:315), [server.rs:338](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:338), [server.rs:351](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:351), [server.rs:368](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:368), [server.rs:374](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:374), [server.rs:398](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:398), [server.rs:421](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:421), [server.rs:457](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:457), [server.rs:505](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:505), [server.rs:517](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:517), [server.rs:565](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:565).  
Why: adding `request_id`, renaming a header, or changing the error schema becomes a string hunt instead of a type-guided edit.  
Refactor: `Introduce Data Transfer Object` plus header constants/response builders.  
What disappears: hand-assembled `json!({...})` errors and repeated header literals.

8. **Aztec versions are plain strings at every seam.**  
Smell: `Primitive Obsession`.  
Impact: structural; `versions.rs`, `server.rs`, `bb.rs`; medium/high frequency because cache, retention, and request routing all depend on version semantics.  
Evidence: tier parsing in [versions.rs:38](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/versions.rs:38), retention logic in [versions.rs:153](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/versions.rs:153), validation in [versions.rs:261](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/versions.rs:261), cache-path derivation in [versions.rs:84](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/versions.rs:84), download entrypoint in [versions.rs:272](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/versions.rs:272), header capture/validation in [server.rs:418](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:418) and [server.rs:524](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:524).  
Why: another prerelease channel or richer version grammar means updating multiple unrelated string parsers and validators.  
Refactor: `Introduce Value Object` like `AztecVersion`.  
What disappears: prefix parsing, ad hoc validation, and raw `String` handoff across modules.

9. **`download_bb` owns too many phases.**  
Smell: `Long Method`.  
Impact: structural; 1 file today, but it is the only installer path for non-bundled versions; medium frequency.  
Evidence: cache check, HTTP, bounded streaming, digest verification, temp install, atomic rename, chmod, and macOS post-processing all live in [versions.rs:272](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/versions.rs:272).  
Why: adding mirror fallback, retry policy, progress callbacks, or another platform-specific post-step means editing one long control-flow block with many exits.  
Refactor: `Extract Function` for `download_tarball`, `verify_digest`, `install_version_dir`, `postprocess_downloaded_bb`.  
What disappears: nested phase transitions and mixed concerns inside `download_bb`.

10. **Tray animation is driven by display copy, not state.**  
Smell: `Primitive Obsession` mapped to UI state encoded in strings.  
Impact: local but cross-module; `server.rs` and `main.rs`; medium frequency whenever statuses or UX copy change.  
Evidence: status emission in [server.rs:275](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:275), [server.rs:437](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:437), [server.rs:465](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:465), [server.rs:531](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:531), initial label in [main.rs:268](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:268), busy-state parsing in [main.rs:356](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:356).  
Why: copy changes like “Downloading Aztec binary” or a new busy phase silently change animation behavior unless the substring parser is updated too.  
Refactor: `Introduce Enum`/state object with `display_text()` and `is_busy()`.  
What disappears: `contains("Proving") || contains("Downloading")` and duplicated status literals.

The raw `#[cfg]` splits in `commands.rs`, `certs.rs`, and `crash_recovery.rs` are not separate findings by themselves. The extractable maintenance cost is where platform semantics leak into callers (finding 4) or where one lifecycle is split across startup and commands (finding 5); the remaining per-OS bodies mostly look like legitimate adapters.
## SDK + frontend

**Findings**

1. `checkAcceleratorStatus()` is a protocol multiplexer stuffed into one method.  
Smell: `Long Method` (Fowler).  
Impact: `architectural` | blast radius: every accelerator-detection/status change | change frequency: high.  
Evidence: `packages/sdk/src/lib/accelerator-prover.ts:202-319` with embedded probe setup at `218-228`, cache write path at `230-233`, and protocol/version branches at `271-314`.  
Why it harms future change: adding a third transport, a richer offline reason, or removing legacy health support means reopening one 118-line method that also owns retry timing and cache behavior.  
Smallest safe refactoring: `Extract Method` into `probeHealthEndpoint`, `retryProbe`, `interpretHealthPayload`, `cacheStatus`.  
What disappears: interleaved transport, compatibility, and caching logic in one control-flow block.

2. `createChonkProof()` is the proof orchestrator, fallback policy, network client, timer, and decoder.  
Smell: `Long Method` (Fowler).  
Impact: `architectural` | blast radius: every prove-path change, native/local split, and UI phase change | change frequency: high.  
Evidence: `packages/sdk/src/lib/accelerator-prover.ts:321-406`; only a small fragment is extracted into `#proveLocally` at `413-423`.  
Why it harms future change: streamed progress, new denial handling, different request metadata, or alternate fallbacks all accumulate in one method that already sequences detection, serialization, HTTP, timing, and decode.  
Smallest safe refactoring: `Extract Method` into `proveRemotely`, `proveAfterDenial`, `decodeProofResponse`; keep `createChonkProof` as a short orchestrator.  
What disappears: one hotspot where unrelated prove-flow concerns keep accreting.

3. Phase sequencing is encoded as scattered callback order, not as a model.  
Smell: `Temporal Coupling` (close analog: callers must emit phases in the right order, but that order is implicit and branch-local).  
Impact: `architectural` | blast radius: the SDK/UI contract for all proof progress reporting | change frequency: medium-high.  
Evidence: phase set at `packages/sdk/src/lib/accelerator-prover.ts:11-20`; emission sites at `331-338`, `344`, `351`, `356-357`, `383-389`, `401-404`, `417-422` (14 manual emissions).  
Why it harms future change: adding `queued`, renaming `receive`, or changing fallback UX requires editing multiple branches and manually preserving a valid sequence across native, local, and denied paths.  
Smallest safe refactoring: `Extract Class` for a `PhaseReporter` with `transition(next, data?)` and allowed transitions.  
What disappears: manual phase-order knowledge scattered across many call sites.

4. Phase events are modeled as strings plus one optional generic payload.  
Smell: `Primitive Obsession` (string literals plus an optional bag stand in for a richer event type).  
Impact: `structural` | blast radius: every `onPhase` consumer and every new phase payload | change frequency: medium.  
Evidence: `packages/sdk/src/lib/accelerator-prover.ts:11-25`, `36-43`, with payload only on `"proved"` at `401` and `422`, while all other emissions at `331-389`, `404`, `417` rely on convention.  
Why it harms future change: the first new phase-specific payload like denial metadata or download progress forces consumers to infer meaning from `phase` + `data?` instead of an explicit type.  
Smallest safe refactoring: `Replace Primitive with Object` via a discriminated union event, e.g. `{ type: "proved", durationMs }`.  
What disappears: ambiguous `data?` rules and string/payload pairing conventions.

5. Accelerator endpoint state is split across loose scalars and repeated URL assembly.  
Smell: `Data Clumps` / close analog `Config Sprawl` (host, ports, protocol, and invalidation rules travel together but are managed separately).  
Impact: `structural` | blast radius: all endpoint/config changes | change frequency: medium.  
Evidence: `packages/sdk/src/lib/accelerator-prover.ts:27-34`, `130-135`, `144-167`, `171-179`, `191-196`, `213-214`.  
Why it harms future change: adding a path prefix, TLS toggle, or alternate endpoint kind requires coordinated edits in constructor parsing, setter invalidation, base-url generation, and health probing.  
Smallest safe refactoring: `Introduce Parameter Object` for an `AcceleratorEndpoint` value object with `healthUrls()` and `baseUrl(protocol)`.  
What disappears: duplicated endpoint construction and multi-field synchronization work.

6. `copy-bb.ts` repeats the target-platform matrix in separate conditionals.  
Smell: `Repeated Switches` (Fowler; the same platform/arch decision is encoded more than once).  
Impact: `structural` | blast radius: every new target or sidecar naming rule | change frequency: medium.  
Evidence: `packages/accelerator/scripts/copy-bb.ts:31-46` (`getTargetTriple`), `152-170` (platform/ext/arch/os routing), `172-178` (darwin-only post-step).  
Why it harms future change: adding `windows-arm64`, `linux-musl`, or a new packaging rule means editing multiple branch trees that can drift independently.  
Smallest safe refactoring: `Replace Conditional with Table` by resolving a single `TargetSpec`.  
What disappears: duplicated `process.platform` / `process.arch` branching across helpers and `main()`.

7. The frontend bridge is a global-script dependency, not a real module boundary.  
Smell: `Global Data` (Fowler; page controllers depend on globals and script load order).  
Impact: `structural` | blast radius: all three HTML pages plus the bridge | change frequency: medium.  
Evidence: `packages/accelerator/src-tauri/frontend/tauri-bridge.js:9`, `16-25`, `37-52`, `65-84`; consumers at `settings.html:8,60,155-170`, `authorize.html:8,53-60`, `update-prompt.html:8,31-42`.  
Why it harms future change: renaming helpers, switching to module scripts, or testing page logic outside Tauri requires coordinated HTML/script-order changes because the dependency is implicit.  
Smallest safe refactoring: introduce a module boundary, either one `window.AcceleratorUI` namespace or ES-module page controllers importing a shared helper file.  
What disappears: hidden load-order coupling and the “used by HTML pages” lint suppressions.

8. Frontend IPC wiring is only partially abstracted; the remaining controller code is duplicated.  
Smell: `Duplicate Code` (frontend analog: repeated IPC/control wiring across page scripts, with the shared helper covering only part of the surface).  
Impact: `local` | blast radius: the prompt pages plus the non-toggle settings control | change frequency: medium.  
Evidence: `packages/accelerator/src-tauri/frontend/authorize.html:33-60` and `update-prompt.html:25-42` both do inline query-param read, DOM hydrate, and paired `wireButton` setup; `settings.html:161-172` reimplements async control/error wiring outside `wireToggle`; the partial abstraction boundary is `tauri-bridge.js:28-84`.  
Why it harms future change: if you want uniform busy/error behavior, telemetry, or command-shape changes across dialogs, you still touch several inline scripts instead of one controller API.  
Smallest safe refactoring: `Extract Function` / `Extract Module` for `bindPromptButtons`, `readQueryParamText`, and a generic `wireAsyncControl`.  
What disappears: near-duplicate dialog bootstraps and one-off async control wiring.

**Rejected / trimmed**

- `checkAcceleratorStatus()` as a `Long Method`: confirmed. The candidate is valid at `packages/sdk/src/lib/accelerator-prover.ts:202-319`.
- Ad-hoc `#onPhase?.(...)` emissions: confirmed, but the file has 14 emission sites, not ~17.
- “Shared helper isn’t used everywhere” in the frontend: only partly true. `wireButton`/`wireToggle` are used on all three pages; the real issue is that page bootstrap and the settings speed control still bypass the abstraction, so I would not call the entire frontend pure copy-paste.