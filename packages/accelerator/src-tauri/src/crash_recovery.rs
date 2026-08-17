//! Platform-specific crash recovery.
//!
//! - **macOS**: Patches the LaunchAgent plist to add `KeepAlive` + `ThrottleInterval`,
//!   so launchd restarts the app if it crashes.
//! - **Linux**: Manages a systemd user service with `Restart=on-failure`.
//!
//! The per-platform logic lives behind the [`CrashRecovery`] trait, implemented by the
//! platform-specific `PlatformRecovery` ZST. The `enable_crash_recovery` / `disable_crash_recovery`
//! free functions are thin dispatch onto it, so callers stay platform-agnostic and the surface is
//! mockable in tests.

/// Platform crash-recovery control. `disable` returns whether the recovery mechanism is confirmed
/// disarmed — always `true` where disarm is unconditional (macOS/Linux), the real /Query-verified
/// result on Windows (where the updater MUST know the always-armed task is gone before NSIS mutates
/// files).
pub trait CrashRecovery {
    /// Arm crash recovery. C8: returns `Err` if the ARMING genuinely failed (so a caller mid-transaction
    /// can roll back) and `Ok` for an idempotent already-armed state — see each `enable_impl`'s per-exit
    /// classification in `implementations-plan/security-hardening/closeout-followups/lessons/phase-1.md`.
    fn enable(&self) -> Result<(), String>;
    fn disable(&self) -> bool;
}

/// Enable crash recovery for the current platform (thin dispatch to the platform `CrashRecovery`).
/// C8: `Err` on a real arming failure — callers that arm as part of a transaction (`set_autostart`)
/// roll back on it; the log-and-continue callers (`main.rs` startup rearm, `updater.rs` post-update
/// rearm) must NOT abort on it.
pub fn enable_crash_recovery() -> Result<(), String> {
    PlatformRecovery.enable()
}

/// Disable crash recovery. See [`CrashRecovery::disable`] for the `bool` contract — callers that must
/// know the recovery is gone (the updater, before install) check it.
pub fn disable_crash_recovery() -> bool {
    PlatformRecovery.disable()
}

/// Whether the crash-recovery artifact belongs to `reference` (this install). `--prepare-uninstall` (B5,
/// codex #6) MUST NOT delete it by name alone — that would strip a COPIED second install's recovery.
#[derive(Debug, PartialEq, Eq)]
pub enum RecoveryOwnership {
    /// The task/unit targets this install — safe to disable.
    Ours,
    /// It targets a DIFFERENT install, or we could not confirm — leave it (fail-closed).
    Foreign,
    /// No crash-recovery artifact exists.
    Absent,
}

/// OS-agnostic, pure (testable anywhere): is `xml` a COMPLETE (`</Task>`), SINGLE-action (exactly one
/// `<Command>`) task whose command is exactly `escaped_exe_element` (`<Command>…</Command>`)? Mirrors the
/// NSIS belt (codex #4) and matches in the ESCAPED domain `task_xml` writes, so no decoding is needed.
#[cfg(any(windows, test))]
fn task_xml_is_exactly_ours(xml: &str, escaped_exe_element: &str) -> bool {
    xml.contains("</Task>")
        && xml.matches("<Command>").count() == 1
        && xml.contains(escaped_exe_element)
}

/// macOS: crash recovery is the autostart LaunchAgent's `KeepAlive`, NOT a separate artifact — it is
/// removed with the plist (the autostart step, gated on the ConfirmedOurs/NoEntry verdict the caller
/// already established). Nothing separate to ownership-check.
#[cfg(target_os = "macos")]
pub fn recovery_ownership(_reference: &std::path::Path) -> RecoveryOwnership {
    RecoveryOwnership::Ours
}

#[cfg(target_os = "linux")]
pub fn recovery_ownership(reference: &std::path::Path) -> RecoveryOwnership {
    let Some(config_dir) = dirs::config_dir() else {
        return RecoveryOwnership::Foreign;
    };
    let unit = config_dir.join(format!("systemd/user/{SYSTEMD_NAME}.service"));
    let content = match std::fs::read_to_string(&unit) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return RecoveryOwnership::Absent,
        Err(_) => return RecoveryOwnership::Foreign, // unreadable ⇒ fail-closed
    };
    // Match the EXACT `ExecStart=` value THIS install would write — systemd-serialized (quoted,
    // `:`-prefixed, `%`-doubled; see `systemd_exec_start`), NOT the bare path. `reference` is already the
    // AppImage-resolved identity (owned_reference_path), matching what `recovery_target` stores. A path we
    // cannot represent as a safe ExecStart can't be ours.
    let Some(exec_start) = systemd_exec_start(reference) else {
        return RecoveryOwnership::Foreign;
    };
    let needle = format!("ExecStart={exec_start}");
    if content.lines().any(|l| l.trim() == needle) {
        RecoveryOwnership::Ours
    } else {
        RecoveryOwnership::Foreign
    }
}

#[cfg(target_os = "windows")]
pub fn recovery_ownership(reference: &std::path::Path) -> RecoveryOwnership {
    let out = match std::process::Command::new(schtasks_exe())
        .args(["/Query", "/TN", TASK_NAME, "/XML"])
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        Ok(_) => return RecoveryOwnership::Absent, // /Query nonzero ⇒ task absent
        Err(_) => return RecoveryOwnership::Foreign, // couldn't query ⇒ fail-closed
    };
    // schtasks `/XML` emits UTF-16LE (BOM) on modern Windows; fall back to UTF-8 lossy otherwise.
    let xml = if out.starts_with(&[0xFF, 0xFE]) {
        let u16s: Vec<u16> = out[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&u16s)
    } else {
        String::from_utf8_lossy(&out).into_owned()
    };
    let element = format!(
        "<Command>{}</Command>",
        xml_escape(&reference.display().to_string())
    );
    if task_xml_is_exactly_ours(&xml, &element) {
        RecoveryOwnership::Ours
    } else {
        RecoveryOwnership::Foreign
    }
}

/// The current platform's crash-recovery implementation. A unit struct — the actual state lives in
/// the OS (launchd plist / systemd unit / Task Scheduler task). Dispatches to the `#[cfg]`-selected
/// `enable_impl` / `disable_impl`.
pub struct PlatformRecovery;

impl CrashRecovery for PlatformRecovery {
    fn enable(&self) -> Result<(), String> {
        enable_impl()
    }
    fn disable(&self) -> bool {
        disable_impl()
    }
}

/// C8 (D13/D20): run the autostart-enable transaction with rollback, generic over injected closures so
/// the ordering + completeness + failure modes are unit-testable on Linux CI without a real `AppHandle`.
///
/// Forward: `plugin_enable` (the owned `autostart::set_enabled` launcher-entry writer) then `crash_arm`
/// (`enable_crash_recovery`). On ANY forward failure, roll back **failure-observably**:
/// - run BOTH `crash_disarm` and (only if we changed it — i.e. `!prior_enabled`) `plugin_disable`,
///   executing both even if one fails (no short-circuit),
/// - do NOT unconditionally disable: if `prior_enabled` the launcher was already on before this call, so
///   leave it (restore prior state) rather than clobbering it,
/// - return a combined `Err` naming the failing step and every rollback sub-result.
pub fn enable_transaction<E, A, D, R>(
    prior_enabled: bool,
    plugin_enable: E,
    crash_arm: A,
    plugin_disable: D,
    crash_disarm: R,
) -> Result<(), String>
where
    E: FnOnce() -> Result<(), String>,
    A: FnOnce() -> Result<(), String>,
    D: FnOnce() -> Result<(), String>,
    R: FnOnce() -> bool,
{
    // Step 1 — plugin launcher entry, ONLY when not already enabled. codex r2 #3: re-running the plugin
    // enable when it is ALREADY on is not merely redundant — on macOS the pinned autostart plugin
    // RECREATES the LaunchAgent plist, stripping the existing `KeepAlive` (crash-recovery) keys. A
    // re-enable that then failed at `crash_arm` would therefore destroy the user's recovery even though
    // we "kept" the plugin. Skipping it when already on leaves the prior plist (and its KeepAlive) intact,
    // and `crash_arm` below is idempotent for an already-armed recovery. If this fails on a FRESH enable
    // the plugin state is unchanged; disarm any partial recovery and surface the failure.
    if !prior_enabled {
        if let Err(e) = plugin_enable() {
            let disarmed = crash_disarm();
            return Err(format!(
                "autostart enable failed at plugin step: {e}; rollback: crash_disarm={}",
                disarm_word(disarmed, false)
            ));
        }
    }
    // Step 2 — arm crash recovery. On failure, roll back to the PRIOR state:
    // - plugin: disable it only if we turned it on (`!prior_enabled`); if it was already on, keep it.
    // - crash recovery: codex #7 — do NOT disarm when `prior_enabled`. If autostart was already on before
    //   this call, crash recovery was armed as part of that prior state, and a failed *idempotent re-arm*
    //   must RESTORE the prior recovery, not destroy the user's existing recovery path. Only disarm the
    //   partial recovery we were creating on a fresh enable.
    // Both sub-rollbacks run regardless of either's outcome (no short-circuit).
    if let Err(e) = crash_arm() {
        let disarmed = if prior_enabled { true } else { crash_disarm() };
        let plugin_rolled = if prior_enabled {
            "kept (was already enabled — never re-run; plist + KeepAlive intact)".to_string()
        } else {
            match plugin_disable() {
                Ok(()) => "disabled".to_string(),
                Err(de) => format!("FAILED ({de}) — autostart may still be active"),
            }
        };
        return Err(format!(
            "autostart enable failed at crash-recovery step: {e}; rollback: crash_disarm={}, plugin={plugin_rolled}",
            disarm_word(disarmed, prior_enabled)
        ));
    }
    Ok(())
}

fn disarm_word(disarmed: bool, skipped_because_prior: bool) -> &'static str {
    if skipped_because_prior {
        "skipped (prior enabled)"
    } else if disarmed {
        "confirmed"
    } else {
        "NOT confirmed"
    }
}

/// Must match `productName` in tauri.conf.json — the auto-launch crate uses this
/// (not the identifier) as the LaunchAgent plist filename.
#[cfg(target_os = "macos")]
const APP_NAME: &str = "Aztec Accelerator";

/// Hyphenated name for systemd unit files (spaces break `systemctl` arguments).
#[cfg(target_os = "linux")]
const SYSTEMD_NAME: &str = "aztec-accelerator";

/// Patch the LaunchAgent plist created by the owned autostart writer to add crash recovery keys.
/// Call this after `manager.enable()`.
///
/// Inserts KeepAlive + ThrottleInterval before the LAST `</dict>` (the top-level one).
/// Previous implementation used `.replace("</dict>", ...)` which replaced ALL occurrences
/// and could corrupt plists with nested dicts.
#[cfg(target_os = "macos")]
fn enable_impl() -> Result<(), String> {
    let plist_path = macos_plist_path();
    // read failure ⇒ the plugin's plist isn't where we expect ⇒ arming did NOT happen ⇒ Err.
    let content = std::fs::read_to_string(&plist_path).map_err(|e| {
        format!(
            "cannot read LaunchAgent plist {}: {e}",
            plist_path.display()
        )
    })?;
    if content.contains("<key>KeepAlive</key>") {
        tracing::debug!("LaunchAgent already has KeepAlive");
        return Ok(()); // idempotent already-armed ⇒ success, NOT a failure (must not trigger rollback).
    }
    // no closing </dict> ⇒ patch impossible ⇒ arming failed ⇒ Err.
    let patched = patch_plist_with_keepalive(&content)
        .ok_or_else(|| "could not find closing </dict> in LaunchAgent plist".to_string())?;
    // write failure ⇒ arming failed ⇒ Err.
    std::fs::write(&plist_path, &patched)
        .map_err(|e| format!("failed to write patched LaunchAgent plist: {e}"))?;
    tracing::info!("LaunchAgent patched with KeepAlive (crash recovery)");
    Ok(())
}

/// Insert KeepAlive and ThrottleInterval keys before the last `</dict>` in a plist string.
/// Returns None if no `</dict>` is found.
#[cfg(target_os = "macos")]
fn patch_plist_with_keepalive(content: &str) -> Option<String> {
    let insert_pos = content.rfind("</dict>")?;
    let keep_alive = "\
    <key>KeepAlive</key>\n\
    <dict>\n\
        <key>SuccessfulExit</key>\n\
        <false/>\n\
    </dict>\n\
    <key>ThrottleInterval</key>\n\
    <integer>5</integer>\n  ";
    let mut patched = String::with_capacity(content.len() + keep_alive.len());
    patched.push_str(&content[..insert_pos]);
    patched.push_str(keep_alive);
    patched.push_str(&content[insert_pos..]);
    Some(patched)
}

/// Remove crash recovery keys from the LaunchAgent plist.
/// Call this after `manager.disable()` to clean up.
#[cfg(target_os = "macos")]
fn disable_impl() -> bool {
    // The plugin recreates the plist from scratch on enable(), so disabling
    // just means the standard disable() removes the plist entirely. Nothing extra needed.
    tracing::info!("macOS crash recovery disabled (plist removed by plugin)");
    true
}

#[cfg(target_os = "macos")]
fn macos_plist_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~"))
        .join("Library/LaunchAgents")
        .join(format!("{APP_NAME}.plist"))
}

/// F-010: conservative cross-platform preflight for enabling autostart. Rejects a path whose bytes could
/// INJECT into any OS launcher serializer (systemd unit / `.desktop` / plist XML / Windows Run-key): a
/// non-absolute path, non-UTF-8 (systemd rejects it; the plugin serializes it lossily), or any control /
/// newline / DEL byte (the injection vector for line/element-based unit + plist formats). Platform-specific
/// NON-injection formatting quirks in the third-party `auto-launch` crate (e.g. space-splitting in a raw
/// `.desktop` `Exec=` / Run-key) are a documented ROBUSTNESS residual, not a same-process injection we can
/// close without patching that crate. `set_autostart` calls this BEFORE invoking the plugin, refusing (and
/// disabling) rather than letting an unsafe path be serialized.
pub fn autostart_path_is_safe(exe: &std::path::Path) -> bool {
    match exe.to_str() {
        None => false, // non-UTF-8
        Some(s) => exe.is_absolute() && !s.bytes().any(|b| b < 0x20 || b == 0x7f),
    }
}

/// F-010: serialize an absolute executable path into a safe systemd `ExecStart` value, or `None` if the
/// path is not representable. systemd's `string_is_safe` REJECTS a decoded executable containing controls,
/// `\`, `"`, `'`, or a glob introducer (`*`/`?`/`[`), and requires valid UTF-8 — those are not escapable, so
/// we fail closed. The returned value uses the `:` prefix (disables systemd `$`-environment expansion) INSIDE
/// the quoted first token, and doubles `%` (systemd specifier). No `\`/`"` escaping is needed because they
/// are rejected. Result form: `":/path/with %% doubled"`.
#[cfg(target_os = "linux")]
fn systemd_exec_start(exe: &std::path::Path) -> Option<String> {
    let s = exe.to_str()?; // None ⇒ non-UTF-8
    if !exe.is_absolute() || s.ends_with('/') {
        return None; // must be an absolute file path, not a directory shape
    }
    if s.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return None; // controls / newline / DEL
    }
    if s.chars()
        .any(|c| matches!(c, '\\' | '"' | '\'' | '*' | '?' | '['))
    {
        return None; // systemd `string_is_safe` forbids these in the executable path
    }
    Some(format!("\":{}\"", s.replace('%', "%%")))
}

/// The path a Linux recovery unit must relaunch: the AppImage file when running from one (its
/// mount is ephemeral), else the executable itself. Pure so the choice is unit-tested without
/// touching process env in a parallel test run.
#[cfg(target_os = "linux")]
fn recovery_target(
    appimage_self: Option<std::path::PathBuf>,
    exe: std::path::PathBuf,
) -> std::path::PathBuf {
    // Provenance is decided ONCE, in autostart::appimage_self — an inherited $APPIMAGE from a
    // parent AppImage must never become a relaunch target (r6 #1).
    appimage_self.unwrap_or(exe)
}

/// Create and enable a systemd user service with `Restart=on-failure`.
/// Call this after `manager.enable()`.
#[cfg(target_os = "linux")]
fn enable_impl() -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot determine executable path for systemd service: {e}"))?;
    // r5 #2 (pre-existing bug, surfaced by the ownership work): inside an AppImage `current_exe()`
    // is the ephemeral `/tmp/.mount_XXXX` squashfs that vanishes when the process exits — the very
    // reason autostart stores `$APPIMAGE` instead (autostart::desired_path, D12). A recovery unit
    // pointing at the dead mount can never relaunch anything, which is exactly when it is needed.
    let exe = recovery_target(crate::autostart::appimage_self_from_env(&exe), exe);

    let service_dir = dirs::config_dir()
        .ok_or_else(|| "cannot determine config dir for systemd service".to_string())?
        .join("systemd/user");

    std::fs::create_dir_all(&service_dir)
        .map_err(|e| format!("cannot create systemd user dir: {e}"))?;

    // F-010: build a systemd-escaped ExecStart. An unsafe path (systemd would reject or it could inject
    // a directive) fails CLOSED — remove any stale unit and report the failure (Err), never write a
    // corrupt/injected unit.
    let exec_start = match systemd_exec_start(&exe) {
        Some(v) => v,
        None => {
            disable_impl();
            return Err(
                "executable path is not representable as a safe systemd ExecStart; removed any stale unit"
                    .to_string(),
            );
        }
    };

    let service_path = service_dir.join(format!("{SYSTEMD_NAME}.service"));
    let service_content = format!(
        "[Unit]\n\
         Description=Aztec Accelerator\n\
         After=default.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exec_start}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         StartLimitBurst=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
    );

    std::fs::write(&service_path, &service_content)
        .map_err(|e| format!("failed to write systemd service: {e}"))?;

    // Reload and enable
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
    let result = std::process::Command::new("systemctl")
        .args(["--user", "enable", SYSTEMD_NAME])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            tracing::info!("systemd user service enabled (crash recovery)");
            Ok(())
        }
        // `systemctl enable` is the actual arming step — a failure here is Err (D16/cond.3).
        Ok(output) => Err(format!(
            "systemctl enable failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )),
        Err(e) => Err(format!("failed to run systemctl: {e}")),
    }
}

/// Disable and remove the systemd user service.
/// Call this after `manager.disable()`.
#[cfg(target_os = "linux")]
fn disable_impl() -> bool {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", SYSTEMD_NAME])
        .output();

    // F-010: remove the unit file BEFORE the final daemon-reload (so the reload reflects the removal), and
    // report whether disarm is CONFIRMED — a missing file is success; a file that cannot be removed is not.
    let mut removed = true;
    if let Some(config_dir) = dirs::config_dir() {
        let service_path = config_dir.join(format!("systemd/user/{SYSTEMD_NAME}.service"));
        if let Err(e) = std::fs::remove_file(&service_path) {
            if service_path.exists() {
                tracing::warn!(
                    "Failed to remove systemd unit (crash recovery not confirmed disarmed): {e}"
                );
                removed = false;
            }
        }
    }

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();

    if removed {
        tracing::info!("systemd user service disabled (crash recovery)");
    }
    removed
}

// ── Windows ──────────────────────────────────────────────────────────────────
//
// A Task Scheduler task with a REPEATING TimeTrigger (every PT1M) + IgnoreNew. Every
// minute Task Scheduler tries to start the app; IgnoreNew makes that a no-op if it's
// already running and a RELAUNCH if it died — so a crash recovers within <=1 min.
//
// Why not `RestartOnFailure`: it was the original design, but it's BROKEN for this —
// it does NOT relaunch on a non-zero/abnormal process exit (proven empirically on a
// windows-2025 runner; see lessons/phase-4.md). It only restarts when the task ENGINE
// fails to start the action, not when the action runs then dies. mac launchd
// (KeepAlive{SuccessfulExit:false}) and linux systemd (Restart=on-failure) genuinely
// key on the exit code; the repeating trigger is the working Windows equivalent.
//
// The repeating trigger relaunches ANYTHING not running, so it can't distinguish an
// intentional quit from a crash. The Quit menu therefore calls disable_crash_recovery()
// (delete this task) BEFORE exiting — see the `"quit"` handler in main.rs. A crash skips
// that path → the task survives → relaunch; a clean quit deletes it first → stays down.
// Logon start is handled by the autostart Run key (the owned autostart module), not this
// task; the Run-key-vs-tick race is absorbed by the exit-0-if-healthy guard in main.rs.

#[cfg(target_os = "windows")]
const TASK_NAME: &str = "Aztec Accelerator Crash Recovery";

/// Absolute path to schtasks.exe — avoids a bare-name PATH lookup (same defense as the
/// absolute System32 tar.exe in copy-bb.ts: a planted `schtasks` earlier on PATH can't win).
///
/// **Prefers the hardcoded `C:\Windows\System32\schtasks.exe`** when it exists, so a tainted
/// `SystemRoot`/`windir` environment cannot redirect crash-recovery task management on a standard
/// install. This mirrors `trust::windows::certutil_exe` exactly — F-13 of audit 2026-07-31-9c4cb0c was
/// precisely that the hardening had been applied to one sibling and not the other, so the two are kept
/// deliberately identical rather than abstracted: the shared shape is a *review* habit, and a helper
/// spanning `trust` and `crash_recovery` would couple two unrelated modules for four lines.
///
/// Residual, inherited from the sibling: the env fallback remains for the rare non-standard Windows
/// root where `C:\Windows` is not it. `GetSystemDirectoryW` is the complete fix and would need
/// `windows-sys` promoted to a production dep of THIS crate (it is dev-only here; see Cargo.toml).
#[cfg(target_os = "windows")]
fn schtasks_exe() -> std::path::PathBuf {
    let hardcoded = std::path::PathBuf::from("C:\\Windows\\System32\\schtasks.exe");
    if hardcoded.is_file() {
        return hardcoded;
    }
    let system_root = std::env::var("SystemRoot")
        .or_else(|_| std::env::var("windir"))
        .unwrap_or_else(|_| "C:\\Windows".to_string());
    std::path::Path::new(&system_root)
        .join("System32")
        .join("schtasks.exe")
}

/// Register the Task Scheduler crash-recovery task. Call after `manager.enable()`.
#[cfg(target_os = "windows")]
fn enable_impl() -> Result<(), String> {
    use std::io::Write;

    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot determine executable path for Task Scheduler: {e}"))?;

    // schtasks /XML expects UTF-16LE with a BOM.
    let mut bytes = vec![0xFFu8, 0xFE];
    for unit in task_xml(&exe.display().to_string()).encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }

    // Random temp filename (not a predictable %TEMP% path a local user could pre-create or
    // symlink), written + closed before schtasks reads it, auto-deleted when it drops.
    let xml_path = {
        let mut tmp = tempfile::Builder::new()
            .prefix("aztec-accel-recovery-")
            .suffix(".xml")
            .tempfile()
            .map_err(|e| format!("failed to create Task Scheduler XML temp file: {e}"))?;
        tmp.write_all(&bytes)
            .map_err(|e| format!("failed to write Task Scheduler XML: {e}"))?;
        tmp.flush()
            .map_err(|e| format!("failed to flush Task Scheduler XML: {e}"))?;
        // Close our handle so schtasks can open it; the file persists until this drops.
        tmp.into_temp_path()
    };

    let result = std::process::Command::new(schtasks_exe())
        .args(["/Create", "/F", "/TN", TASK_NAME, "/XML"])
        .arg(&*xml_path)
        .output();
    // xml_path (TempPath) drops at end of scope → the temp file is removed.

    match result {
        Ok(output) if output.status.success() => {
            tracing::info!("Task Scheduler crash-recovery task registered");
            Ok(())
        }
        Ok(output) => Err(format!(
            "schtasks /Create failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )),
        Err(e) => Err(format!("failed to run schtasks: {e}")),
    }
}

/// Remove the Task Scheduler crash-recovery task. Returns `true` if the task is confirmed gone
/// (removed or already absent), `false` if removal could not be verified — callers that rely on
/// the task being gone (the updater, before NSIS mutates files) MUST check this and not proceed.
#[cfg(target_os = "windows")]
fn disable_impl() -> bool {
    // Deletion is correctness-critical now: the repeating-trigger task relaunches the app a
    // minute after an intentional quit if it survives. So retry, and verify via /Query
    // (locale-independent — it exits non-zero when the task is absent) rather than trusting
    // a single best-effort /Delete whose stderr wording varies by Windows language.
    for attempt in 1..=3 {
        let _ = std::process::Command::new(schtasks_exe())
            .args(["/Delete", "/F", "/TN", TASK_NAME])
            .output();
        let still_present = std::process::Command::new(schtasks_exe())
            .args(["/Query", "/TN", TASK_NAME])
            .output()
            .map(|o| o.status.success())
            // If /Query itself can't run, don't claim the task is gone — assume it may persist
            // so we keep retrying and ultimately report failure rather than a false success.
            .unwrap_or(true);
        if !still_present {
            tracing::info!("Task Scheduler crash-recovery task removed (or absent)");
            return true;
        }
        tracing::warn!("crash-recovery task still present after /Delete (attempt {attempt})");
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    tracing::error!(
        "crash-recovery task could NOT be removed after retries — the app may relaunch after an intentional quit"
    );
    false
}

/// Build the Task Scheduler task definition. The exe path is XML-escaped.
#[cfg(target_os = "windows")]
fn task_xml(exe_path: &str) -> String {
    let exe = xml_escape(exe_path);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Aztec Accelerator crash recovery</Description>
  </RegistrationInfo>
  <Triggers>
    <TimeTrigger>
      <StartBoundary>2024-01-01T00:00:00</StartBoundary>
      <Enabled>true</Enabled>
      <Repetition>
        <Interval>PT1M</Interval>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
    </TimeTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
    </Exec>
  </Actions>
</Task>"#
    )
}

#[cfg(target_os = "windows")]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use std::path::Path;

    use super::enable_transaction;
    use std::cell::Cell;

    // The Windows CLI task-ownership decision (B5 codex #6/#4) — the same complete + single-command + exact
    // logic the NSIS belt applies, in Rust. Mutation proof: drop any of the three conjuncts and a case
    // below flips.
    #[test]
    fn task_xml_ownership_requires_complete_single_and_exact() {
        let ours = "<Command>C:\\Install\\AztecAccelerator.exe</Command>";
        let t = super::task_xml_is_exactly_ours;
        assert!(t(
            "<Task><Command>C:\\Install\\AztecAccelerator.exe</Command></Task>",
            ours
        ));
        // foreign path
        assert!(!t(
            "<Task><Command>C:\\Other\\AztecAccelerator.exe</Command></Task>",
            ours
        ));
        // deceptive prefix
        assert!(!t(
            "<Task><Command>C:\\Install-evil\\AztecAccelerator.exe</Command></Task>",
            ours
        ));
        // multi-action (two <Command>)
        assert!(!t(
            "<Task><Command>C:\\Install\\AztecAccelerator.exe</Command><Command>x</Command></Task>",
            ours
        ));
        // truncated AFTER our element (no </Task>)
        assert!(!t(
            "<Task><Command>C:\\Install\\AztecAccelerator.exe</Command>",
            ours
        ));
    }

    // The Linux crash-recovery ownership check reads the unit's serialized ExecStart. Foreign target ⇒
    // leave; absent unit ⇒ Absent. (An OURS unit is covered by systemd_exec_start's round-trip test.)
    #[cfg(target_os = "linux")]
    #[test]
    #[serial_test::serial]
    fn linux_recovery_ownership_leaves_a_foreign_unit() {
        use super::{recovery_ownership, RecoveryOwnership};
        use std::path::Path;
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CONFIG_HOME", home.path());
        let unit_dir = home.path().join("systemd/user");
        std::fs::create_dir_all(&unit_dir).unwrap();
        let unit = unit_dir.join("aztec-accelerator.service");

        // No unit yet ⇒ Absent.
        assert_eq!(
            recovery_ownership(Path::new("/opt/us/AztecAccelerator")),
            RecoveryOwnership::Absent
        );

        // A unit whose ExecStart targets ANOTHER install ⇒ Foreign (must not be deleted).
        let foreign = super::systemd_exec_start(Path::new("/opt/other/AztecAccelerator")).unwrap();
        std::fs::write(&unit, format!("[Service]\nExecStart={foreign}\n")).unwrap();
        assert_eq!(
            recovery_ownership(Path::new("/opt/us/AztecAccelerator")),
            RecoveryOwnership::Foreign
        );

        // A unit whose ExecStart targets US ⇒ Ours.
        let ours = super::systemd_exec_start(Path::new("/opt/us/AztecAccelerator")).unwrap();
        std::fs::write(&unit, format!("[Service]\nExecStart={ours}\n")).unwrap();
        assert_eq!(
            recovery_ownership(Path::new("/opt/us/AztecAccelerator")),
            RecoveryOwnership::Ours
        );
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    // Residual #3 (arc-bug-hunt): the COMPOSITION nothing else covers. `recovery_target` and
    // `systemd_exec_start` were each unit-tested, but no test asserted the ExecStart value the
    // unit actually receives — and that value is the whole point of the AppImage fix: relaunching
    // the ephemeral /tmp/.mount_* path can never work, because the mount is gone exactly when
    // recovery fires. Asserts the composed string, not the two halves separately.
    #[cfg(target_os = "linux")]
    #[test]
    fn appimage_execstart_targets_the_image_not_the_mount() {
        use super::{recovery_target, systemd_exec_start};
        use std::path::PathBuf;
        let mount = PathBuf::from("/tmp/.mount_AbC123/usr/bin/AztecAccelerator");
        let image = PathBuf::from("/home/u/Apps/Aztec Accelerator.AppImage");

        // AppImage run: the unit must relaunch the IMAGE (quoted, `:`-prefixed per F-010).
        let target = recovery_target(Some(image.clone()), mount.clone());
        assert_eq!(target, image);
        assert_eq!(
            systemd_exec_start(&target).as_deref(),
            Some("\":/home/u/Apps/Aztec Accelerator.AppImage\""),
            "spaces are legal in an ExecStart value and must survive; the mount path must not appear"
        );

        // Non-AppImage (or unproven $APPIMAGE): the executable itself, still F-010-serialized.
        let target = recovery_target(None, mount.clone());
        assert_eq!(target, mount);
        assert_eq!(
            systemd_exec_start(&target).as_deref(),
            Some("\":/tmp/.mount_AbC123/usr/bin/AztecAccelerator\"")
        );

        // An image path systemd cannot represent must FAIL CLOSED rather than write a unit that
        // silently relaunches something else (enable_impl turns None into an Err + stale-unit
        // removal).
        assert_eq!(
            systemd_exec_start(&PathBuf::from("/home/u/App\"s.AppImage")),
            None
        );
    }

    // r5 #2: a Linux recovery unit must relaunch the AppImage FILE, never the ephemeral
    // /tmp/.mount_* executable (which is gone exactly when recovery would fire).
    #[cfg(target_os = "linux")]
    #[test]
    fn recovery_target_prefers_the_appimage_file() {
        use super::recovery_target;
        use std::path::PathBuf;
        let mount = PathBuf::from("/tmp/.mount_AbC123/AztecAccelerator");
        assert_eq!(
            recovery_target(
                Some(PathBuf::from("/home/u/Apps/AztecAccelerator.AppImage")),
                mount.clone()
            ),
            PathBuf::from("/home/u/Apps/AztecAccelerator.AppImage")
        );
        // Not an AppImage run, or an UNPROVEN/foreign $APPIMAGE (autostart::appimage_self
        // returned None): the executable itself is the correct relaunch target.
        assert_eq!(recovery_target(None, mount.clone()), mount);
    }

    // C8 (D13/D20): the enable transaction's rollback ordering + completeness + failure-observability,
    // exercised with injected closures — no real AppHandle / OS calls, so it runs on every platform's CI.

    #[test]
    fn enable_transaction_happy_path_no_rollback() {
        let disable_called = Cell::new(false);
        let disarm_called = Cell::new(false);
        let r = enable_transaction(
            false,
            || Ok(()),
            || Ok(()),
            || {
                disable_called.set(true);
                Ok(())
            },
            || {
                disarm_called.set(true);
                true
            },
        );
        assert!(r.is_ok());
        assert!(!disable_called.get(), "no rollback on success");
        assert!(!disarm_called.get(), "no rollback on success");
    }

    #[test]
    fn enable_transaction_arm_fails_prior_disabled_rolls_back_both() {
        let disable_called = Cell::new(false);
        let disarm_called = Cell::new(false);
        let r = enable_transaction(
            false, // prior disabled
            || Ok(()),
            || Err("arm boom".to_string()),
            || {
                disable_called.set(true);
                Ok(())
            },
            || {
                disarm_called.set(true);
                true
            },
        );
        let msg = r.unwrap_err();
        assert!(
            disable_called.get(),
            "prior-disabled ⇒ plugin_disable rolls back the enable"
        );
        assert!(disarm_called.get(), "crash_disarm always runs on rollback");
        assert!(
            msg.contains("arm boom") && msg.contains("plugin=disabled"),
            "combined error: {msg}"
        );
    }

    #[test]
    fn enable_transaction_arm_fails_prior_enabled_keeps_plugin_and_recovery() {
        let enable_called = Cell::new(false);
        let disable_called = Cell::new(false);
        let disarm_called = Cell::new(false);
        let r = enable_transaction(
            true, // prior ENABLED (autostart + crash recovery were both already on)
            || {
                enable_called.set(true);
                Ok(())
            },
            || Err("arm boom".to_string()),
            || {
                disable_called.set(true);
                Ok(())
            },
            || {
                disarm_called.set(true);
                true
            },
        );
        let msg = r.unwrap_err();
        // codex r2 #3: never RE-RUN the plugin enable when already on — on macOS it recreates the
        // LaunchAgent plist and strips the existing KeepAlive (destroying recovery before crash_arm).
        assert!(
            !enable_called.get(),
            "prior-enabled ⇒ do NOT re-run plugin_enable (would strip the plist's KeepAlive on macOS)"
        );
        assert!(
            !disable_called.get(),
            "prior-enabled ⇒ do NOT disable (restore prior state)"
        );
        // codex #7: the key regression guard — a failed idempotent re-arm must NOT destroy the
        // pre-existing crash recovery that was armed as part of the prior enabled state.
        assert!(
            !disarm_called.get(),
            "prior-enabled ⇒ do NOT disarm the user's pre-existing crash recovery"
        );
        assert!(
            msg.contains("kept") && msg.contains("skipped (prior enabled)"),
            "error notes plugin kept + disarm skipped: {msg}"
        );
    }

    #[test]
    fn enable_transaction_plugin_enable_fails_no_arm_no_disable() {
        let arm_called = Cell::new(false);
        let disable_called = Cell::new(false);
        let disarm_called = Cell::new(false);
        let r = enable_transaction(
            false,
            || Err("plugin boom".to_string()),
            || {
                arm_called.set(true);
                Ok(())
            },
            || {
                disable_called.set(true);
                Ok(())
            },
            || {
                disarm_called.set(true);
                true
            },
        );
        let msg = r.unwrap_err();
        assert!(
            !arm_called.get(),
            "crash_arm never runs if the launcher didn't enable"
        );
        assert!(
            !disable_called.get(),
            "nothing to disable — the enable failed"
        );
        assert!(
            disarm_called.get(),
            "defensively disarm any stale recovery on a fresh-enable attempt"
        );
        assert!(
            msg.contains("plugin boom") && msg.contains("plugin step"),
            "error: {msg}"
        );
    }

    #[test]
    fn enable_transaction_rollback_disable_failure_is_surfaced() {
        let r = enable_transaction(
            false,
            || Ok(()),
            || Err("arm boom".to_string()),
            || Err("disable boom".to_string()), // rollback disable ALSO fails
            || false,                           // and disarm not confirmed
        );
        let msg = r.unwrap_err();
        assert!(
            msg.contains("disable boom"),
            "rollback disable failure surfaced: {msg}"
        );
        assert!(
            msg.contains("autostart may still be active"),
            "warns state may be unclean: {msg}"
        );
        assert!(
            msg.contains("NOT confirmed"),
            "disarm-not-confirmed surfaced: {msg}"
        );
    }

    /// F-010: reproduce systemd's ExecStart decode (unquote → strip `:` prefix → specifier-expand `%%`→`%`)
    /// to prove the serializer round-trips exactly to the intended path — i.e. no injection, exact argv0.
    #[cfg(target_os = "linux")]
    fn decode_systemd_exec_start(value: &str) -> String {
        // Our serializer always emits `":<...>"` — one double-quoted token.
        let inner = value
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .expect("quoted token");
        let after_prefix = inner.strip_prefix(':').expect(": prefix"); // strip the exec prefix
        after_prefix.replace("%%", "%") // specifier expansion (only %% appears)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn systemd_exec_start_serializes_and_round_trips() {
        // Plain path.
        let v = super::systemd_exec_start(Path::new("/usr/bin/aztec-accelerator")).unwrap();
        assert_eq!(v, "\":/usr/bin/aztec-accelerator\"");
        assert_eq!(decode_systemd_exec_start(&v), "/usr/bin/aztec-accelerator");
        // The serialized value can NEVER contain a newline (the unit-injection vector) for any accepted path.
        assert!(!v.contains('\n'));
        // A `%`, a space, and a `$` all survive as literals (— `%` doubled, `:` disables `$` expansion).
        let v = super::systemd_exec_start(Path::new("/opt/my app/100% $HOME/bb")).unwrap();
        assert_eq!(v, "\":/opt/my app/100%% $HOME/bb\"");
        assert_eq!(decode_systemd_exec_start(&v), "/opt/my app/100% $HOME/bb");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn systemd_exec_start_rejects_unrepresentable_paths() {
        for bad in [
            "relative/bb",                 // not absolute
            "/dir/",                       // directory shape
            "/x/\nExecStartPre=/bin/evil", // newline injection
            "/x/\tbb",                     // control
            "/x/a\"b",                     // quote (systemd rejects)
            "/x/a\\b",                     // backslash
            "/x/a'b",                      // single quote
            "/x/a*b",                      // glob
            "/x/a?b",
            "/x/a[b",
        ] {
            assert!(
                super::systemd_exec_start(Path::new(bad)).is_none(),
                "should reject {bad:?}"
            );
        }
    }

    #[test]
    fn autostart_preflight_rejects_injection_and_accepts_normal_paths() {
        // A PLATFORM-absolute path is accepted (Windows `is_absolute` needs a drive prefix, so `/usr/...`
        // is NOT absolute there — the test binary runs on Windows CI too).
        #[cfg(unix)]
        let (ok1, ok2, inj_nl, inj_del) = (
            "/usr/bin/aztec-accelerator",
            "/opt/my app/aztec",
            "/x/\nInject",
            "/x/\u{7f}bb",
        );
        #[cfg(windows)]
        let (ok1, ok2, inj_nl, inj_del) = (
            r"C:\Program Files\Aztec\aztec.exe",
            r"C:\my app\aztec.exe", // space + backslash are fine (formatting), not injection
            "C:\\x\\\nInject",
            "C:\\x\\\u{7f}bb",
        );
        assert!(super::autostart_path_is_safe(Path::new(ok1)));
        assert!(super::autostart_path_is_safe(Path::new(ok2)));
        assert!(!super::autostart_path_is_safe(Path::new("relative/bb"))); // not absolute (both platforms)
        assert!(!super::autostart_path_is_safe(Path::new(inj_nl))); // newline injection
        assert!(!super::autostart_path_is_safe(Path::new(inj_del))); // DEL control
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn task_xml_uses_repeating_trigger_and_escapes_exe() {
        let xml = super::task_xml(r"C:\Program Files\A & B\aztec-accelerator.exe");
        // Crash → relaunch is a REPEATING TimeTrigger (every PT1M) + IgnoreNew, proven
        // on a real runner. NOT RestartOnFailure, which does NOT relaunch a dead/crashed
        // process (see the module comment + lessons/phase-4.md).
        assert!(xml.contains("<TimeTrigger>"));
        assert!(xml.contains("<Repetition>"));
        assert!(xml.contains("<Interval>PT1M</Interval>"));
        // Regression guards: the broken mechanism must not come back, and logon-start is
        // the autostart Run key's job (not a LogonTrigger here).
        assert!(
            !xml.contains("<RestartOnFailure>"),
            "RestartOnFailure does not relaunch a crash — regression"
        );
        assert!(
            !xml.contains("<LogonTrigger>"),
            "logon start is the autostart Run key's job, not this task"
        );
        // IgnoreNew = the every-minute tick is a no-op if the app is alive, a relaunch if
        // it died (the exit-0-if-healthy guard in main.rs absorbs the Run-key-vs-tick race).
        assert!(xml.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
        // The raw ampersand must be escaped or the XML is invalid.
        assert!(xml.contains("A &amp; B"));
        assert!(!xml.contains("A & B"));
        // Path text (sans the escaped char) survives.
        assert!(xml.contains(r"C:\Program Files"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn patch_plist_inserts_before_last_dict() {
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>aztec-accelerator</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Applications/Aztec Accelerator.app/Contents/MacOS/aztec-accelerator</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>"#;

        let patched = super::patch_plist_with_keepalive(plist).unwrap();
        assert!(patched.contains("<key>KeepAlive</key>"));
        assert!(patched.contains("<key>ThrottleInterval</key>"));
        assert!(patched.contains("<integer>5</integer>"));
        // Should still have exactly one </plist> and the KeepAlive should be inside the dict
        assert_eq!(patched.matches("</plist>").count(), 1);
        assert_eq!(patched.matches("</dict>").count(), 2); // inner KeepAlive dict + outer
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn patch_plist_handles_nested_dicts() {
        // Plist with a nested dict — the old .replace() would have broken this
        let plist = r#"<dict>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>/usr/bin</string>
    </dict>
    <key>Label</key>
    <string>test</string>
</dict>"#;

        let patched = super::patch_plist_with_keepalive(plist).unwrap();
        assert!(patched.contains("<key>KeepAlive</key>"));
        // The nested EnvironmentVariables dict should be untouched
        assert!(patched.contains("<key>EnvironmentVariables</key>"));
        // KeepAlive should be inserted before the LAST </dict>, not inside the nested one
        let keepalive_pos = patched.find("<key>KeepAlive</key>").unwrap();
        let nested_dict_end = patched.find("<string>/usr/bin</string>").unwrap();
        assert!(
            keepalive_pos > nested_dict_end,
            "KeepAlive should be after the nested dict"
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn patch_plist_returns_none_for_invalid() {
        assert!(super::patch_plist_with_keepalive("not a plist").is_none());
    }

    /// F-13 (audit 2026-07-31-9c4cb0c): a tainted `SystemRoot`/`windir` must not redirect which
    /// `schtasks.exe` we execute. Before the fix this resolver built its path purely from the
    /// environment, while its `certutil_exe` sibling already preferred the hardcoded System32 path —
    /// the finding was exactly that asymmetry.
    ///
    /// This test EXECUTES on real Windows: the `windows-build` CI job (`accelerator.yml`) runs
    /// `cargo test` for this crate on `windows-latest`. That matters — the `NTE_NOT_FOUND` lesson is
    /// that a wrong platform *value* is invisible to any amount of Linux-side purity.
    #[test]
    #[cfg(windows)]
    #[serial_test::serial(windows_system_root)]
    fn poisoned_system_root_cannot_redirect_schtasks() {
        let hardcoded = std::path::Path::new("C:\\Windows\\System32\\schtasks.exe");
        assert!(
            hardcoded.is_file(),
            "precondition: this asserts the STANDARD-install path; on a runner where \
             C:\\Windows\\System32\\schtasks.exe is absent the resolver legitimately falls back to \
             the environment and there is nothing to prove"
        );

        let prior_root = std::env::var_os("SystemRoot");
        let prior_windir = std::env::var_os("windir");
        std::env::set_var("SystemRoot", "C:\\Users\\Public\\evil");
        std::env::set_var("windir", "C:\\Users\\Public\\evil");

        let resolved = super::schtasks_exe();

        // Restore BEFORE asserting: a failing assertion unwinds, and a leaked poisoned environment
        // would contaminate every later test in this binary.
        match prior_root {
            Some(v) => std::env::set_var("SystemRoot", v),
            None => std::env::remove_var("SystemRoot"),
        }
        match prior_windir {
            Some(v) => std::env::set_var("windir", v),
            None => std::env::remove_var("windir"),
        }

        assert_eq!(
            resolved, hardcoded,
            "a poisoned SystemRoot/windir redirected schtasks.exe — F-13 has regressed"
        );
    }
}
