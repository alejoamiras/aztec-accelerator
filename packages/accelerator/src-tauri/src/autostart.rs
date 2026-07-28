//! Owned autostart ("Start on Login") readers, writers and self-heal.
//!
//! Replaces `tauri-plugin-autostart` entirely (plan D7): the plugin could not report the stored
//! target (its `get_app_path()` is rebuilt from `current_exe()` every launch and never forwarded),
//! its `is_enabled()` was existence-only on macOS/Linux, its Windows `enable()` wrote the Run value
//! UNQUOTED (a live same-user persistence-hijack primitive — plan §9), its Linux writer hardcoded
//! `$HOME/.config` ignoring `XDG_CONFIG_HOME`, and its `get_dir()` panicked on an unresolvable HOME.
//!
//! Layering (plan §4.1):
//! - a PURE layer — parsers/serializers/classifiers for all three platforms, compiled and
//!   unit-tested on every OS (nothing today asserts what the app writes to an autostart entry);
//! - a thin `#[cfg]`-dispatched I/O layer (locate artifact, read, write atomically).
//!
//! The one load-bearing rule (plan D1): **heal iff the stored target does not resolve** — never
//! "differs from `current_exe()`". Resolve-based healing is convergent (every writer writes a path
//! that exists), so racing healers agree, and a leftover copy in `~/Downloads` can never steal a
//! healthy `/Applications` entry. A `Healthy` entry is NEVER written, an `Absent` or `Unreadable`
//! one is NEVER resurrected (plan §9 "never resurrect").

use std::path::{Path, PathBuf};

/// Must match `productName` in tauri.conf.json — it names the LaunchAgent plist, the `.desktop`
/// file and the Windows Run value, exactly as the removed plugin derived them from
/// `package_info().name`. Pinned by `app_name_matches_tauri_conf` below (plan D7: one derivation
/// feeding all three writers, or the platforms drift).
pub(crate) const APP_NAME: &str = "Aztec Accelerator";

// ─────────────────────────────────────────────────────────────────────────────
// Types (plan §4.2)
// ─────────────────────────────────────────────────────────────────────────────

/// Classification of the stored autostart entry (plan D10 — codex's taxonomy, one healing branch).
#[derive(Debug, Clone, PartialEq)]
pub enum StoredTarget {
    /// No entry. NEVER heal — an autostart writer that can create entries is a persistence
    /// primitive (plan §9).
    Absent,
    /// Stored target resolves to an executable. Never written. `points_elsewhere` (canonicalized
    /// target ≠ ours) drives Settings copy ONLY — a healthy entry is never silently stolen by
    /// whichever copy launched last.
    Healthy {
        program: PathBuf,
        points_elsewhere: bool,
    },
    /// Entry parsed fine, target does not resolve. THE ONLY HEALABLE STATE.
    Broken {
        /// The stored program token (for status display; redacted before IPC — plan D24).
        program: String,
    },
    /// I/O or parse failure. Never heal, never write (fail-closed — plan §9 "Injection").
    Unreadable { reason: String },
}

/// Structured status for Settings (plan D3/D13/D14/D17). The switch shows *intent*; health rides
/// in a separate row. Serialized camelCase across IPC.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutostartStatus {
    /// Artifact present AND not platform-disabled (Windows `StartupApproved`, Linux `Hidden=true`,
    /// macOS `Disabled` — plan D13/D20). This is what the switch reflects.
    pub intent_enabled: bool,
    /// The stored target resolves to an executable.
    pub healthy: bool,
    /// The artifact exists but could not be parsed (hand-edited, third-party-rewritten, corrupt).
    /// The heal never touches it; an explicit OFF/ON resets it.
    pub unreadable: bool,
    /// Resolves, but to a different copy than the running one. Informational only.
    pub points_elsewhere: bool,
    /// Our own desired path resolves, so a repair would succeed right now (plan D14 — false when
    /// the app was relocated while running).
    pub can_repair_now: bool,
    /// Redacted stored program (basename + one ancestor, plan D24 — full user paths never cross
    /// IPC; `textContent` on the frontend handles injection, this handles disclosure).
    pub stored_path: Option<String>,
}

/// Outcome of a one-shot heal attempt. Paths inside are logged REDACTED and never cross IPC.
#[derive(Debug, PartialEq)]
pub enum HealOutcome {
    /// Entry absent or healthy — nothing to do.
    NotNeeded,
    Healed {
        from: String,
        to: String,
    },
    /// Preconditions not met (own path unresolvable, updater active, artifact unreadable).
    Skipped(&'static str),
    Failed(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure layer — compiled on every OS (plan §4.1). No fs, no registry, no env.
// ─────────────────────────────────────────────────────────────────────────────

/// D24: redact a path for logs/IPC — keep at most the last two components behind an ellipsis.
/// The Settings copy never needs the full user path, so the webview never receives it.
pub(crate) fn redact_path(p: &str) -> String {
    let sep = if p.contains('\\') { '\\' } else { '/' };
    let parts: Vec<&str> = p.split(sep).filter(|c| !c.is_empty()).collect();
    match parts.len() {
        0 => String::from("…"),
        1 => format!("…{sep}{}", parts[0]),
        _ => format!(
            "…{sep}{}{sep}{}",
            parts[parts.len() - 2],
            parts[parts.len() - 1]
        ),
    }
}

// ── macOS plist (via the `plist` crate — plan D15: already compiled into every build through
//    tauri-utils, so a direct dependency adds zero crates; a real parser is strictly safer than a
//    hand-rolled `<string>` locator for mutate-one-key-preserve-the-rest) ──

#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is macOS-only"
    )
)]
fn plist_root(bytes: &[u8]) -> Result<plist::Dictionary, String> {
    let value = plist::Value::from_reader(std::io::Cursor::new(bytes))
        .map_err(|e| format!("plist parse failed: {e}"))?;
    value
        .into_dictionary()
        .ok_or_else(|| "plist root is not a dictionary".to_string())
}

/// First `ProgramArguments` entry — the program launchd will exec.
#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is macOS-only"
    )
)]
pub(crate) fn plist_program(bytes: &[u8]) -> Result<String, String> {
    let dict = plist_root(bytes)?;
    let args = dict
        .get("ProgramArguments")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "plist has no ProgramArguments array".to_string())?;
    args.first()
        .and_then(|v| v.as_string())
        .map(str::to_string)
        .ok_or_else(|| "ProgramArguments is empty or not strings".to_string())
}

/// D20: the launchd `Disabled` key — the macOS analogue of Windows `StartupApproved`.
#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is macOS-only"
    )
)]
pub(crate) fn plist_disabled(bytes: &[u8]) -> Result<bool, String> {
    let dict = plist_root(bytes)?;
    Ok(dict
        .get("Disabled")
        .and_then(|v| v.as_boolean())
        .unwrap_or(false))
}

/// In-place heal: replace `ProgramArguments[0]`, preserving `KeepAlive`, `ThrottleInterval`,
/// `RunAtLoad`, `Label` and any unknown keys SEMANTICALLY (`plist::Dictionary` is indexmap-backed,
/// so key order survives; only whitespace may differ). This is the operation that must never
/// recreate the file — recreation is what strips crash recovery (C1).
#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is macOS-only"
    )
)]
pub(crate) fn plist_set_program(bytes: &[u8], new_program: &str) -> Result<Vec<u8>, String> {
    let mut dict = plist_root(bytes)?;
    let args = dict
        .get_mut("ProgramArguments")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| "plist has no ProgramArguments array".to_string())?;
    let first = args
        .first_mut()
        .ok_or_else(|| "ProgramArguments is empty".to_string())?;
    if first.as_string().is_none() {
        return Err("ProgramArguments[0] is not a string".to_string());
    }
    *first = plist::Value::String(new_program.to_string());
    render_plist(dict)
}

/// Explicit-ON normalization (plan §4.5): drop a `Disabled=true` override. The HEAL never calls
/// this — a heal repairs the pointer without re-enabling anything (r2 behavioural-delta note).
#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is macOS-only"
    )
)]
pub(crate) fn plist_clear_disabled(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut dict = plist_root(bytes)?;
    dict.remove("Disabled");
    render_plist(dict)
}

/// Fresh enable artifact — same keys the removed plugin wrote (`auto-launch/macos.rs:70-132`:
/// Label + ProgramArguments + RunAtLoad), but XML-escaped by a real serializer.
#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is macOS-only"
    )
)]
pub(crate) fn plist_render_fresh(label: &str, program: &str) -> Result<Vec<u8>, String> {
    let mut dict = plist::Dictionary::new();
    dict.insert("Label".into(), plist::Value::String(label.to_string()));
    dict.insert(
        "ProgramArguments".into(),
        plist::Value::Array(vec![plist::Value::String(program.to_string())]),
    );
    dict.insert("RunAtLoad".into(), plist::Value::Boolean(true));
    render_plist(dict)
}

#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is macOS-only"
    )
)]
fn render_plist(dict: plist::Dictionary) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    plist::Value::Dictionary(dict)
        .to_writer_xml(&mut out)
        .map_err(|e| format!("plist serialize failed: {e}"))?;
    Ok(out)
}

// ── Linux .desktop ──

/// First value for `key` in the `[Desktop Entry]` group. `Err` on duplicates (strict — a duplicated
/// `Exec` is ambiguous, and ambiguity never gets written back).
#[cfg_attr(
    not(target_os = "linux"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is Linux-only"
    )
)]
pub(crate) fn desktop_field(ini: &str, key: &str) -> Result<Option<String>, String> {
    let mut in_entry = false;
    let mut found: Option<String> = None;
    for line in ini.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_entry = trimmed == "[Desktop Entry]";
            continue;
        }
        if !in_entry || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(value) = rest.strip_prefix('=') {
                if found.is_some() {
                    return Err(format!("duplicate {key} in Desktop Entry"));
                }
                found = Some(value.trim_start().to_string());
            }
        }
    }
    Ok(found)
}

/// D20: `Hidden=true` — the freedesktop "this entry is deleted" override.
#[cfg_attr(
    not(target_os = "linux"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is Linux-only"
    )
)]
pub(crate) fn desktop_hidden(ini: &str) -> bool {
    matches!(desktop_field(ini, "Hidden"), Ok(Some(v)) if v.trim() == "true")
}

/// Extract the program from an `Exec=` value: our owned quoted format, or a legacy unquoted value
/// (first space-delimited token — exactly how a spec-compliant launcher would split it, so the
/// classification matches what the OS would actually try to run).
///
/// Decode order is the exact inverse of [`desktop_quote`]: `%%`→`%` first, then unquote, then
/// backslash-unescape. F-B3: decoding happens BEFORE resolution — a healthy `&`- or `%`-containing
/// path must classify `Healthy`, not be rewritten (identically) every launch.
#[cfg_attr(
    not(target_os = "linux"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is Linux-only"
    )
)]
pub(crate) fn desktop_exec_program(exec: &str) -> Result<String, String> {
    let exec = exec.trim();
    if exec.is_empty() {
        return Err("empty Exec value".to_string());
    }
    let undoubled = exec.replace("%%", "\u{0}PCT\u{0}"); // placeholder survives the split below
    let raw_token = if let Some(stripped) = undoubled.strip_prefix('"') {
        // Quoted: scan to the closing unescaped quote.
        let mut out = String::new();
        let mut chars = stripped.chars();
        let mut closed = false;
        while let Some(c) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some(e @ ('"' | '`' | '$' | '\\')) => out.push(e),
                    Some(other) => return Err(format!("invalid Exec escape \\{other}")),
                    None => return Err("unterminated Exec escape".to_string()),
                },
                '"' => {
                    closed = true;
                    break;
                }
                c => out.push(c),
            }
        }
        if !closed {
            return Err("unterminated quoted Exec".to_string());
        }
        // Trailing arguments after a QUOTED program are not ours — neither we nor the removed
        // plugin ever wrote any, so they are a user customization (`Exec="/opt/app" --minimized`).
        // Fail closed, exactly like the Windows reader: the heal rewrites the whole `Exec=` line,
        // so tolerating this would silently delete the user's flags on the next relocation.
        if !chars.as_str().trim().is_empty() {
            return Err("Exec has trailing arguments (not the owned format)".to_string());
        }
        out
    } else {
        // Legacy unquoted (what auto-launch wrote, trailing space included): the OS execs the FIRST
        // token, so classification must too — for a spaced path that token is a path fragment, and
        // the entry is exactly the broken-legacy case the heal exists to repair.
        undoubled.split(' ').next().unwrap_or_default().to_string()
    };
    let program = raw_token.replace("\u{0}PCT\u{0}", "%");
    if program.is_empty() {
        return Err("empty Exec program".to_string());
    }
    Ok(program)
}

/// freedesktop Exec quoting for an absolute path: backslash-escape `\` `"` `` ` `` `$`, wrap in
/// double quotes, then double `%` (field-code escape). `None` for paths no quoting can make safe
/// (controls — same class `autostart_path_is_safe` rejects).
#[cfg_attr(
    not(target_os = "linux"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is Linux-only"
    )
)]
pub(crate) fn desktop_quote(path: &str) -> Option<String> {
    if path.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return None;
    }
    let mut escaped = String::with_capacity(path.len() + 8);
    for c in path.chars() {
        if matches!(c, '\\' | '"' | '`' | '$') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    Some(format!("\"{}\"", escaped).replace('%', "%%"))
}

/// Rewrite the `Exec=` line in place, preserving every other line byte-identically (the in-place
/// heal for `.desktop`, mirroring the plist patch).
#[cfg_attr(
    not(target_os = "linux"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is Linux-only"
    )
)]
pub(crate) fn desktop_set_exec(ini: &str, new_exec_value: &str) -> Result<String, String> {
    // Strictness first: a duplicated Exec is Unreadable, never rewritten.
    if desktop_field(ini, "Exec")?.is_none() {
        return Err("no Exec line in Desktop Entry".to_string());
    }
    let mut out = Vec::with_capacity(ini.len() + 32);
    let mut in_entry = false;
    let mut replaced = false;
    for line in ini.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_entry = trimmed == "[Desktop Entry]";
        }
        if in_entry && !replaced && !trimmed.starts_with('#') && is_key_line(trimmed, "Exec") {
            out.push(format!("Exec={new_exec_value}"));
            replaced = true;
        } else {
            out.push(line.to_string());
        }
    }
    let mut joined = out.join("\n");
    if ini.ends_with('\n') {
        joined.push('\n');
    }
    Ok(joined)
}

/// Explicit-ON normalization: drop a `Hidden=true` line (the heal never does this).
#[cfg_attr(
    not(target_os = "linux"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is Linux-only"
    )
)]
pub(crate) fn desktop_clear_hidden(ini: &str) -> String {
    let mut out = Vec::new();
    let mut in_entry = false;
    for line in ini.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_entry = trimmed == "[Desktop Entry]";
        }
        if in_entry && is_key_line(trimmed, "Hidden") {
            continue;
        }
        out.push(line);
    }
    let mut joined = out.join("\n");
    if ini.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

#[cfg_attr(
    not(target_os = "linux"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is Linux-only"
    )
)]
fn is_key_line(trimmed_line: &str, key: &str) -> bool {
    trimmed_line
        .strip_prefix(key)
        .map(|rest| rest.trim_start().starts_with('='))
        .unwrap_or(false)
}

/// Fresh enable artifact — the removed plugin's shape (`auto-launch/linux.rs:33-45`), with a
/// QUOTED Exec and no trailing space.
#[cfg_attr(
    not(target_os = "linux"),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is Linux-only"
    )
)]
pub(crate) fn desktop_render_fresh(app_name: &str, quoted_exec: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name={app_name}\n\
         Comment={app_name} startup script\n\
         Exec={quoted_exec}\n\
         StartupNotify=false\n\
         Terminal=false\n"
    )
}

// ── Windows Run value ──

/// Quote a Run value: `"` is illegal in Windows paths, so a path containing one is unrepresentable
/// (None); otherwise wrap in quotes. This IS the §9 security fix — the removed plugin wrote the
/// value unquoted, which makes CreateProcess try user-writable prefixes first.
#[cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is Windows-only"
    )
)]
pub(crate) fn run_value_quote(path: &str) -> Option<String> {
    if path.contains('"') || path.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return None;
    }
    Some(format!("\"{path}\""))
}

/// The candidate programs an UNQUOTED Run value resolves to, in documented `CreateProcess` order:
/// each space-prefix (with `.exe` appended when the prefix has no extension), then the full string.
/// For a QUOTED value: the quoted content, provided nothing trails it (trailing arguments are not
/// our owned format — strict, `Err` → Unreadable).
#[cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is Windows-only"
    )
)]
pub(crate) fn run_value_candidates(value: &str) -> Result<Vec<String>, String> {
    let v = value.trim_end();
    if v.is_empty() {
        return Err("empty Run value".to_string());
    }
    if let Some(stripped) = v.strip_prefix('"') {
        let end = stripped
            .find('"')
            .ok_or_else(|| "unterminated quoted Run value".to_string())?;
        let program = &stripped[..end];
        let rest = stripped[end + 1..].trim();
        if !rest.is_empty() {
            return Err("Run value has trailing arguments (not the owned format)".to_string());
        }
        if program.is_empty() {
            return Err("empty quoted Run value".to_string());
        }
        return Ok(vec![program.to_string()]);
    }
    // Unquoted: model the OS exactly (plan §4.7) so classification matches what Windows would run.
    let mut candidates = Vec::new();
    let bytes = v.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b' ' {
            let prefix = &v[..i];
            if !prefix.is_empty() {
                candidates.push(append_exe_if_extensionless(prefix));
            }
        }
    }
    candidates.push(append_exe_if_extensionless(v));
    Ok(candidates)
}

#[cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is Windows-only"
    )
)]
fn append_exe_if_extensionless(p: &str) -> String {
    let last_component = p.rsplit(['\\', '/']).next().unwrap_or(p);
    if last_component.contains('.') {
        p.to_string()
    } else {
        format!("{p}.exe")
    }
}

/// First candidate the injected `exists` accepts — the program the OS would actually launch.
#[cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is Windows-only"
    )
)]
pub(crate) fn resolve_first(
    candidates: &[String],
    exists: &dyn Fn(&str) -> bool,
) -> Option<String> {
    candidates.iter().find(|c| exists(c)).cloned()
}

/// Windows `StartupApproved` semantics: a missing key, missing value or short blob reads ENABLED;
/// otherwise the FLAG BYTE decides — Explorer writes an even flag (`0x02`/`0x06`) for enabled and
/// an odd one (`0x03`/`0x07`) for disabled, with the following 8 bytes carrying the disable
/// timestamp.
///
/// Deliberately diverges from `auto-launch/windows.rs:96-102`, which keys off "last 8 bytes all
/// zero". That infers the flag from the timestamp and misreads in the dangerous direction: a
/// DISABLED blob whose timestamp was zeroed (imaging / GPO / cleanup tooling) reads as enabled,
/// which would rearm the crash-recovery relauncher against an explicit administrator OFF — exactly
/// what "off stays off" forbids. Read-side only: the WRITE stays byte-identical to the crate's
/// enabled blob, so nothing else in the ecosystem sees a new shape.
#[cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "pure layer compiles on every OS so all three formats unit-test on Linux CI (plan §4.1); production use is Windows-only"
    )
)]
pub(crate) fn startup_approved_blob_enabled(bytes: Option<&[u8]>) -> bool {
    match bytes {
        None => true,
        Some(b) if b.len() < 8 => true,
        Some(b) => b[0] % 2 == 0,
    }
}

// ── Classification (shared) ──

/// Is `path` something we'd trust launchd/systemd/CreateProcess to execute — a regular file
/// (following symlinks), with an exec bit on unix?
fn is_regular_executable(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Err(_) => false,
        Ok(md) => {
            if !md.is_file() {
                return false;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                md.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                true
            }
        }
    }
}

/// Shared tail of every platform reader: resolve the parsed program against the filesystem and the
/// desired path. `desired: None` means our own path is unresolvable (relocated-while-running) —
/// classification still works, `points_elsewhere` just can't be computed.
fn classify_program(program: String, desired: Option<&Path>) -> StoredTarget {
    let p = PathBuf::from(&program);
    if is_regular_executable(&p) {
        let points_elsewhere = match desired {
            None => false,
            Some(d) => {
                let stored_canon = p.canonicalize().unwrap_or(p.clone());
                let desired_canon = d.canonicalize().unwrap_or_else(|_| d.to_path_buf());
                stored_canon != desired_canon
            }
        };
        StoredTarget::Healthy {
            program: p,
            points_elsewhere,
        }
    } else {
        StoredTarget::Broken { program }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// I/O layer — artifact locations, atomic writes, the lock (plan §4.4/§4.5, D19)
// ─────────────────────────────────────────────────────────────────────────────

/// D19: the dedicated, short-lived lock serialising OWNED autostart mutations (`set_enabled`,
/// `heal_if_broken`). Deliberately NOT `updater.lock` — `perform_update` holds that across the
/// entire multi-minute download, and taking it here would hard-fail the Settings toggle during any
/// background update (fable, r4). Blocking acquire is safe: holders keep it for one
/// read-modify-write; fs2 releases on process death.
fn acquire_autostart_lock() -> Result<std::fs::File, String> {
    use fs2::FileExt as _;
    let dir = dirs::home_dir()
        .ok_or_else(|| "cannot resolve home directory for autostart lock".to_string())?
        .join(".aztec-accelerator");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create state dir: {e}"))?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(dir.join("autostart.lock"))
        .map_err(|e| format!("cannot open autostart lock: {e}"))?;
    file.lock_exclusive()
        .map_err(|e| format!("cannot acquire autostart lock: {e}"))?;
    Ok(file)
}

/// Atomic same-dir temp + rename, `0600`, refusing a symlinked destination (plan §4.5 —
/// TOCTOU/symlink-swap on a user-writable dotfile path).
#[cfg(not(windows))]
fn write_artifact_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write as _;
    if let Ok(md) = std::fs::symlink_metadata(path) {
        if md.file_type().is_symlink() {
            return Err("autostart artifact is a symlink; refusing to write".to_string());
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| "artifact path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("cannot create artifact dir: {e}"))?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| format!("cannot create temp file: {e}"))?;
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = tmp
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    tmp.write_all(bytes)
        .map_err(|e| format!("cannot write temp file: {e}"))?;
    tmp.persist(path)
        .map_err(|e| format!("cannot persist artifact: {e}"))?;
    Ok(())
}

// ── Platform artifact backends ──

#[cfg(target_os = "macos")]
mod backend {
    use super::*;

    pub(super) fn artifact_path() -> Result<PathBuf, String> {
        // Same file `crash_recovery::macos_plist_path()` patches KeepAlive into — patching
        // ProgramArguments[0] here heals crash recovery's persisted path for free (§4.7; the
        // already-loaded launchd job is the documented until-next-login gap).
        Ok(dirs::home_dir()
            .ok_or_else(|| "cannot resolve home directory".to_string())?
            .join("Library/LaunchAgents")
            .join(format!("{APP_NAME}.plist")))
    }

    pub(super) fn read_raw() -> Result<Option<Vec<u8>>, String> {
        let path = artifact_path()?;
        match std::fs::read(&path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("cannot read LaunchAgent plist: {e}")),
        }
    }

    /// Presence WITHOUT parsing (plan D13): the tolerant question the crash-recovery rearm asks.
    pub(super) fn artifact_present() -> Result<bool, String> {
        Ok(read_raw()?.is_some())
    }

    pub(super) fn read_stored_program() -> Result<Option<String>, String> {
        match read_raw()? {
            None => Ok(None),
            Some(bytes) => plist_program(&bytes).map(Some),
        }
    }

    pub(super) fn platform_disabled() -> Result<bool, String> {
        match read_raw()? {
            None => Ok(false),
            Some(bytes) => plist_disabled(&bytes),
        }
    }

    pub(super) fn heal_write(desired: &Path) -> Result<(), String> {
        let path = artifact_path()?;
        let bytes = std::fs::read(&path).map_err(|e| format!("cannot re-read plist: {e}"))?;
        let patched = plist_set_program(&bytes, &desired.to_string_lossy())?;
        write_artifact_atomic(&path, &patched)
    }

    pub(super) fn enable_write(desired: &Path) -> Result<(), String> {
        let path = artifact_path()?;
        let program = desired.to_string_lossy();
        let new_bytes = match read_raw()? {
            // In-place patch when present and parseable — preserves KeepAlive (C1) — and clear a
            // manual Disabled override (explicit ON resets overrides, §4.5).
            Some(bytes) => match plist_set_program(&bytes, &program) {
                Ok(patched) => plist_clear_disabled(&patched)?,
                // Unreadable + EXPLICIT user ON: recreating is the user's intent (the heal, by
                // contrast, never touches Unreadable). Rollback restores the exact prior bytes.
                Err(_) => plist_render_fresh(APP_NAME, &program)?,
            },
            None => plist_render_fresh(APP_NAME, &program)?,
        };
        write_artifact_atomic(&path, &new_bytes)
    }

    pub(super) fn remove() -> Result<(), String> {
        let path = artifact_path()?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("cannot remove LaunchAgent plist: {e}")),
        }
    }

    pub(super) fn restore_raw(snapshot: Option<Vec<u8>>) -> Result<(), String> {
        match snapshot {
            None => remove(),
            Some(bytes) => write_artifact_atomic(&artifact_path()?, &bytes),
        }
    }
}

#[cfg(target_os = "linux")]
mod backend {
    use super::*;

    pub(super) fn artifact_path() -> Result<PathBuf, String> {
        // D9: honour XDG_CONFIG_HOME (freedesktop autostart spec; parity with
        // crash_recovery.rs's systemd dir) — and NO `.unwrap()` on an unresolvable home (C8).
        Ok(dirs::config_dir()
            .ok_or_else(|| "cannot resolve config directory".to_string())?
            .join("autostart")
            .join(format!("{APP_NAME}.desktop")))
    }

    pub(super) fn read_raw() -> Result<Option<String>, String> {
        let path = artifact_path()?;
        match std::fs::read_to_string(&path) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("cannot read autostart .desktop: {e}")),
        }
    }

    /// Presence WITHOUT parsing (plan D13): the tolerant question the crash-recovery rearm asks.
    pub(super) fn artifact_present() -> Result<bool, String> {
        Ok(read_raw()?.is_some())
    }

    pub(super) fn read_stored_program() -> Result<Option<String>, String> {
        match read_raw()? {
            None => Ok(None),
            Some(ini) => {
                let exec = desktop_field(&ini, "Exec")?
                    .ok_or_else(|| "no Exec line in Desktop Entry".to_string())?;
                desktop_exec_program(&exec).map(Some)
            }
        }
    }

    pub(super) fn platform_disabled() -> Result<bool, String> {
        Ok(match read_raw()? {
            None => false,
            Some(ini) => desktop_hidden(&ini),
        })
    }

    pub(super) fn heal_write(desired: &Path) -> Result<(), String> {
        let path = artifact_path()?;
        let ini =
            std::fs::read_to_string(&path).map_err(|e| format!("cannot re-read .desktop: {e}"))?;
        let quoted = desktop_quote(&desired.to_string_lossy())
            .ok_or_else(|| "desired path is not representable in Exec".to_string())?;
        let rewritten = desktop_set_exec(&ini, &quoted)?;
        write_artifact_atomic(&path, rewritten.as_bytes())
    }

    pub(super) fn enable_write(desired: &Path) -> Result<(), String> {
        let path = artifact_path()?;
        let quoted = desktop_quote(&desired.to_string_lossy())
            .ok_or_else(|| "desired path is not representable in Exec".to_string())?;
        let new_ini = match read_raw()? {
            Some(ini) => match desktop_set_exec(&ini, &quoted) {
                Ok(rewritten) => desktop_clear_hidden(&rewritten),
                Err(_) => desktop_render_fresh(APP_NAME, &quoted),
            },
            None => desktop_render_fresh(APP_NAME, &quoted),
        };
        write_artifact_atomic(&path, new_ini.as_bytes())
    }

    pub(super) fn remove() -> Result<(), String> {
        let path = artifact_path()?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("cannot remove autostart .desktop: {e}")),
        }
    }

    pub(super) fn restore_raw(snapshot: Option<String>) -> Result<(), String> {
        match snapshot {
            None => remove(),
            Some(ini) => write_artifact_atomic(&artifact_path()?, ini.as_bytes()),
        }
    }
}

#[cfg(windows)]
mod backend {
    use super::*;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
    use winreg::RegKey;

    const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
    const STARTUP_APPROVED_KEY: &str =
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    /// `auto-launch`'s exact "enabled" blob — explicit ON writes it; the heal and OFF never touch
    /// the key (§4.5: a Task-Manager OFF must survive everything except an explicit ON).
    const STARTUP_APPROVED_ENABLED: [u8; 12] = [0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    pub(super) fn read_raw() -> Result<Option<String>, String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey_with_flags(RUN_KEY, KEY_READ)
            .map_err(|e| format!("cannot open Run key: {e}"))?;
        match key.get_value::<String, _>(APP_NAME) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("cannot read Run value: {e}")),
        }
    }

    /// Presence WITHOUT parsing (plan D13): the Run value exists, whatever its shape.
    pub(super) fn artifact_present() -> Result<bool, String> {
        Ok(read_raw()?.is_some())
    }

    pub(super) fn read_stored_program() -> Result<Option<String>, String> {
        match read_raw()? {
            None => Ok(None),
            Some(value) => {
                let candidates = run_value_candidates(&value)?;
                // The program the OS would launch: first existing candidate, else the first
                // candidate (for Broken display) — resolution happens in classify_program.
                let exists = |c: &str| is_regular_executable(Path::new(c));
                Ok(Some(
                    resolve_first(&candidates, &exists).unwrap_or_else(|| candidates[0].clone()),
                ))
            }
        }
    }

    pub(super) fn platform_disabled() -> Result<bool, String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let blob = hkcu
            .open_subkey_with_flags(STARTUP_APPROVED_KEY, KEY_READ)
            .ok()
            .and_then(|k| k.get_raw_value(APP_NAME).ok());
        Ok(!startup_approved_blob_enabled(
            blob.as_ref().map(|v| v.bytes.as_slice()),
        ))
    }

    pub(super) fn heal_write(desired: &Path) -> Result<(), String> {
        write_quoted(desired)
    }

    pub(super) fn enable_write(desired: &Path) -> Result<(), String> {
        write_quoted(desired)?;
        // Explicit ON resets a Task-Manager OFF, exactly as auto-launch's enable() did: the key
        // may legitimately not exist (tolerate the open error, as the crate does), but a FAILED
        // WRITE is not tolerable — swallowing it would report an enable that leaves the entry
        // Task-Manager-disabled, i.e. autostart silently off.
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(key) = hkcu.open_subkey_with_flags(STARTUP_APPROVED_KEY, KEY_SET_VALUE) {
            key.set_raw_value(
                APP_NAME,
                &winreg::RegValue {
                    vtype: winreg::enums::RegType::REG_BINARY,
                    bytes: STARTUP_APPROVED_ENABLED.to_vec(),
                },
            )
            .map_err(|e| format!("cannot clear the Task Manager startup override: {e}"))?;
        }
        Ok(())
    }

    fn write_quoted(desired: &Path) -> Result<(), String> {
        let quoted = run_value_quote(&desired.to_string_lossy())
            .ok_or_else(|| "desired path is not representable as a Run value".to_string())?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
            .map_err(|e| format!("cannot open Run key for write: {e}"))?
            .set_value(APP_NAME, &quoted)
            .map_err(|e| format!("cannot write Run value: {e}"))
    }

    pub(super) fn remove() -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
            .map_err(|e| format!("cannot open Run key for delete: {e}"))?;
        match key.delete_value(APP_NAME) {
            Ok(()) => Ok(()),
            // Idempotent OFF: already absent is success (the removed plugin errored here).
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("cannot delete Run value: {e}")),
        }
    }

    pub(super) fn restore_raw(snapshot: Option<String>) -> Result<(), String> {
        match snapshot {
            None => remove(),
            Some(value) => {
                let hkcu = RegKey::predef(HKEY_CURRENT_USER);
                hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)
                    .map_err(|e| format!("cannot open Run key for restore: {e}"))?
                    .set_value(APP_NAME, &value)
                    .map_err(|e| format!("cannot restore Run value: {e}"))
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public surface (plan §4.2)
// ─────────────────────────────────────────────────────────────────────────────

/// The path an autostart entry SHOULD launch (plan D11/D12):
/// - macOS: `current_exe().canonicalize()` (C7 — the plugin stored canonicalized; comparing raw
///   would false-positive on symlinks);
/// - Linux: `app.env().appimage` when present — inside an AppImage, `current_exe()` points into
///   the ephemeral `/tmp/.mount_XXXX` squashfs that vanishes at exit (D12) — else `current_exe()`;
/// - Windows: `current_exe()` VERBATIM — `canonicalize()` can yield an extended-length `\\?\`
///   path whose Run-value compatibility is unproven (D11); canonicalize only for comparison.
pub fn desired_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    {
        use tauri::Manager as _;
        if let Some(appimage) = app.env().appimage {
            let p = PathBuf::from(appimage);
            return Ok(p.canonicalize().unwrap_or(p));
        }
        let exe =
            std::env::current_exe().map_err(|e| format!("cannot resolve executable path: {e}"))?;
        Ok(exe.canonicalize().unwrap_or(exe))
    }
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        let exe =
            std::env::current_exe().map_err(|e| format!("cannot resolve executable path: {e}"))?;
        exe.canonicalize()
            .map_err(|e| format!("cannot canonicalize executable path: {e}"))
    }
    #[cfg(windows)]
    {
        let _ = app;
        std::env::current_exe().map_err(|e| format!("cannot resolve executable path: {e}"))
    }
}

/// Read and classify the stored entry. `desired: None` ⇒ `points_elsewhere` can't be computed.
pub fn read_stored_target(desired: Option<&Path>) -> StoredTarget {
    match backend::read_stored_program() {
        Err(reason) => StoredTarget::Unreadable { reason },
        Ok(None) => StoredTarget::Absent,
        Ok(Some(program)) => classify_program(program, desired),
    }
}

/// D13/D20: artifact present AND not platform-disabled (`StartupApproved` / `Hidden=true` /
/// launchd `Disabled`). This is what the startup crash-recovery rearm and the updater's
/// pre-install capture key off — NOT health: a Broken entry still means "the user wants
/// autostart", and keying the rearm off health would stop protecting exactly the users whose
/// entry broke.
///
/// Deliberately keyed on PRESENCE, not parseability: an entry we cannot parse still means the user
/// asked for autostart (the removed plugin's existence-only `is_enabled()` said so too), so an
/// odd artifact must not silently drop those users out of crash-recovery protection. Only a real
/// I/O failure is `Err` — codex #7's "unknown state is not a confirmed off" still holds for that.
/// The strict, never-write-what-we-don't-understand reading lives in [`read_stored_target`], which
/// is what the heal keys off.
pub fn intent_enabled(_app: &tauri::AppHandle) -> Result<bool, String> {
    intent_enabled_now()
}

/// `AppHandle`-free form of [`intent_enabled`] (L3 test surface).
pub fn intent_enabled_now() -> Result<bool, String> {
    if !backend::artifact_present()? {
        return Ok(false);
    }
    Ok(!backend::platform_disabled()?)
}

/// Structured status for Settings (plan §5). Read-only: opening Settings NEVER writes OS state.
pub fn status(app: &tauri::AppHandle) -> Result<AutostartStatus, String> {
    let desired = desired_path(app).ok();
    let can_repair_now = desired
        .as_deref()
        .map(|d| {
            d.try_exists().unwrap_or(false) && crate::crash_recovery::autostart_path_is_safe(d)
        })
        .unwrap_or(false);
    match read_stored_target(desired.as_deref()) {
        // An artifact we cannot PARSE is not an unknown state — we know it is there, and the user
        // must keep both controls (OFF always works; explicit ON resets it). Only a real I/O
        // failure reaches the caller as `Err`, which is what keeps the switch disabled on genuinely
        // unknown state (codex #7). Reporting parse-strangeness as `Err` here left the switch
        // permanently disabled with no way back — worse than the plugin it replaced.
        StoredTarget::Unreadable { .. } => Ok(AutostartStatus {
            intent_enabled: intent_enabled_now()?,
            healthy: false,
            unreadable: true,
            points_elsewhere: false,
            can_repair_now,
            stored_path: None,
        }),
        StoredTarget::Absent => Ok(AutostartStatus {
            intent_enabled: false,
            healthy: true,
            unreadable: false,
            points_elsewhere: false,
            can_repair_now,
            stored_path: None,
        }),
        StoredTarget::Healthy {
            program,
            points_elsewhere,
        } => Ok(AutostartStatus {
            intent_enabled: intent_enabled_now()?,
            healthy: true,
            unreadable: false,
            points_elsewhere,
            can_repair_now,
            stored_path: Some(redact_path(&program.to_string_lossy())),
        }),
        StoredTarget::Broken { program } => Ok(AutostartStatus {
            intent_enabled: intent_enabled_now()?,
            healthy: false,
            unreadable: false,
            points_elsewhere: false,
            can_repair_now,
            stored_path: Some(redact_path(&program)),
        }),
    }
}

/// One-shot self-heal (plan §4.4, piece 1 — no update marker yet; the AUTOMATIC Windows call site
/// is compile-time gated off until piece 2, see main.rs; `repair_autostart` reaches this on every
/// platform behind the updater-lock bow-out).
pub fn heal_if_broken(app: &tauri::AppHandle) -> HealOutcome {
    let desired = match desired_path(app) {
        Ok(d) => d,
        Err(_) => return HealOutcome::Skipped("own path unresolvable"),
    };
    heal_if_broken_at(&desired)
}

/// `AppHandle`-free heal core — the surface `tests/autostart_heal.rs` (L3) drives against real OS
/// artifacts under a throwaway `$HOME`. Production reaches it only through [`heal_if_broken`].
pub fn heal_if_broken_at(desired: &Path) -> HealOutcome {
    // 1. Our own desired path must exist — a relocated-while-running process has a stale
    //    current_exe(); healing from it writes a path guaranteed dead (D14 surfaces this in the UI).
    if !desired.try_exists().unwrap_or(false) {
        return HealOutcome::Skipped("own path unresolvable");
    }
    // 2. C6: the F-010 preflight now runs on the heal path too, not just at toggle time.
    if !crate::crash_recovery::autostart_path_is_safe(desired) {
        return HealOutcome::Failed("desired path is unsafe for autostart serializers".to_string());
    }
    // 3. Only Broken proceeds. Absent/Healthy → NotNeeded; Unreadable → never write.
    match read_stored_target(Some(desired)) {
        StoredTarget::Broken { .. } => {}
        StoredTarget::Unreadable { .. } => return HealOutcome::Skipped("artifact unreadable"),
        _ => return HealOutcome::NotNeeded,
    }
    // 4. Bow out while an update transaction is live (non-blocking — the poller/next launch
    //    retries). Piece 2 adds the Windows post-install marker on top of this.
    let _updater_guard = match crate::updater::acquire_updater_lock() {
        Some(f) => f,
        None => return HealOutcome::Skipped("updater active"),
    };
    // 5. D19: serialise against set_enabled and other healers, re-read under the lock.
    let _lock = match acquire_autostart_lock() {
        Ok(f) => f,
        Err(e) => return HealOutcome::Failed(e),
    };
    let old_program = match read_stored_target(Some(desired)) {
        StoredTarget::Broken { program } => program,
        StoredTarget::Unreadable { .. } => return HealOutcome::Skipped("artifact unreadable"),
        _ => return HealOutcome::NotNeeded,
    };
    // 6. In-place patch — never a recreate (C1: recreation strips macOS KeepAlive).
    if let Err(e) = backend::heal_write(desired) {
        return HealOutcome::Failed(e);
    }
    let from = redact_path(&old_program);
    let to = redact_path(&desired.to_string_lossy());
    // §9 auditability: an unexplained startup-entry rewrite must be visible in the shipped log —
    // redacted (D24): no full user paths.
    tracing::info!(%from, %to, "autostart entry healed (stored target no longer resolved)");
    HealOutcome::Healed { from, to }
}

/// L3 test surface: the enable-time ARTIFACT write alone — the exact writer `set_enabled(true)`
/// runs inside its transaction, without arming crash recovery (integration tests must not shell
/// out to `systemctl`/`schtasks` on a runner). Quoted/escaped like every production write.
pub fn enable_entry_at(desired: &Path) -> Result<(), String> {
    let _lock = acquire_autostart_lock()?;
    backend::enable_write(desired)
}

/// L3 test surface: remove the artifact alone (idempotent), without the crash-recovery disarm.
pub fn remove_entry() -> Result<(), String> {
    let _lock = acquire_autostart_lock()?;
    backend::remove()
}

/// Explicit user toggle (replaces the plugin's enable/disable; plan §4.5). Runs the existing
/// `enable_transaction` (C8) with OUR writer closures: `prior_enabled := intent_enabled` (D16 —
/// health-aware prior would send a Broken entry down the full recreate path, stripping KeepAlive),
/// and rollback restores the EXACT prior artifact bytes, not merely "disable".
pub fn set_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let _lock = acquire_autostart_lock()?;
    if enabled {
        let desired = desired_path(app)?;
        if !desired.try_exists().unwrap_or(false) {
            return Err(
                "the running executable's path no longer exists (was the app moved?); \
                 reopen Aztec Accelerator from its new location and try again"
                    .to_string(),
            );
        }
        // F-010 preflight, exactly as the old commands.rs enable path: refuse + clean up rather
        // than serialize an unsafe path.
        if !crate::crash_recovery::autostart_path_is_safe(&desired) {
            let _ = backend::remove();
            crate::crash_recovery::disable_crash_recovery();
            return Err(
                "Executable path is unsafe for autostart (control/newline/non-UTF-8); refusing to enable."
                    .to_string(),
            );
        }
        // `prior_enabled` answers a NARROWER question than `intent_enabled`: "is there already a
        // sound entry I must not recreate?" (C1 — recreating a good macOS plist strips KeepAlive).
        // An `Absent` or unparseable artifact is NOT that, so explicit ON writes a fresh one —
        // which is the documented reset path out of a corrupt entry, and is exactly what a user
        // toggling the switch means. Snapshot rollback below still restores the prior bytes.
        let prior_enabled = match read_stored_target(None) {
            StoredTarget::Healthy { .. } | StoredTarget::Broken { .. } => {
                !backend::platform_disabled()
                    .map_err(|e| format!("cannot determine current autostart state: {e}"))?
            }
            StoredTarget::Absent | StoredTarget::Unreadable { .. } => false,
        };
        // Snapshot for exact-prior rollback (strictly stronger than the plugin's delete-on-rollback).
        let snapshot = std::cell::RefCell::new(None);
        let snap_taken = std::cell::Cell::new(false);
        crate::crash_recovery::enable_transaction(
            prior_enabled,
            || {
                *snapshot.borrow_mut() = backend::read_raw()?;
                snap_taken.set(true);
                backend::enable_write(&desired)
            },
            crate::crash_recovery::enable_crash_recovery,
            || {
                if snap_taken.get() {
                    backend::restore_raw(snapshot.borrow_mut().take())
                } else {
                    Ok(())
                }
            },
            crate::crash_recovery::disable_crash_recovery,
        )
    } else {
        // OFF: remove the launcher entry (idempotent — already-absent is success), THEN disarm
        // crash recovery, surfacing a non-confirmed disarm exactly as before (commands.rs C8).
        backend::remove()?;
        if !crate::crash_recovery::disable_crash_recovery() {
            return Err(
                "Autostart launcher disabled, but crash recovery could not be confirmed disarmed — \
                 the app may still relaunch on next login. Please retry."
                    .to_string(),
            );
        }
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — L1 (pure, every platform) + L2 (differential oracle) — plan §6
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── APP_NAME drift guard (D7: one derivation) ──

    #[test]
    fn app_name_matches_tauri_conf() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(
            conf["productName"].as_str().unwrap(),
            APP_NAME,
            "APP_NAME must track productName — it names the plist/.desktop/Run artifacts"
        );
    }

    // ── plist: parse, patch, preserve (L1) + oracle (L2) ──

    /// The exact shape auto-launch wrote, AFTER crash_recovery patched KeepAlive in — the file a
    /// heal must mutate without losing a single recovery key.
    fn plist_with_keepalive(program: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
  <key>Label</key>
  <string>Aztec Accelerator</string>
  <key>ProgramArguments</key>
  <array><string>{program}</string></array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>ThrottleInterval</key>
    <integer>5</integer>
  <key>SomeFutureKey</key>
  <string>unknown-must-survive</string>
  </dict>
</plist>"#
        )
    }

    #[test]
    fn plist_program_reads_auto_launch_format() {
        let xml = plist_with_keepalive(
            "/Applications/Aztec Accelerator.app/Contents/MacOS/aztec-accelerator",
        );
        assert_eq!(
            plist_program(xml.as_bytes()).unwrap(),
            "/Applications/Aztec Accelerator.app/Contents/MacOS/aztec-accelerator"
        );
    }

    #[test]
    fn plist_set_program_preserves_keepalive_and_unknown_keys_structurally() {
        let xml = plist_with_keepalive("/old/dead/path");
        let patched = plist_set_program(xml.as_bytes(), "/new/live/path").unwrap();
        // L2 differential oracle: re-parse with the real parser and compare STRUCTURE.
        let before = plist::Value::from_reader(std::io::Cursor::new(xml.as_bytes())).unwrap();
        let after = plist::Value::from_reader(std::io::Cursor::new(&patched)).unwrap();
        let (bd, ad) = (
            before.as_dictionary().unwrap(),
            after.as_dictionary().unwrap(),
        );
        assert_eq!(plist_program(&patched).unwrap(), "/new/live/path");
        for key in [
            "Label",
            "KeepAlive",
            "ThrottleInterval",
            "RunAtLoad",
            "SomeFutureKey",
        ] {
            assert_eq!(bd.get(key), ad.get(key), "{key} must survive the patch");
        }
        // indexmap-backed: key ORDER survives too.
        let order = |d: &plist::Dictionary| d.keys().cloned().collect::<Vec<_>>();
        assert_eq!(order(bd), order(ad));
    }

    #[test]
    fn plist_round_trips_xml_metacharacters_in_paths() {
        for hostile in [
            "/tmp/a & b/<app>/\"quoted\"/exe",
            "/tmp/it's/América/程序",
            "/tmp/]]>/x",
        ] {
            let fresh = plist_render_fresh(APP_NAME, hostile).unwrap();
            assert_eq!(
                plist_program(&fresh).unwrap(),
                hostile,
                "escaping must round-trip"
            );
            let patched =
                plist_set_program(&plist_with_keepalive("/x").into_bytes(), hostile).unwrap();
            assert_eq!(plist_program(&patched).unwrap(), hostile);
        }
    }

    #[test]
    fn plist_handles_binary_format() {
        // The plist crate reads binary transparently — a hand-rolled scanner would go Unreadable
        // forever (one of the reasons D15 reversed).
        let mut dict = plist::Dictionary::new();
        dict.insert("Label".into(), plist::Value::String(APP_NAME.into()));
        dict.insert(
            "ProgramArguments".into(),
            plist::Value::Array(vec![plist::Value::String("/bin/x".into())]),
        );
        let mut buf = Vec::new();
        plist::Value::Dictionary(dict)
            .to_writer_binary(&mut buf)
            .unwrap();
        assert_eq!(plist_program(&buf).unwrap(), "/bin/x");
    }

    #[test]
    fn plist_malformed_fails_closed() {
        assert!(plist_program(b"not a plist at all").is_err());
        assert!(plist_set_program(b"<plist></plist>", "/x").is_err());
        let no_args = r#"<?xml version="1.0"?><plist version="1.0"><dict><key>Label</key><string>x</string></dict></plist>"#;
        assert!(plist_program(no_args.as_bytes()).is_err());
        let empty_args = r#"<?xml version="1.0"?><plist version="1.0"><dict><key>ProgramArguments</key><array/></dict></plist>"#;
        assert!(plist_program(empty_args.as_bytes()).is_err());
    }

    #[test]
    fn plist_disabled_key_detected_and_cleared() {
        let xml = r#"<?xml version="1.0"?><plist version="1.0"><dict>
            <key>ProgramArguments</key><array><string>/x</string></array>
            <key>Disabled</key><true/></dict></plist>"#;
        assert!(plist_disabled(xml.as_bytes()).unwrap());
        let cleared = plist_clear_disabled(xml.as_bytes()).unwrap();
        assert!(!plist_disabled(&cleared).unwrap());
        assert!(!plist_disabled(&plist_render_fresh(APP_NAME, "/x").unwrap()).unwrap());
    }

    // ── .desktop: quote/parse/rewrite (L1) ──

    /// Byte-exact legacy fixture: what auto-launch 0.5.0 wrote (unquoted Exec, trailing space from
    /// the empty-args join, no trailing newline) — the golden fixture pinning the crate contract.
    fn legacy_desktop(path: &str) -> String {
        format!(
            "[Desktop Entry]\nType=Application\nVersion=1.0\nName={APP_NAME}\nComment={APP_NAME}startup script\nExec={path} \nStartupNotify=false\nTerminal=false"
        )
    }

    #[test]
    fn desktop_quote_round_trips_hostile_paths() {
        for hostile in [
            "/opt/aztec accelerator/bin/app",
            "/tmp/it's/a\"b/c`d/e$f/g\\h",
            "/tmp/100%/done",
            "/tmp/América/程序",
            "/tmp/-leading-dash",
        ] {
            let quoted = desktop_quote(hostile).expect("quotable");
            assert_eq!(
                desktop_exec_program(&quoted).unwrap(),
                hostile,
                "quote→parse must round-trip {hostile:?}"
            );
        }
    }

    #[test]
    fn desktop_quote_rejects_controls() {
        assert!(desktop_quote("/tmp/a\nb").is_none());
        assert!(desktop_quote("/tmp/a\x07b").is_none());
    }

    #[test]
    fn desktop_field_code_injection_is_neutralized() {
        // A path containing a field code must be written as %% and decode back to the literal —
        // never expanded into "insert the clicked URL here".
        let path = "/tmp/%U/%f/app";
        let quoted = desktop_quote(path).unwrap();
        // Every % must be doubled — a LONE % is a live field code to the launcher.
        let mut chars = quoted.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '%' {
                assert_eq!(chars.next(), Some('%'), "lone %% escape in {quoted}");
            }
        }
        assert_eq!(quoted.matches('%').count(), 2 * path.matches('%').count());
        assert_eq!(desktop_exec_program(&quoted).unwrap(), path);
    }

    #[test]
    fn desktop_legacy_unquoted_first_token_semantics() {
        // Exactly what the plugin wrote for a spaced path: the OS would exec the FIRST token, so
        // classification must too (the entry is effectively broken unless /opt/aztec exists).
        assert_eq!(
            desktop_exec_program("/opt/aztec accelerator/bin/app ").unwrap(),
            "/opt/aztec"
        );
        // Unspaced legacy value: the whole token.
        assert_eq!(
            desktop_exec_program("/usr/bin/app ").unwrap(),
            "/usr/bin/app"
        );
    }

    #[test]
    fn desktop_reader_parses_legacy_fixture() {
        let ini = legacy_desktop("/usr/bin/aztec-accelerator");
        let exec = desktop_field(&ini, "Exec").unwrap().unwrap();
        assert_eq!(
            desktop_exec_program(&exec).unwrap(),
            "/usr/bin/aztec-accelerator"
        );
        assert!(!desktop_hidden(&ini));
    }

    #[test]
    fn desktop_duplicate_exec_is_unreadable() {
        let ini = "[Desktop Entry]\nExec=/a\nExec=/b\n";
        assert!(desktop_field(ini, "Exec").is_err());
        assert!(desktop_set_exec(ini, "\"/c\"").is_err());
    }

    #[test]
    fn desktop_set_exec_rewrites_only_the_exec_line() {
        let ini = legacy_desktop("/old/dead");
        let rewritten = desktop_set_exec(&ini, "\"/new/live\"").unwrap();
        let exec = desktop_field(&rewritten, "Exec").unwrap().unwrap();
        assert_eq!(desktop_exec_program(&exec).unwrap(), "/new/live");
        // Every non-Exec line byte-identical.
        let before: Vec<&str> = ini.lines().filter(|l| !l.starts_with("Exec")).collect();
        let after: Vec<&str> = rewritten
            .lines()
            .filter(|l| !l.starts_with("Exec"))
            .collect();
        assert_eq!(before, after);
    }

    #[test]
    fn desktop_hidden_detected_and_cleared_and_exec_outside_entry_ignored() {
        let ini = "[Desktop Entry]\nExec=/a\nHidden=true\n[Other]\nExec=/decoy\n";
        assert!(desktop_hidden(ini));
        let cleared = desktop_clear_hidden(ini);
        assert!(!desktop_hidden(&cleared));
        assert!(cleared.contains("Exec=/decoy"), "other groups untouched");
        // Only the [Desktop Entry] Exec counts.
        assert_eq!(desktop_field(ini, "Exec").unwrap().unwrap(), "/a");
    }

    #[test]
    fn desktop_render_fresh_is_quoted_and_parses() {
        let quoted = desktop_quote("/opt/a b/app").unwrap();
        let ini = desktop_render_fresh(APP_NAME, &quoted);
        let exec = desktop_field(&ini, "Exec").unwrap().unwrap();
        assert_eq!(desktop_exec_program(&exec).unwrap(), "/opt/a b/app");
        assert!(!ini.contains("Exec=/"), "fresh Exec must be quoted");
    }

    // ── Windows Run value (L1) ──

    #[test]
    fn run_value_quote_is_the_security_fix() {
        // §9: today's plugin writes this UNQUOTED; quoted, CreateProcess has exactly one candidate.
        let installed = r"C:\Users\u\AppData\Local\Aztec Accelerator\Aztec Accelerator.exe";
        let quoted = run_value_quote(installed).unwrap();
        assert_eq!(quoted, format!("\"{installed}\""));
        assert_eq!(
            run_value_candidates(&quoted).unwrap(),
            vec![installed.to_string()]
        );
    }

    #[test]
    fn run_value_quote_rejects_unrepresentable() {
        assert!(run_value_quote("C:\\a\"b\\x.exe").is_none());
        assert!(run_value_quote("C:\\a\nb\\x.exe").is_none());
    }

    #[test]
    fn run_value_candidates_model_createprocess_order() {
        // The documented prefix walk for the exact value the plugin writes today (trailing space
        // included), with .exe appended to extensionless prefixes — the §9 hijack is candidate #1.
        let legacy = r"C:\Users\u\AppData\Local\Aztec Accelerator\Aztec Accelerator.exe ";
        assert_eq!(
            run_value_candidates(legacy).unwrap(),
            vec![
                r"C:\Users\u\AppData\Local\Aztec.exe".to_string(),
                r"C:\Users\u\AppData\Local\Aztec Accelerator\Aztec.exe".to_string(),
                r"C:\Users\u\AppData\Local\Aztec Accelerator\Aztec Accelerator.exe".to_string(),
            ]
        );
        // Extension-bearing components are NOT double-suffixed — the append rule uses the
        // last-dot heuristic per component ("a.exe b" carries a dot, so it stays as-is).
        assert_eq!(
            run_value_candidates(r"C:\a.exe b").unwrap(),
            vec![r"C:\a.exe".to_string(), r"C:\a.exe b".to_string()]
        );
    }

    #[test]
    fn run_value_trailing_arguments_are_not_ours() {
        assert!(run_value_candidates("\"C:\\app.exe\" --flag").is_err());
        assert!(run_value_candidates("\"C:\\app.exe").is_err());
        assert!(run_value_candidates("   ").is_err());
    }

    #[test]
    fn resolve_first_picks_in_os_order() {
        let candidates: Vec<String> = ["C:\\Aztec.exe", "C:\\Aztec Accelerator\\app.exe"]
            .map(String::from)
            .into();
        let hijack_present = |c: &str| c == "C:\\Aztec.exe";
        assert_eq!(
            resolve_first(&candidates, &hijack_present).unwrap(),
            "C:\\Aztec.exe",
            "the model must reproduce the hijack, or the L1 security assertion is vacuous"
        );
        let only_real = |c: &str| c.ends_with("app.exe");
        assert_eq!(
            resolve_first(&candidates, &only_real).unwrap(),
            "C:\\Aztec Accelerator\\app.exe"
        );
        assert_eq!(resolve_first(&candidates, &|_: &str| false), None);
    }

    #[test]
    fn desktop_quoted_exec_with_trailing_args_is_unreadable() {
        // A user's `Exec="/opt/app" --minimized` must fail closed: the heal rewrites the whole
        // Exec line, so classifying it as merely Broken would silently delete the flag. Matches
        // the Windows reader's rule for the same shape.
        assert!(desktop_exec_program("\"/opt/app\" --minimized").is_err());
        assert!(desktop_exec_program("\"/opt/app\"  extra").is_err());
        // A quoted program with trailing WHITESPACE only is still ours.
        assert_eq!(desktop_exec_program("\"/opt/app\"  ").unwrap(), "/opt/app");
    }

    #[test]
    fn startup_approved_zeroed_timestamp_still_reads_disabled() {
        // The regression the flag-byte rule exists for: auto-launch's "last 8 bytes all zero"
        // heuristic reads this as ENABLED, which would rearm crash recovery against an explicit
        // administrator OFF (imaging/GPO tooling writes exactly this shape).
        let disabled_zeroed = [0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(!startup_approved_blob_enabled(Some(&disabled_zeroed)));
        // And the enabled flag with a non-zero tail is still enabled.
        let enabled_with_tail = [
            0x02, 0, 0, 0, 0x9a, 0xde, 0x9f, 0x3e, 0x9c, 0x5c, 0xd9, 0x01,
        ];
        assert!(startup_approved_blob_enabled(Some(&enabled_with_tail)));
    }

    #[test]
    fn startup_approved_blob_rules_match_auto_launch() {
        assert!(startup_approved_blob_enabled(None), "absent ⇒ enabled");
        assert!(
            startup_approved_blob_enabled(Some(&[0x02; 4])),
            "short ⇒ enabled"
        );
        let enabled = [0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(startup_approved_blob_enabled(Some(&enabled)));
        let disabled = [
            0x03, 0, 0, 0, 0x9a, 0xde, 0x9f, 0x3e, 0x9c, 0x5c, 0xd9, 0x01,
        ];
        assert!(
            !startup_approved_blob_enabled(Some(&disabled)),
            "timestamped blob ⇒ Task-Manager OFF"
        );
    }

    // ── Classification state table (L1) — real files in a tempdir ──

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn classify_state_table() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("live-exe");
        std::fs::write(&live, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        make_executable(&live);

        // absent target ⇒ Broken
        let gone = dir.path().join("nope");
        assert!(matches!(
            classify_program(gone.to_string_lossy().into_owned(), Some(&live)),
            StoredTarget::Broken { .. }
        ));

        // resolving target == desired ⇒ Healthy, not elsewhere
        match classify_program(live.to_string_lossy().into_owned(), Some(&live)) {
            StoredTarget::Healthy {
                points_elsewhere, ..
            } => assert!(!points_elsewhere),
            other => panic!("expected Healthy, got {other:?}"),
        }

        // resolving target ≠ desired ⇒ Healthy{points_elsewhere} — NEVER auto-repointed (D1)
        let other_exe = dir.path().join("other-exe");
        std::fs::write(&other_exe, b"x").unwrap();
        #[cfg(unix)]
        make_executable(&other_exe);
        match classify_program(other_exe.to_string_lossy().into_owned(), Some(&live)) {
            StoredTarget::Healthy {
                points_elsewhere, ..
            } => assert!(points_elsewhere),
            other => panic!("expected Healthy elsewhere, got {other:?}"),
        }

        // directory ⇒ Broken
        assert!(matches!(
            classify_program(dir.path().to_string_lossy().into_owned(), Some(&live)),
            StoredTarget::Broken { .. }
        ));

        // desired unresolvable (relocated-while-running): classification still works
        match classify_program(live.to_string_lossy().into_owned(), None) {
            StoredTarget::Healthy {
                points_elsewhere, ..
            } => assert!(!points_elsewhere),
            other => panic!("expected Healthy, got {other:?}"),
        }

        #[cfg(unix)]
        {
            // non-executable file ⇒ Broken
            let plain = dir.path().join("plain");
            std::fs::write(&plain, b"data").unwrap();
            std::fs::set_permissions(&plain, {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::Permissions::from_mode(0o644)
            })
            .unwrap();
            assert!(matches!(
                classify_program(plain.to_string_lossy().into_owned(), Some(&live)),
                StoredTarget::Broken { .. }
            ));

            // valid symlink ⇒ Healthy (metadata follows), canonicalized equal ⇒ not elsewhere (C7)
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&live, &link).unwrap();
            match classify_program(link.to_string_lossy().into_owned(), Some(&live)) {
                StoredTarget::Healthy {
                    points_elsewhere, ..
                } => {
                    assert!(
                        !points_elsewhere,
                        "symlink to desired must not read as elsewhere"
                    )
                }
                other => panic!("expected Healthy, got {other:?}"),
            }

            // dangling symlink ⇒ Broken
            let dangling = dir.path().join("dangling");
            std::os::unix::fs::symlink(dir.path().join("void"), &dangling).unwrap();
            assert!(matches!(
                classify_program(dangling.to_string_lossy().into_owned(), Some(&live)),
                StoredTarget::Broken { .. }
            ));
        }
    }

    // ── Redaction (D24) ──

    #[test]
    fn redact_path_keeps_two_components() {
        assert_eq!(
            redact_path("/Users/alice/Applications/Aztec Accelerator.app"),
            "…/Applications/Aztec Accelerator.app"
        );
        assert_eq!(
            redact_path(r"C:\Users\alice\AppData\Local\Aztec Accelerator\Aztec Accelerator.exe"),
            r"…\Aztec Accelerator\Aztec Accelerator.exe"
        );
        assert_eq!(redact_path("app"), "…/app");
        assert_eq!(redact_path(""), "…");
        assert!(
            !redact_path("/Users/alice/bin/app").contains("alice"),
            "usernames must not survive redaction"
        );
    }
}
