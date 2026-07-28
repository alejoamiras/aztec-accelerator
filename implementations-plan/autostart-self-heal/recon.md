# Recon — autostart self-heal

Phase 0.4. Two read-only sweeps against `main` @ `5d0efa4`, plus one measured spike. Every claim was
verified by reading source (including the vendored crates in `~/.cargo/registry`), not inferred.

Feeds the three planning legs AND every audit. Audits are asked explicitly: *does this design ignore
or duplicate what recon found?*

---

## THE BUG

The app writes an OS-level "start on login" entry containing an **absolute binary path**, once, when
the user toggles it — and never revalidates it. Relocate the app (the standard macOS
download→run→drag-to-`/Applications` flow) and autostart silently dies. Worse, the UI still says ON,
because the underlying crate's `is_enabled()` only checks the entry **exists**.

Owner scoping: **no migration code** — treat the install base as zero. That removes the
upgrade trigger but NOT the relocation trigger, which is a fresh-install scenario.

---

## A. Correcting an earlier claim

Earlier framing said "Windows self-heals, macOS doesn't." **That is true only of crash recovery.**
The autostart entry is stale on **all three platforms** — nothing rewrites it anywhere, ever, after
first write. Windows/Linux crash-recovery `enable_impl()` rebuilds its artifact from `current_exe()`
every call; autostart has no equivalent.

---

## B. The write/read map (file:line)

| Where | What |
|---|---|
| `commands.rs:465-471` | **THE ONLY WRITE.** `enable_transaction(prior_enabled, ‖manager.enable()‖, enable_crash_recovery, ‖manager.disable()‖, disable_crash_recovery)` |
| `commands.rs:450-459` | preflight `autostart_path_is_safe(&exe)` before writing |
| `commands.rs:475-476` | toggle OFF: `manager.disable()` then `disable_crash_recovery()` |
| `commands.rs:45-58` | `get_autostart_enabled` — Settings READ |
| `commands.rs:612` | `complete_onboarding` → same write path |
| `main.rs:607-625` | startup READ; on `Ok(true)` calls `enable_crash_recovery()` — **never touches the autostart artifact** |
| `updater.rs:362-371` | pre-install READ (Windows), drives crash-recovery rearm only |

**Nothing re-validates or rewrites the stored path. That is the entire bug in one line.**

---

## C. The plugin CANNOT tell us what is stored — this is the big scope finding

`AutoLaunchManager` (what `app.autolaunch()` returns) exposes only `enable`/`disable`/`is_enabled`
(`tauri-plugin-autostart-2.5.1/src/lib.rs:49-72`). The underlying `AutoLaunch::get_app_path()` returns
the **in-memory** field, which the plugin rebuilds from `current_exe()` on *every* launch
(`lib.rs:186,202,222`) — not the on-disk value. It is never forwarded anyway.

**So a staleness check requires writing per-platform artifact READERS from scratch**, bypassing the
plugin: parse plist `ProgramArguments[0]`, parse `.desktop` `Exec=`, read the registry string. Plus
careful WRITERS. That is 3+3 new pieces, not "call a setter".

### Per-platform artifact mechanics (quoted from the vendored crate)

| OS | Artifact | `enable()` | `is_enabled()` |
|---|---|---|---|
| macOS | `~/Library/LaunchAgents/{app_name}.plist` (`macos.rs:184-190`) | writes a **fresh** plist from scratch (`macos.rs:70-132`); **no XML escaping** of the path | `Ok(get_file().exists())` (`macos.rs:162-176`) |
| Linux | `~/.config/autostart/{app_name}.desktop` (`linux.rs:80-83`) | `Exec={app_path} {args}` — **unquoted** (`linux.rs:39`) | `Ok(get_file().exists())` (`linux.rs:70-72`) |
| Windows | `HKCU\...\CurrentVersion\Run` value `{app_name}` | `format!("{} {}", app_path, args)` — **unquoted** (`windows.rs:37-43`) | value parses as String **AND** `StartupApproved` last-8-bytes not all-zero (`windows.rs:73-83`) |

`app_name` = `productName` = `"Aztec Accelerator"` (contains a space). Windows `is_enabled()` checks
the Task-Manager disabled state too — so "off stays off" has a real mechanism to respect.

---

## D. Every constraint a design must satisfy

1. **Never re-run `manager.enable()` when already enabled** — `crash_recovery.rs:75-101`. macOS
   `enable()` recreates the plist from scratch, **stripping the `KeepAlive`/`ThrottleInterval` keys
   crash-recovery patched in**. This is why disable→enable is forbidden: it destroys macOS crash
   recovery. Backed by 5 closure-injected tests (`crash_recovery.rs:523-680`).
2. **macOS `enable_crash_recovery()` early-returns on `KeepAlive` presence** (`crash_recovery.rs:141-163`)
   — runs every launch but never compares `ProgramArguments[0]`, so macOS crash recovery is
   permanently stale once armed. Windows/Linux rebuild unconditionally.
3. **Must not run inside the updater's disarm window** — `updater.rs:357-396` deliberately disarms
   Windows crash recovery so no Task Scheduler tick fires mid-NSIS-mutation; `:431-434` rearms
   before `app.restart()` (which never returns).
4. **A redundant instance ALWAYS runs startup reconciliation first.** Quantified: the reconciliation
   point (`main.rs:607-625`) is synchronous and instant; redundant-instance detection lives inside a
   *spawned* task (`main.rs:725-730`) that can take **~8s** (5s `bind_with_retry` + 3s health probe),
   and bow-out is **Windows-only** (`main.rs:296`). macOS/Linux never bow out at all.
5. **Never write an unquoted/unescaped path** — Windows Run (`windows.rs:37-43`), Linux `Exec=`
   (`linux.rs:39`), macOS plist (no XML escaping anywhere, and `autostart_path_is_safe` does not
   reject `&`/`<`/`>`/`"` — they are legal path bytes).
6. **Re-run the F-010 preflight** `autostart_path_is_safe` (`crash_recovery.rs:211-216`) against
   whatever the heal is about to write — today it is only called at user-toggle time.
7. **macOS stores a CANONICALIZED path** (`tauri-plugin-autostart-2.5.1/src/lib.rs:202`
   `current_exe.canonicalize()`). Comparing against a raw `std::env::current_exe()` false-positives
   on symlinks.
8. **`auto-launch`'s Linux `get_dir()` `.unwrap()`s `dirs::home_dir()`** (`linux.rs:82`) — panics if
   `$HOME` is unresolvable. And it hardcodes `$HOME/.config/autostart`, **ignoring `$XDG_CONFIG_HOME`**,
   unlike our own `crash_recovery.rs` Linux path which uses `dirs::config_dir()`. **Test isolation
   must set `$HOME`, not `XDG_CONFIG_HOME`**, or the test silently watches the wrong directory.

---

## E. Testing reality — the honest picture

### What exists today

- **No test anywhere, on any platform, asserts what the app writes to the autostart entry.** The one
  script that touches it (`updater-smoke-windows.ps1:212`) **writes the Run key itself** as a
  precondition and removes it in cleanup.
- `StartupApproved`: zero occurrences in any `.rs`/`.ts`/`.ps1` in the repo.
- Playwright autostart specs are fully mocked (`e2e/tauri-mock.js:33,36`) — they assert the frontend
  calls `invoke("set_autostart")`, nothing about Rust or the OS.
- `e2e-webdriver/` has **zero** autostart references.
- `enable_impl`/`disable_impl` (the functions that actually shell out to `schtasks`/`systemctl`) have
  **no tests at any level**.
- The genuine positives: `task_xml` string-template test (`crash_recovery.rs:757-785`, PR-gated on
  Windows) and the release-only Task-Scheduler arm→disarm→rearm lifecycle proof
  (`updater-smoke-windows.ps1:219-310`).

### BONUS BUG — macOS unit tests never run, anywhere

A **bare** `cargo test` (the only invocation that compiles a crate's `#[cfg(test)]` blocks) exists in
exactly two jobs: `accelerator.yml:109` (Linux) and `accelerator.yml:535` (`windows-build`).
**No macOS job runs one.** `cert-trust`'s macOS leg runs only `--test tls_handshake` and
`--test trust_macos`. So every `#[cfg(target_os = "macos")]` test — including the three
`patch_plist_*` tests (`crash_recovery.rs:787-845`) — **never compiles on any OS in any workflow**.
Fix cost: trivial (one bare `cargo test` on a macOS leg).

### Windows CI — what it genuinely proves (PR-gated)

Compiles + unit-tests the crate; fetches and runs real `bb.exe`; builds a **production** NSIS
installer, installs it silently, launches it and health-probes it; cert-trust add/verify/remove; the
NSIS uninstall-hook behavioural test. That is real. **6 of 10 Windows jobs are release-only or
dispatch-only**, and `_e2e-updater-windows.yml` declares **only `workflow_call`** — `workflow_dispatch`
was *deliberately removed* (`implementations-plan/windows-disarm-proof-2026-06-04/plan.md:84-85`), so
today there is no way to run the Windows updater/crash-recovery smoke without cutting a release.
Windows also **never** runs `built-debug` WebDriver — the only mode exercising the shipped
custom-protocol/CSP path (Linux-only, every trigger).

`_e2e-updater-windows.yml:7-14` claims "no prior Windows STABLE release to download as N-1 yet."
**False, and false since April** — 7 stable releases ship a Windows setup.exe
(`accelerator-v1.0.0`…`v1.0.7`).

### Container testability — MEASURED, not assumed

| Target | Status |
|---|---|
| Wine registry in Docker | **PROVEN.** Wrote a stale Run value, read it back, overwrote it, read the healed value — the exact heal cycle. Image `nsisbox` already built. |
| Rust → `x86_64-pc-windows-gnu` | **PROVEN.** Target + MinGW toolchain present on this host; a real `winreg` binary built first try. |
| Running that binary under the container's wine | **FAILS — exit 53, no output.** Debian bookworm's `wine64` package ships no `wine64` command (only `/usr/lib/wine/wine64`); a fresh `win64` prefix still fails. i686 fallback needs `gcc-mingw-w64-i686`, not installed. **Unresolved — needs a spike, probably a WineHQ build rather than Debian's wine 8.0.** |
| Linux XDG `.desktop` | Fully driveable headlessly — pure filesystem. Must set `$HOME` (see constraint 8). |
| Linux systemd unit **content** | Testable with no systemd: `crash_recovery.rs:286-296` writes the unit file **before** shelling to `systemctl` and does not roll back on failure, so content is assertable regardless of the `Result`. |
| macOS plist patch logic | The patch functions are **pure string transforms** gated behind `#[cfg(target_os = "macos")]` for no technical reason — ungating makes them Linux-container unit-testable. |
| Windows registry natively | No container story without the wine spike above; otherwise needs `windows-latest`. |

---

## F. Reuse / adapt / build-new

**Reuse as-is**
- `crash_recovery.rs:62-114` `enable_transaction` + its 5 tests — the rollback-ordering pattern to imitate.
- `crash_recovery.rs:211-216` `autostart_path_is_safe` — must be re-run before any heal write.
- `crash_recovery.rs:168-183` `patch_plist_with_keepalive` — the exact style for an in-place
  "patch `ProgramArguments[0]`" heal that leaves `KeepAlive` intact.
- `crash_recovery.rs:224-239` `systemd_exec_start` — escaping/validation pattern for `Exec=` rewrites.
- `tests/trust_{linux,macos,windows}.rs` — the ONLY real-OS integration pattern in the repo:
  `#[ignore]`d, throwaway `$HOME` tempdir, run via `cargo test --test X -- --ignored` on the
  OS-matched CI leg. There is no autostart equivalent; this is the shape to copy.

**Adapt**
- `crash_recovery.rs:151-154` macOS early-return → path-aware, without losing idempotency-not-failure.
- `main.rs:607-625` → the heal call site, with `#[cfg]`/webdriver gating matching neighbours.

**Build new (no prior art)**
- Per-platform stored-path **readers** (§C).
- Any instance-coordination primitive — **none exists**; no `tauri-plugin-single-instance`
  (grep-confirmed). Port-bind-and-bow-out is Windows-only and seconds late.
- Injectable `Command` execution for `systemctl`/`schtasks` if a container must assert `Ok`, not just
  file content.

---

## G. Collision risks

- A heal at `main.rs:607-625` races the updater's Windows disarm window exactly as the crash-recovery
  rearm already does — recreating an artifact the updater deliberately removed, mid-install.
- Two live instances at different `current_exe()` paths (mid-relocation zombie + fresh launch) can
  **flip-flop** the stored path; nothing serializes heal writes.
- `main.rs:607-625` is **not** webdriver-gated (unlike neighbouring blocks), so it runs in E2E launches.
- This exact fix is already `plan.md:78-98` + `:436-440` in the `pre-release-polish` plan, marked
  **Unresolved** after two rejected design rounds. Reconcile rather than duplicate.

---

## H. Conventions

- Inline `#[cfg(test)] mod tests` at the bottom of the file under test; pure logic factored out and
  closure/fixture-driven so it needs no AppHandle. Exemplars: `enable_transaction`,
  `systemd_exec_start`, `patch_plist_with_keepalive`, `classify_launch_https`.
- Real-OS integration in `src-tauri/tests/*.rs`, `#[ignore]`d with a reason, `$HOME` tempdir, wired
  into the per-OS CI matrix.
- Comments cite stable IDs (`F-0XX`, `C8 (D..)`, `codex r2 #N`) and justify against a specific
  regression — *why*, not *what*.
