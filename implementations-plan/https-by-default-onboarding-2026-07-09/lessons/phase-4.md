# Phase 4 — Windows trust + renewal window + 3-OS cert CI

## Big win: the Windows code IS validated (not blind)
This Linux box can't build the Tauri app for Windows normally, BUT I got a real Windows type+lint check: `rustup target add x86_64-pc-windows-gnu` + `sudo apt install gcc-mingw-w64-x86-64` + a **dummy `binaries/bb-x86_64-pc-windows-gnu.exe`** placeholder (the only thing the tauri build script was missing) → `cargo check --target x86_64-pc-windows-gnu` and `cargo clippy --target … -D warnings` both **Finished clean**. So `trust/windows.rs` + the `#[cfg(windows)]` renewal paths compile + lint on the real Windows target. (Cleaned up the dummy + target after.) macOS `#[cfg(macos)]` code still CI-only — cross-compiling to Apple needs the macOS SDK — but trust/macos.rs was moved verbatim from working code.

What CI still owns: the RUNTIME behavior — the exact serial string `certutil -delstore` accepts, whether `-addstore Root` prompts in a CI session (spike I3), the macOS install path (spike I7).

## What shipped (Phase 4)
- **trust/windows.rs**: CurrentUser Root via `certutil.exe` (absolute System32 via `SystemRoot` env + hardcoded fallback — the `schtasks_exe` precedent; avoids a windows-sys/FFI dep). Exit-code-only verify (`-verifystore`, locale-independent). **Delete-old-by-serial** (parsed from PEM via x509-parser — D4); **uninstall-by-CN** (no x509-parser at uninstall). Wired into `trust/mod.rs` (windows→windows.rs; stub only for other targets). No guaranteed dialog → wizard Start is the consent (R8).
- **Renewal consent window (§7/D7)**: `certs::leaf_is_expiring()` + `rotate_now()`; `renew_cert` + `record_renewal_prompt` commands; `renewal.html`; `windows::show_renewal_window` (`#[cfg(macos|windows)]`). main.rs: Linux keeps **silent** background rotation; macOS/Windows show the consent window at launch when expiring (throttled by `last_rotation_prompt_at`). Replaces today's context-free background Keychain prompt.
- **Settings HTTPS row now visible on all 3 OSes** (Windows joined); Playwright spec flipped to "visible on Windows".
- **3-OS cert-trust CI matrix** (`accelerator.yml`): ubuntu→trust_linux, windows→trust_windows, macos→trust_macos, `fail-fast:false`.
- **tests/trust_windows.rs**: real CurrentUser Root add/verify/chain(`certutil -verify`)/remove, cleanup-by-CN guard (Drop). **tests/trust_macos.rs**: headless-SAFE subset only (generate + verify/status — NO install, which needs interactive auth per H-1/R5; the real install path is the manual runbook's job — do not read green as prod-path coverage).
- **Release WebDriver pre-gate → 3 OS** (`release-accelerator.yml`, codex catch): the wizard is a release-blocking cross-OS flow.

## Phase 6 (bundled here — packaging/uninstall)
- **NSIS uninstall hook** `src-tauri/nsis/hooks.nsi` + `tauri.conf.json bundle.windows.nsis.installerHooks`. `NSIS_HOOK_POSTUNINSTALL` **guarded by `${If} $UpdateMode <> 1`** (CRITICAL R1 — must not fire on auto-update; must be right in the first hooked release). Deletes by CN + removes the certs dir. `$SYSDIR\certutil.exe` (absolute).
- `.deb depends libnss3-tools` already landed in P3.

## Validation
- ✅ Linux: `cargo build`+`clippy -D warnings`+`cargo test` (24 lib/7 main) clean; fmt; actionlint; biome.
- ✅ **Windows target: `cargo check` + `cargo clippy -D warnings` clean** (trust/windows.rs + renewal).
- ⏳ CI-only (unpushable until workflow scope): the 3-OS cert-trust matrix runtime, Windows updater trust-survives-update assertion (R1 defense-in-depth — the `$UpdateMode` guard is the actual fix; the assertion is a follow-up), WebDriver wizard on 3 OS.

## Deferred / CI-iteration
- Updater "trust survives an over-the-top update" assertion (R1 defense-in-depth): the guard is the fix; the assertion needs a Windows updater-smoke that first installs trust — CI-iteration once pushable.
- macOS install-path CI (I7): stays manual-runbook per H-1/R5.
