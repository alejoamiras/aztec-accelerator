## 1. R1 security rewrite

Verdict: The retraction is honest, not cosmetic; the trade is defensible only as explicit risk acceptance, not as equivalent security.

- **Medium:** R2 now correctly admits re-prompting is a real user-presence control, identifies the durable capability gained by an attacker, and labels prompt fatigue unmeasured (`plan.md:155-175`). That fixes R1’s reasoning.
- **Medium:** Disclosure improves informed consent but does not replace renewed authorization. One mistaken click now survives later visits, XSS, and domain transfer.
- **Medium:** The actual disclosure text remains unspecified. Because saving is best-effort (`core/src/server/auth.rs:97-106`), wording such as “will remain approved” is false on write failure. Exact copy should be decided and pinned before implementation.

## 2. New R2 material

Verdict: The stale-path diagnosis is substantially right, but the proposed self-heal is unsafe and incomplete.

- **High:** I5 is contradicted by existing code. `enable_transaction` deliberately refuses to re-run plugin enable when already enabled because macOS recreation strips `KeepAlive`, and a later failure loses recovery (`crash_recovery.rs:75-101`). Disable→enable is idempotent only on successful completion—not atomic, race-safe, or rollback-safe.
- **High:** A second instance can perform startup reconciliation before it discovers the healthy incumbent (`main.rs:270-303,607-625`). During an update, that can recreate launcher/recovery state while the updater has deliberately disarmed recovery (`updater.rs:357-396`). The updater lock is private to updater operations (`updater.rs:44-63`); Phase 2 specifies no coordination.
- **High:** The pinned Windows plugin writes `"<path> <args>"` without quoting the path (`auto-launch-0.5.0/src/windows.rs:37-43`). `"Aztec Accelerator.exe"` therefore makes the rewritten Run command invalid. Re-pointing through `manager.enable()` does not solve the bug.
- **Medium:** The diagnosis is slightly overstated: Windows `is_enabled()` checks Run-value existence **and** the `StartupApproved` disabled state, but never compares paths (`auto-launch-0.5.0/src/windows.rs:73-93`). Normal app/Task-Manager disable should stay off; concurrent disable or external macOS launchctl state is not protected.
- **Low:** R2 is correct to reject the Windows crash-recovery half of Fable’s finding. A launched N re-arms the task from N’s `current_exe()` (`main.rs:614-618`; `crash_recovery.rs:381-410`). macOS really does retain the stale path because existing `KeepAlive` returns early (`crash_recovery.rs:142-153`).
- **Medium:** Phase 7 is sensible integration hygiene, but cannot compensate for invalid earlier gates or the absent updater/single-instance synchronization.

## 3. Validation gates

Verdict: Several R2 gates remain impossible or non-observing.

- **High:** Phase 1 requires old-name-present-before, old-name-absent-after, and new-name-present-after while product code is still unmodified (`plan.md:259-266`). Before Phase 3, N still has the old name, so that gate cannot pass.
- **High:** Adding `workflow_dispatch` alone does not make the updater workflow standalone. It requires `n-version`, downloads N from an artifact produced by its caller, and runs one selected mode (`_e2e-updater-windows.yml:16-55,65-70`). The proposed command supplies no inputs, has no N-producing job, and cannot observe “both positive and negative legs.”
- **High:** “Production Build Smoke” is the playground/browser build (`app.yml:108-128`) and accelerator-only changes do not trigger its path filter (`app.yml:22-35`). It proves nothing about Tauri bundle metadata. The macOS bundle requirement has no executable command.
- **Medium:** R2’s history claim is itself wrong: a job literally named `Release Smoke` does run on accelerator PRs (`accelerator.yml:382-385`).
- **Medium:** Phase 5/6 names bare package scripts—`frontend:build`, `test:e2e:ui`, `test:e2e:webdriver`—that exist only under `packages/accelerator/package.json:8-14`. Even with `bun run --cwd`, WebDriver needs the build/launch preparation in `_e2e-webdriver.yml:62-97`.
- **Low:** Phase 2’s two explicit commands and Phase 3’s local commands are executable; the named WebDriver and Windows jobs exist.

## 4. Assumption attack

Verdict: Most corrections are sound, but new Fact 9 and I5 overclaim safety.

**Facts**

- **Low:** Corrections 7, 8, and 12 check out: the blast radius was undercounted, restart handling does not migrate launchers, and the local tag `accelerator-v1.0.7` exists.
- **Medium:** Fact 9’s stale-path conclusion is correct, but “only checks the Run value exists” omits `StartupApproved`.

**Inferences**

- **Low:** I3/I4 retractions are correct.
- **High:** I5 is false for failure atomicity, updater concurrency, second instances, macOS `KeepAlive`, and spaced Windows paths.

**Asks**

- **Low:** ASK-2 is genuinely optional; the goal seed requires only non-optional phases (`plan.md:307-308,368-372`).
- **Low:** ASK-3/4 are explicitly blocking and excluded from autonomous decision-making (`plan.md:242-247,286-287,381`); they are not silently assumed.

## 5. Implementation critique

Verdict: D2 is right, but the identity migration and persistence-test mechanics remain incomplete.

- **Low:** Full `AuthDecision::Allow` migration is the clean design: it encodes unconditional intended persistence and removes a meaningless parameter (`authorization.rs:181`; `commands.rs:130-150`).
- **Medium:** The promised disk test needs an injectable save destination, but `lock_mutate_save` hardcodes `config_path()` (`config.rs:221-228`) and `HeadlessState` carries no path/sink (`server.rs:107-139`). `config.rs` or server state is missing from the change map.
- **High:** The space does propagate, but currently breaks Linux launching: `main.desktop:6` renders `Exec=env GDK_BACKEND=x11 {{exec}}` without quoting. Phase 3 says merely “inspect” it and omits a mandatory template edit. The pinned bundler also installs binaries under their configured names ([Tauri source](https://raw.githubusercontent.com/tauri-apps/tauri/tauri-cli-v2.10.1/crates/tauri-bundler/src/bundle/linux/debian.rs)).
- **Low:** `README.md:27` still tells macOS users to kill the old process name and is absent from the identity map.

reject (with blocking findings: Phase 1 is internally impossible and lacks a standalone N artifact/mode matrix; the autostart rewrite is non-atomic and races updater/second-instance activity; spaced Windows Run and Linux desktop commands are unquoted; and Phase 4’s Production Build Smoke does not test the Tauri app)