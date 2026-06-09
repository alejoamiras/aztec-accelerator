9 findings. Highest-cost debt is concentrated in a few shared seams, not spread across the repo.  
Top 3: the nullable `AppState`/`HeadlessState` boundary, canonical-origin invariant leakage, and the 203-line Tauri `.setup` bootstrap.  
Overall health: good after the recent refactor; the remaining work is mostly structural consolidation, not broad code rot.

**NOT pursued** — `compute_threads` feature envy (single-use adapter); `CrashRecovery` trait speculative generality (real but minor/local only); cert `rotate()` temporal coupling (folded into the cert-path clump); SDK `catch => offline` (verified false alarm); popup/security/xattr helper duplication and boxed startup-error downcast (real but below final-report bar); duplicate C1/C4 `AppState` reports (deduped here).

### F-01 — Server runtime mode is a nullable state bag that every entrypoint hand-assembles  [priority: high]
- **Smell / mapping**: Data Clumps + nullable Special-Case bag
- **Maintenance impact**: architectural; spans `accelerator-core`, headless server, Tauri, and tests; every new shared server dependency lands here
- **Found by**: both (rebuttals merged C1/C4 into one root cause)
- **Instances**: `packages/accelerator/core/src/server.rs:83-119`; `packages/accelerator/server/src/main.rs:62-75`; `packages/accelerator/src-tauri/src/main.rs:345-367`; `packages/accelerator/core/src/server.rs:641-650, 689-695, 726-737, 911-919, 1013-1035`
- **Description**: `HeadlessState` exposes mostly-optional fields, `AppState` adds three optional callbacks, and each launcher/test manually assembles a different subset with `..Default::default()`.
- **Why it harms future change**: adding one required proving dependency or changing a default means parallel edits across binaries/tests; a missed site silently gets `None` and fails later.
- **Recommended refactoring**: `Extract Factory` + explicit mode types. Add core-owned `headless(...)` / `desktop(...)` constructors, make always-required server deps non-optional, and isolate GUI callbacks into a small desktop extras type. This deletes the repeated semaphore/version literals and the `..Default::default()` omission trap.
- **Effort**: days

### F-02 — Canonical origin is modeled as raw strings, with a production bypass  [priority: high]
- **Smell / mapping**: Primitive Obsession
- **Maintenance impact**: structural; spans auth, config, and headless ingress; touched on every `/prove` auth check and every config load/save path
- **Found by**: both (rebuttals explicitly kept C3 over C5; headless bypass confirmed)
- **Instances**: `packages/accelerator/core/src/authorization.rs:21-58, 77-79, 101-145`; `packages/accelerator/core/src/config.rs:41-55, 83-130`; `packages/accelerator/core/src/server/auth.rs:35-52, 111-116`; `packages/accelerator/server/src/main.rs:43-57`
- **Description**: canonical origins are stored, compared, persisted, and queued as plain `String`s; correctness depends on callers canonicalizing first and on `config::load()` migrating persisted strings. Headless `ALLOWED_ORIGINS` skips that migration and writes raw env strings straight into `approved_origins`.
- **Why it harms future change**: a new ingress or import path can feed non-canonical origins into exact-match approval and compile cleanly; reviewers must re-prove “is this string canonical?” at every call site.
- **Recommended refactoring**: `Replace Primitive with Object` via `CanonicalOrigin` with `serde(try_from = "String")`. Store `Vec<CanonicalOrigin>` and use the newtype across auth/config. This removes comment-only invariants, the `migrate_approved_origins` repair pass, and the headless env bypass.
- **Effort**: days

### F-03 — Tauri startup is still a 203-line god-closure  [priority: high]
- **Smell / mapping**: Long Method + Divergent Change
- **Maintenance impact**: structural; one file, but it is the desktop wiring spine for tray, crash recovery, callbacks, HTTPS/HTTP startup, diagnostics, and updater polling
- **Found by**: both
- **Instances**: `packages/accelerator/src-tauri/src/main.rs:260-462`; especially `266-305`, `316-367`, `369-379`, `396-441`, `448-459`
- **Description**: one `.setup` closure performs most desktop bootstrap responsibilities inline, including nested callback creation and a sizeable inline server-error policy block.
- **Why it harms future change**: adding or reordering one startup step means editing a closure that already captures and reclones `tray`, `status`, `config_state`, `auth_manager`, and `AppHandle` across unrelated concerns.
- **Recommended refactoring**: `Extract Function` into named bootstrap phases plus small callback builders. This leaves `.setup` as orchestration only and removes most clone-stutter.
- **Effort**: days

### F-04 — `versions.rs` is a multi-responsibility hotspot  [priority: high]
- **Smell / mapping**: Large Class
- **Maintenance impact**: structural; one 1209-line core module with multiple direct consumers; every version, platform, cache, download, or eviction change converges here
- **Found by**: codex (Claude rebuttal upheld it and folded smaller C2 items into it)
- **Instances**: `packages/accelerator/core/src/versions.rs:5-13, 15-64, 131-192, 213-320, 342-580`
- **Description**: one module owns HTTP client policy, version parsing/classification, platform naming, URL construction, cache layout, digest lookup, install/extract, and eviction.
- **Why it harms future change**: platform tweaks, retention-policy changes, release-layout changes, and macOS finalization all reopen the same file and force retesting unrelated paths.
- **Recommended refactoring**: `Extract Module` into at least `version_id`, `platform`, `artifact_layout`, `cache`, and `downloader/install`. That removes the single-file collision point and stops platform-finalization logic piggybacking on cache/download orchestration.
- **Effort**: days

### F-05 — The SDK public contract is manually copied across source, barrel, and docs  [priority: high]
- **Smell / mapping**: Duplicate Code + Divergent Change
- **Maintenance impact**: structural; spans the package’s published source, barrel, README, migration doc, and bundled skill; every public API change fans out across them
- **Found by**: both
- **Instances**: `packages/sdk/src/lib/accelerator-prover.ts:45-92, 219-222`; `packages/sdk/src/index.ts:1-8`; `packages/sdk/README.md:61-100`; `packages/sdk/MIGRATION.md:3-43, 83`; `packages/sdk/.claude/skills/aztec-accelerator/SKILL.md:78-114`
- **Description**: the same SDK contract is restated by hand in several places, and they have already drifted: README still documents the old flat `AcceleratorStatus`, MIGRATION says `AcceleratorProtocol` is exported but the barrel omits it, README omits `setForceLocal`, and the skill’s phase table omits `denied`.
- **Why it harms future change**: one API edit now requires synchronized prose, barrel, and example updates; missing one immediately gives consumers and tooling contradictory guidance.
- **Recommended refactoring**: make the barrel the canonical surface, fix the current drift, and add an export/doc sync test or generated API snippet. This removes manual contract duplication and contradictory docs.
- **Effort**: hours

### F-06 — `AcceleratorProver` transport and probe state are split across two client models  [priority: med]
- **Smell / mapping**: Divergent Change + Temporal Coupling
- **Maintenance impact**: structural; localized to one SDK file, but it governs every health probe and every native proof request
- **Found by**: both (transport split converged; round-2 folded the long-method/state-coupling concerns into the same seam)
- **Instances**: `packages/sdk/src/lib/accelerator-prover.ts:163-168, 204-229, 235-374, 414-423`
- **Description**: `/health` uses `fetch` + `Promise.any` + manual retry, while `/prove` uses `ky`; negotiated protocol and cached status are mutated in multiple places in the same class.
- **Why it harms future change**: adding headers, auth, proxying, path prefixes, or richer retry/error rules means touching two HTTP stacks and several cache/protocol invalidation points.
- **Recommended refactoring**: `Extract Class` for `AcceleratorTransport` or similar, owning URL construction, protocol negotiation, cached status, and normalized transport errors. This shrinks or deletes `#probeAndParseHealth` and removes duplicated URL/client policy.
- **Effort**: days

### F-07 — Certificate artifact paths are an undeclared parameter object  [priority: med]
- **Smell / mapping**: Data Clumps
- **Maintenance impact**: structural; concentrated in the Safari/TLS cert lifecycle; moderate change frequency on rotation, generation, and load paths
- **Found by**: both
- **Instances**: `packages/accelerator/src-tauri/src/certs.rs:20-33, 90-128, 201-205, 261-286`
- **Description**: the CA cert, leaf cert, and leaf key paths always travel together, and `rotate()` redefines the same triplet again as staged `*.new` files.
- **Why it harms future change**: renaming a file or adding a fourth TLS artifact requires synchronized edits across existence checks, writers, loaders, staging cleanup, and promotion.
- **Recommended refactoring**: `Introduce Parameter Object` with `CertPaths::live()` / `CertPaths::staged()` plus `exists()` and `promote()`/`swap_into()`. This removes positional 3-path calls and repeated hard-coded filenames.
- **Effort**: hours

### F-08 — `/prove` status ownership is split between `prove` and `resolve_version`  [priority: med]
- **Smell / mapping**: Temporal Coupling
- **Maintenance impact**: local; one hot core path; every new proof phase or status tweak has to preserve this hidden contract
- **Found by**: claude (round-2 kept it as a standalone report finding)
- **Instances**: `packages/accelerator/core/src/server/prove.rs:64-96, 160-165`; characterization at `packages/accelerator/core/src/server.rs:617-685`
- **Description**: `prove()` sets `Proving`, `resolve_version()` may overwrite it with `Downloading`, then `resolve_version()` must restore `Proving` before returning so the caller’s state machine still works.
- **Why it harms future change**: adding another phase or moving the download step forces you to preserve a cross-function ordering rule that neither signature encodes.
- **Recommended refactoring**: `Move Function`/`Extract State Machine`: let `prove()` own the full status sequence and make `resolve_version()` return data only. That removes the restore-to-`Proving` side effect.
- **Effort**: hours

### F-09 — HTTPS startup is duplicated across launch-time and settings-time entrypoints  [priority: low]
- **Smell / mapping**: Duplicate Code
- **Maintenance impact**: structural; two Tauri entrypoints; moderate change frequency whenever HTTPS startup/reporting/shutdown behavior changes
- **Found by**: claude (round-2 kept it as a standalone report finding)
- **Instances**: `packages/accelerator/src-tauri/src/main.rs:55-99`; `packages/accelerator/src-tauri/src/commands.rs:139-167`
- **Description**: both entrypoints perform `load_rustls_config()` and then `spawn(start_https(state, tls))` with the same error logging; only the preamble differs.
- **Why it harms future change**: if HTTPS startup gains handle tracking, success reporting, or shutdown hooks, one path can drift while the other keeps the old behavior.
- **Recommended refactoring**: `Extract Function` into a shared GUI-side `spawn_https(state, tls)` helper, optionally paired with `load_tls_or_log()`. This leaves each entrypoint with only its genuinely different preconditions.
- **Effort**: hours

Couldn’t write `findings/consolidated.md` in this read-only session.