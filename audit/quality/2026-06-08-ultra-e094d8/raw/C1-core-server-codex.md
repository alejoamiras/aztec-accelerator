3 findings: 1 architectural and 2 structural; the dominant maintenance cost is the `AppState` boundary, not an unreduced long-method problem.

## Finding 1 — Manual `AppState` Assembly Is Duplicated Across Launchers
1. **Title** — Manual `AppState` assembly is duplicated across launchers.
2. **Smell** — `Duplicate Code`.
3. **Maintenance impact** — structural; blast radius 3 production modules; medium-high change frequency because any server bootstrap change touches launcher code.
4. **Concrete evidence** — `packages/accelerator/server/src/main.rs:43-75` and `packages/accelerator/src-tauri/src/main.rs:345-367` both hand-build the same `HeadlessState` bundle (`bundled_version`, `app_version`, `config`, `auth_manager`, `prove_semaphore`, defaulted `https_bound`) and then wrap it in `AppState`; the shared field inventory they must know about lives in `packages/accelerator/core/src/server.rs:84-109`.
5. **Why it harms future change** — adding a new server dependency, changing a default, or tightening invariants requires parallel edits in multiple binaries, and omissions are easy to miss because the constructors lean on `Option` plus `..Default::default()`.
6. **Smallest safe refactoring** — `Replace Constructor with Factory Function`: move launcher-facing creation into core-owned constructors like `AppState::headless(...)` and `AppState::desktop(...)`.
7. **What disappears** — duplicated `Arc::new(...)` / `Some(...)` scaffolding and launcher knowledge of every server field.
8. **Instances** — `packages/accelerator/server/src/main.rs:43-75`; `packages/accelerator/src-tauri/src/main.rs:345-367`.

## Finding 2 — Server Mode Is Encoded By Optional-Field Combinations
1. **Title** — Server mode is encoded by optional-field combinations.
2. **Smell** — `Temporal Coupling` (analog): valid behavior depends on which `AppState`/`HeadlessState` fields were populated together at construction time, so one capability change forces coordinated edits across constructors and consumers.
3. **Maintenance impact** — architectural; blast radius 6 modules; high change frequency because `/health`, `/prove`, auth, and deferred HTTPS startup all depend on this state shape.
4. **Concrete evidence** — `packages/accelerator/core/src/server.rs:84-109,112-117` exposes a public state bag plus `Deref`; `/health` switches on `bundled_version`, `app_version`, and `https_bound` in `packages/accelerator/core/src/server.rs:150-179`; auth semantics switch on `auth_manager`, `config`, and `show_auth_popup` in `packages/accelerator/core/src/server/auth.rs:18-21,49-68,82-85,111-120`; `/prove` switches on `bundled_version`, `on_status`, `on_versions_changed`, `config`, and `prove_semaphore` in `packages/accelerator/core/src/server/prove.rs:59-99,104-113,143-165`; `packages/accelerator/src-tauri/src/commands.rs:22-24,154-161` has to preserve the “full shared state” for later HTTPS startup.
5. **Why it harms future change** — a new runtime mode or field invariant is not localized to one type; maintainers must reason about legal field combinations across multiple handlers and lifecycle entrypoints, and a forgotten field changes behavior later instead of failing at construction.
6. **Smallest safe refactoring** — `Encapsulate Record` on `AppState`/`HeadlessState`, then `Extract Class` for distinct concerns such as desktop callbacks vs authorization/proving runtime, with explicit core constructors for headless vs desktop.
7. **What disappears** — implicit mode encoding through `Option` combinations and cross-module field spelunking through `state.*`.
8. **Instances** — `packages/accelerator/core/src/server.rs:84-117,150-179`; `packages/accelerator/core/src/server/auth.rs:18-21,49-68,82-85,111-120`; `packages/accelerator/core/src/server/prove.rs:59-99,104-113,143-165`; `packages/accelerator/src-tauri/src/commands.rs:22-24,154-161`.

## Finding 3 — Startup Error Handling Depends On Type-Erased Downcasts
1. **Title** — Startup error handling depends on type-erased downcasts.
2. **Smell** — `error-as-control-flow` (analog): callers branch on startup outcomes by recovering structure from `Box<dyn Error>` instead of matching a typed boundary.
3. **Maintenance impact** — structural; blast radius 3 files; medium change frequency because startup, restart, and port-conflict policy evolve over time.
4. **Concrete evidence** — `packages/accelerator/core/src/server.rs:119-125` returns `Result<(), Box<dyn std::error::Error + Send + Sync>>`; `packages/accelerator/server/src/main.rs:77-79` can only log/exit generically; `packages/accelerator/src-tauri/src/main.rs:401-409,432-439` must downcast back to `std::io::Error` to detect `AddrInUse` and choose different recovery/status paths.
5. **Why it harms future change** — any new startup policy (different messaging, retry handling, telemetry, foreign-process classification) pushes more downcasts or stringly inspection into callers instead of extending a stable API contract.
6. **Smallest safe refactoring** — `Change Function Declaration`: return a small typed `ServerStartError` enum from `start`, then let callers match `AddrInUse` directly.
7. **What disappears** — boxed-error knowledge leaking across the boundary and caller-side downcast logic.
8. **Instances** — `packages/accelerator/core/src/server.rs:119-125`; `packages/accelerator/server/src/main.rs:77-79`; `packages/accelerator/src-tauri/src/main.rs:401-409,432-439`.