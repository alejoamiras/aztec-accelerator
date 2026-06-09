Cluster verdict: 3 findings — 1 architectural, 1 structural, 1 local; the main change-cost is in caller-managed crash-recovery sequencing, while `verified_sites.rs` has no strong maintainability smell.

## Finding 1 — Certificate artifact paths form a lockstep clump
1. **Title** — Certificate artifact triplets are carried as separate paths.
2. **Smell** — `Data Clumps`.
3. **Maintenance impact** — `structural`; blast radius: mainly `packages/accelerator/src-tauri/src/certs.rs`; change frequency: low-to-moderate policy/storage code on the app startup/renewal path.
4. **Concrete evidence** — The same `{ca.pem, localhost.pem, localhost.key}` set is re-declared or manipulated in parallel at `packages/accelerator/src-tauri/src/certs.rs:20-33`, `90-94`, `101-105`, `128`, `204-205`, `262-286`; staged cleanup is another parallel triplet at `277-279`.
5. **Why it harms future change** — Renaming a cert file, adding a fourth artifact, or changing the staging convention requires updating existence checks, writers, loaders, cleanup, and promotion branches separately.
6. **Smallest safe refactoring** — `Introduce Parameter Object` / `Extract Class` for a `CertPaths` and `StagedCertPaths`, then move `exists`, `cleanup_staged`, and `promote` onto that type.
7. **What disappears** — Repeated hard-coded filename triplets and the manual parallel `join`/`remove_file`/`rename` choreography.
8. **Instances** — `packages/accelerator/src-tauri/src/certs.rs:20-33, 90-94, 101-105, 128, 204-205, 262-286`.

## Finding 2 — Crash-recovery sequencing is pushed into callers
1. **Title** — Arm/disarm protocol for crash recovery is spread across the app.
2. **Smell** — `Temporal Coupling` (analog; this behaves like `Shotgun Surgery` because every new quit/update/autostart path must reproduce the same ordered protocol).
3. **Maintenance impact** — `architectural`; blast radius: `crash_recovery.rs`, `main.rs`, `commands.rs`, `updater.rs`; change frequency: moderate whenever quit, autostart, or update flows evolve.
4. **Concrete evidence** — The module itself documents that intentional quit must disable recovery first at `packages/accelerator/src-tauri/src/crash_recovery.rs:233-238`; Windows disable semantics are specialized and stateful at `318-348`; callers manually honor that protocol at `packages/accelerator/src-tauri/src/main.rs:270-275, 284-293`, `packages/accelerator/src-tauri/src/commands.rs:42-50`, and `packages/accelerator/src-tauri/src/updater.rs:86-129`.
5. **Why it harms future change** — Adding another shutdown, restart, updater, or “temporarily stop the app” path means remembering when recovery must be disarmed, when it must be restored, and on which branches that restore is required.
6. **Smallest safe refactoring** — `Extract Method` / introduce a higher-level API such as `sync_with_autostart(enabled)` and a scoped `disarm_for_update()` guard that re-arms on drop unless explicitly committed.
7. **What disappears** — Duplicated sequencing logic and comments that currently carry the protocol instead of the API enforcing it.
8. **Instances** — `packages/accelerator/src-tauri/src/crash_recovery.rs:233-238, 318-348`; `packages/accelerator/src-tauri/src/main.rs:270-275, 284-293`; `packages/accelerator/src-tauri/src/commands.rs:42-50`; `packages/accelerator/src-tauri/src/updater.rs:86-129`.

## Finding 3 — The `CrashRecovery` abstraction is unused indirection
1. **Title** — Trait + ZST wrapper add a seam that nothing uses.
2. **Smell** — `Speculative Generality`.
3. **Maintenance impact** — `local`; blast radius: `packages/accelerator/src-tauri/src/crash_recovery.rs`; change frequency: low, but every edit to this API crosses unnecessary layers.
4. **Concrete evidence** — The abstraction is declared at `packages/accelerator/src-tauri/src/crash_recovery.rs:16-18`, wrapped by free functions at `22-29`, implemented only by `PlatformRecovery` at `35-43`, and there are no other references to `CrashRecovery` or `PlatformRecovery` under `packages/accelerator/src-tauri/src`.
5. **Why it harms future change** — A maintainer has to trace free function → ZST → trait impl → `enable_impl`/`disable_impl`, yet callers still cannot inject an alternate implementation because they call the free functions directly.
6. **Smallest safe refactoring** — `Inline Class` / `Inline Method`: remove the trait and `PlatformRecovery`, and have `enable_crash_recovery` / `disable_crash_recovery` call `enable_impl` / `disable_impl` directly.
7. **What disappears** — One unused abstraction layer and the false impression that the module already has a practical test seam.
8. **Instances** — `packages/accelerator/src-tauri/src/crash_recovery.rs:7-10, 16-18, 22-29, 35-43`.