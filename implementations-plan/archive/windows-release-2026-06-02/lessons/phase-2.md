# Phase 2 — Windows platform parity (compile + build)

## Research finding: the codebase was already mostly Windows-aware
The previous devs kept clean cfg hygiene, so the compile surface was small:
- `main.rs` already had a `windows` arm for `open_in_browser` (`explorer`), and the
  macOS activation-policy call is `#[cfg(target_os = "macos")]` (skipped on Windows).
- `certs.rs` / `commands.rs` Safari + CA-trust functions already have clean
  `#[cfg(not(target_os = "macos"))]` **stubs** that return errors — they compile on
  Windows (Safari is correctly macOS-only). No work needed there.

**Only two true compile blockers:**
1. `crash_recovery.rs` — `enable`/`disable_crash_recovery` existed only for macos/linux;
   `commands.rs:36/39` + `main.rs:263` call them unconditionally ⇒ Windows won't link.
2. `versions.rs:86 current_platform()` — the cfg blocks covered only macos/linux, so on
   Windows the fn body is empty ⇒ returns `()` ⇒ type error (expected `&str`).

## Decisions
- **Crash recovery = Task Scheduler** (owner decision), via `schtasks /Create /XML` with a
  logon trigger + `RestartOnFailure`. Mirrors the Linux systemd approach (a *separate*
  recovery mechanism alongside the autostart plugin's Run key). **PROVISIONAL** — the P4
  gating tests resolve crash-vs-quit + the logon double-launch (Run key AND task both fire
  at logon); the watchdog is the documented fallback. Task XML is UTF-16LE+BOM (what
  `schtasks` wants) and the exe path is XML-escaped (a username with `&` would break it).
- **`bb_binary_name()` helper** (`bb.exe` on Windows) threaded through the runtime
  multi-version downloader (`version_bb_path`, the archive lookup, the unpack) — the
  downloader fetches `barretenberg-amd64-windows.tar.gz` whose binary is `bb.exe`.
- **NSIS `installMode: currentUser`** — installs to `%LOCALAPPDATA%`, **no UAC elevation**.
  This is the key enabler for the unsigned silent auto-update (P3/P4): a per-user install
  needs no admin prompt, so the updater can run the installer headlessly. WebView2 =
  `downloadBootstrapper` (fetches the runtime on first install if absent).

## Iteration strategy
Can't cross-compile to Windows from macOS (no MSVC), and local clippy only checks the
macOS cfg. So the **windows-build CI job is the oracle**: `setup-accelerator` composite
(bun + rust + cache + bb prebuild) → `cargo test` on windows-latest. Push → read compile
errors → fix → repeat. The full `tauri build` (NSIS installer) + a launch/health smoke is
the next commit on this branch, once `cargo test` is green.

### Iterations (the windows-latest compiler as oracle)
Each push surfaced the next Windows-specific issue — exactly the value of a real CI gate:

1. **prebuild tar failure** — `gzip: stdin: unexpected end of file`. The composite runs
   prebuild under `shell: bash` (Git Bash), where a bare `tar.exe` resolves to **Git's GNU
   tar**, which mishandles `C:\` paths. (windows-prebuild only passed by luck — it ran under
   pwsh → System32 bsdtar.) Fix: invoke `%SystemRoot%\System32\tar.exe` by absolute path in
   copy-bb.ts — shell-independent. (Checksum had already passed, so it was purely the extractor.)
2. **`icons/icon.ico` not found** — tauri-build needs a `.ico` for the Windows executable
   resource; the icon set had only PNG + icns. Generated a multi-res `.ico` (16–256px) from
   the 1024px `icon.png` via ImageMagick; added to `bundle.icon`.
3. **The crate COMPILED** ✓ — my crash_recovery + versions arms were correct. 6 *test*
   failures remained, all Unix assumptions: forward-slash path asserts (`certs_dir`,
   `version_bb_path`), `"bb"` vs `"bb.exe"` in the extract tests, and the `current_platform`
   allowlist missing `amd64-windows`. Fixed with `bb_binary_name()` + `Path::ends_with`. Also
   found + fixed production `"bb"` hardcoding in `bb.rs` find_bb (the **runtime sidecar
   lookup** — would've stopped the app finding bb.exe to prove) and `list_cached_versions`.
4. **transient** — a 21s `setup-accelerator` cache-restore blip (infra, not code); cleared by
   the next push.
5. Added `tauri build --bundles nsis` (ephemeral throwaway signing key, since
   `createUpdaterArtifacts` requires one) + an installer assertion.

### Result — PASS (PR #269, windows-build green on windows-latest)
The full Windows bundle path works:
- `cargo test` — crate compiles + **all unit tests pass** on Windows (incl. the new
  crash_recovery `task_xml` test).
- `tauri build --bundles nsis` — release build (8m39s) → **`Aztec Accelerator_1.0.4-rc.1_x64-setup.exe`**
  + the `.nsis.zip` updater artifact + `.sig` files (ephemeral key).
- Entire accelerator gate (mac/linux + both Windows jobs) green: 20 pass, 0 fail.

P2 is functionally complete: the Windows app **compiles, tests, and produces an NSIS
installer + updater artifact**. Crash-recovery (Task Scheduler) is wired + compiles; its
runtime semantics are P4's gating tests.

## Codex review (verdict: ship-with-changes)
**Adopted now (the two cheap Mediums):**
- **find_bb PATH-hijack** (`bb.rs:54`): the `which::which("bb")` last-resort resolves via
  PATH+PATHEXT on Windows, so a planted `bb.exe`/`bb.bat` could hijack proving. Gated that
  fallback to `#[cfg(not(windows))]` — Windows requires the bundled sidecar (always present)
  or an explicit `BB_BINARY_PATH`.
- **schtasks operational hardening** (`crash_recovery.rs`): replaced the predictable
  `%TEMP%\aztec-accelerator-recovery.xml` with a random `tempfile` (closed before schtasks
  reads, auto-deleted), and call `%SystemRoot%\System32\schtasks.exe` by absolute path
  (same defense as the tar.exe fix). Codex confirmed no XML-injection in the task path.

**Deferred to P4 (the two Highs — runtime behavior a build-smoke can't catch):**
1. **Dual-launch ghost tray (TOP P4 priority).** The autostart Run key AND the Task
   Scheduler logon trigger are independent launchers; if both fire, one instance loses the
   `:59833` bind — and **the app does NOT self-terminate on bind failure** (verified
   `main.rs:373-381`: it just sets a "port in use" tray tooltip and stays resident → a ghost
   tray with no server). Fix candidates for P4: (a) exit-on-bind-fail (clean single-instance,
   makes double-launch benign on all platforms), (b) suppress the Run key when Task Scheduler
   is active, or (c) the watchdog. The 3 gating tests must include this.
2. **Runtime-downloader self-contained gap.** The dumpbin import-canary only covers the
   *build-time bundled* bb.exe. The runtime multi-version downloader (`versions.rs`
   extract) takes only `bb.exe` and ignores other archive entries, so a *future* Windows bb
   that gains DLLs would break a runtime-downloaded version while CI stays green. Mitigated
   transitively (the build-time canary fires on any bb bump), but P4 should add a runtime
   "only bb.exe present" assert mirroring copy-bb.ts.

**Looks-fine (codex-confirmed):** UTF-16LE+BOM gen correct; bb.exe/amd64-windows wiring
coherent for x64; the ephemeral CI signing key is fine provided those smoke artifacts are
never promoted to release.

Next: merge → P3 (auto-update).
