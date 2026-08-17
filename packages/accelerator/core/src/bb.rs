use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::versions;

/// Maximum time to wait for bb prove to complete before killing the process.
const PROVE_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

/// B3 (F4): cap on bb stderr RETAINED for the log line. The reader keeps draining past this to EOF (so
/// bb can never block on a full pipe buffer), but stops ACCUMULATING — the earlier `wait_with_output()`
/// buffered all of stderr into memory for up to `PROVE_TIMEOUT`, an unbounded allocation an authentic
/// chatty (or pathological) bb could drive. 64 KiB is far more than any real diagnostic.
const STDERR_RETAIN_CAP: usize = 64 * 1024;

/// B3 (F5): reject a bb proof file larger than this before reading it into memory. A real chonk proof is
/// a few tens of KiB; this generous ceiling bounds the read so a wrong/corrupt/huge `proof` file can't be
/// slurped whole (`std::fs::read` is unbounded) into a memory-exhaustion.
const MAX_PROOF_BYTES: u64 = 64 * 1024 * 1024;

/// Find the `bb` binary. When `version` is provided, the marker-verified version cache is the ONLY
/// acceptable source — never the standard search chain.
///
/// Search order:
/// 0. `BB_BINARY_PATH` env var — trusted, unversioned operator override (A4). The one documented
///    exception to "no unverified execution": whoever sets the process env already owns the process.
/// 1. Version cache (`~/.aztec-accelerator/versions/{version}/bb`) — when a version is requested.
/// 2. Bundled sidecar (Tauri externalBin) — `binaries/bb-{target-triple}` next to the executable
/// 3. `~/.bb/bb` — user-installed via `bbup`
/// 4. `bb` on `$PATH`
///
/// F-007: for a REQUESTED (non-bundled) version, the marker-verified cache entry is the ONLY acceptable
/// source. `resolve_version` normalizes the bundled request to `None`, so a `Some(v)` here is always a
/// genuinely non-bundled request; ANY cache failure (absent, tampered, unreadable) is a hard error —
/// steps 2–4 would silently execute the WRONG version (or an unverified binary) over the private
/// witness. Only `find_bb(None)` (bundled / unspecified) walks the sidecar → `~/.bb` → `$PATH` chain.
/// q7e3-F-08: `version` is the validated `&AztecVersion` — the cache-path lookup is traversal-safe.
pub fn find_bb(version: Option<&versions::AztecVersion>) -> Result<PathBuf, String> {
    // 0. Explicit override via environment variable (trusted, unversioned — A4).
    if let Ok(path) = std::env::var("BB_BINARY_PATH") {
        let explicit = PathBuf::from(&path);
        if explicit.exists() {
            return Ok(explicit);
        }
    }

    // 1. Version cache — a requested non-bundled version MUST resolve to a marker-verified entry, with
    //    NO fall-through to a different bb (F-007).
    if let Some(v) = version {
        return versions::verify_cached_bb(v)
            .map_err(|e| format!("cached bb for {v} failed integrity verification: {e}"));
    }

    // 2. Sidecar: check next to the current executable (bb.exe on Windows)
    if let Ok(exe) = std::env::current_exe() {
        let sidecar = exe
            .parent()
            .unwrap_or(&exe)
            .join(versions::bb_binary_name());
        if sidecar.exists() {
            return Ok(sidecar);
        }
    }

    // 3. ~/.bb/bb (bbup install location)
    if let Some(home) = dirs::home_dir().or_else(home_dir_fallback) {
        let bbup_path = home.join(".bb").join(versions::bb_binary_name());
        if bbup_path.exists() {
            return Ok(bbup_path);
        }
    }

    // 4. bb on $PATH — Unix only. On Windows we deliberately skip a bare PATH lookup:
    //    which() there resolves via PATH+PATHEXT, so a planted bb.exe/bb.bat/bb.cmd in
    //    CWD or a writable PATH dir could hijack proving. The bundled sidecar (step 2) is
    //    always present in shipped builds; for Windows dev, set BB_BINARY_PATH explicitly.
    #[cfg(not(target_os = "windows"))]
    if let Ok(path) = which::which("bb") {
        return Ok(path);
    }

    Err("bb binary not found. Install via bbup or bundle as sidecar.".to_string())
}

fn home_dir_fallback() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Per-user private base for prove workspaces: `<data-local>/aztec-accelerator/prove-tmp`, created
/// owner-only. Using our OWN per-user directory (not the shared OS temp) keeps the witness off a
/// world-readable / shared `$TMPDIR`/`%TEMP%` and out of a non-sticky temp parent where an
/// attacker could replace an ancestor between creation and use (F-003 hardening). `None` if no
/// data-local dir is resolvable (caller falls back to OS temp).
fn prove_tmp_parent() -> Option<PathBuf> {
    let base = dirs::data_local_dir()?
        .join("aztec-accelerator")
        .join("prove-tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&base)
            .ok()?;
        // Tighten even if it pre-existed with a looser mode.
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).ok()?;
    }
    #[cfg(windows)]
    {
        // F-003 Windows tail: create `prove-tmp` with an owner-only PROTECTED+inheritable DACL, or harden
        // it if it pre-exists. Fail closed (None → caller must NOT fall back to a shared temp on Windows).
        if let Some(mid) = base.parent() {
            std::fs::create_dir_all(mid).ok()?;
        }
        match crate::win_acl::secure_create_dir(&base) {
            Ok(()) => {}
            Err(_) if base.is_dir() => crate::win_acl::harden_existing_dir(&base).ok()?,
            Err(_) => return None,
        }
    }
    #[cfg(all(not(unix), not(windows)))]
    std::fs::create_dir_all(&base).ok()?;
    Some(base)
}

/// How long an abandoned prove workspace must sit untouched before the startup sweep removes it
/// (F-08a, audit 2026-07-31-9c4cb0c).
///
/// **The bind win is the guard; this floor is a belt.** The sweep runs only in the instance that won
/// `:59833` (see the call site in `server.rs`), and both binaries reach proving through that same
/// bind, so a live workspace belongs to a process that by definition does not exist. The floor only
/// covers the seam where [`crate::server::bind_with_retry`] waits out a predecessor for up to 5 s and
/// could win the port while that predecessor is still finishing an in-flight proof.
///
/// 24 hours, not a tight bound derived from `PROVE_TIMEOUT`: the workspace is created *before* the
/// timed region (`create_prove_tempdir` above) and read *after* it, so it outlives `PROVE_TIMEOUT` on
/// both ends and a floor set at 300 s would delete live directories. Since the harm being fixed is
/// residue accumulating over months, there is no value in reaping aggressively — a floor ~240× the
/// longest possible proof puts the race out of reach instead of arguing it away.
///
/// A per-workspace advisory lock (`flock`/`LockFileEx`) held for the workspace's lifetime is the
/// correct-by-construction alternative. Deliberately not built: it is disproportionate to a Med/Low
/// disk-hygiene finding. It is the upgrade path if same-session reaping is ever needed.
const PROVE_RESIDUE_FLOOR: Duration = Duration::from_secs(24 * 60 * 60);

/// Remove abandoned prove workspaces left by a process that died before `TempDir`'s `Drop` could run
/// — a crash, a Quit mid-proof, or an auto-update restart. Returns how many were removed.
///
/// Call ONLY after winning the `:59833` bind. Sweeps the private `prove-tmp` parent and nothing else:
/// `create_prove_tempdir` falls back to the OS temp dir on non-Windows when no data-local dir
/// resolves, and prefix-matching `prove-*` in a shared `/tmp` could delete a stranger's directory.
/// That residue is deliberately left unreaped.
pub fn reap_orphaned_prove_workspaces() -> usize {
    let Some(parent) = prove_tmp_parent() else {
        return 0;
    };
    reap_prove_dirs_older_than(&parent, PROVE_RESIDUE_FLOOR)
}

/// The sweep itself, with the parent and floor injected so it is testable without touching the real
/// per-user directory.
///
/// Never follows symlinks (`symlink_metadata`), never recurses above `parent`, and matches only our
/// own `prove-` prefix — so a symlink planted in `prove-tmp` cannot turn this into an arbitrary-delete
/// primitive. A single unreadable or undeletable entry is logged and skipped, never fatal: this runs
/// on the startup path and must not be able to stop the server coming up.
fn reap_prove_dirs_older_than(parent: &Path, floor: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("prove-"))
        {
            continue;
        }
        // symlink_metadata, NOT metadata: a symlink here must be judged as a symlink (and skipped),
        // never followed to its target's mtime and then removed.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let aged = meta
            .modified()
            .ok()
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age >= floor);
        if !aged {
            continue;
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => {
                removed += 1;
                tracing::info!(dir = %path.display(), "reaped an abandoned prove workspace");
            }
            Err(e) => tracing::warn!(dir = %path.display(), "could not reap prove workspace: {e}"),
        }
    }
    removed
}

/// Create the per-prove temp workspace under the per-user private base (see `prove_tmp_parent`).
/// On Unix the directory is created `0o700` (owner-only) at the creation syscall — never
/// write-then-chmod — so the private witness never has a world-traversable window (F-003).
/// `tempfile::tempdir()` alone applies no mode and inherits the umask default (typically `0o755`).
/// Falls back to the OS temp dir (still `0o700` on Unix) only if no per-user dir is resolvable.
fn create_prove_tempdir() -> std::io::Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("prove-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(windows)]
    {
        // F-003 (D4/D21): fail closed on Windows — NO OS-`%TEMP%` fallback for the private witness. The
        // `prove-tmp` parent is owner-only + inheritable, so the child tempdir inherits owner-only AT
        // creation (no window); harden it explicitly (PROTECTED) too.
        let parent = prove_tmp_parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no per-user data dir for a private prove workspace (refusing OS-temp fallback)",
            )
        })?;
        let dir = builder.tempdir_in(parent)?;
        crate::win_acl::harden_existing_dir(dir.path())?;
        Ok(dir)
    }
    #[cfg(not(windows))]
    match prove_tmp_parent() {
        Some(parent) => builder.tempdir_in(parent),
        None => {
            tracing::warn!(
                "No per-user data dir for a private prove workspace; using OS temp (0o700 on Unix)"
            );
            builder.tempdir()
        }
    }
}

/// Write the proving witness (private ZK inputs) with mode `0o600` supplied to the creation
/// syscall (F-003) — no write-then-chmod window. `create_new(true)` fails closed if the path
/// already exists (defends against a pre-planted file/symlink in the workspace).
fn write_witness(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    #[cfg(windows)]
    {
        // F-003 Windows tail: create the empty witness with an owner-only DACL BEFORE writing bytes
        // (CREATE_NEW rejects a pre-planted file/symlink), then write.
        let mut file = crate::win_acl::secure_create_file(path)?;
        file.write_all(bytes)
    }
    #[cfg(not(windows))]
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        file.write_all(bytes)
    }
}

/// Run `bb prove` on the given IVC inputs (msgpack bytes) and return the proof
/// with a 4-byte BE field-count header suitable for `ChonkProofWithPublicInputs.fromBuffer()`.
///
/// When `version` is specified, searches the version cache for the matching `bb` binary.
/// When `threads` is specified, passes `-t N` to limit parallelism.
pub async fn prove(
    ivc_inputs: &[u8],
    version: Option<&versions::AztecVersion>,
    threads: Option<usize>,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    prove_with_timeout(ivc_inputs, version, threads, PROVE_TIMEOUT).await
}

/// The body of [`prove`], with the timeout injected so tests can drive the timeout/kill path without a
/// 5-minute wait (the same externalized-`Duration` shape as `bind_with_retry_inner`).
async fn prove_with_timeout(
    ivc_inputs: &[u8],
    version: Option<&versions::AztecVersion>,
    threads: Option<usize>,
    timeout: Duration,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    // Take the lease BEFORE resolving the path, and hold it for this whole function — the window
    // being closed is between "cleanup decided this version was evictable" and "we executed it".
    // `_lease` is bound (not `_`) so it lives to the end of the scope rather than dropping instantly.
    // The server leases before it waits for the prove permit; this second lease covers direct callers
    // of `prove` and costs nothing (the registry is refcounted, so nesting is fine). `None` means a
    // cleanup is deleting this version right now — fail rather than execute a binary being unlinked.
    let _lease = match version {
        Some(v) => match versions::acquire_lease(v.as_str()) {
            Some(l) => Some(l),
            None => {
                return Err(format!("bb {v}: the cached version is being evicted").into());
            }
        },
        None => None,
    };
    let bb_path =
        find_bb(version).map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;

    let tmp_dir = create_prove_tempdir()?;
    let input_path = tmp_dir.path().join("ivc-inputs.msgpack");
    let output_dir = tmp_dir.path().join("output");
    std::fs::create_dir_all(&output_dir)?;
    write_witness(&input_path, ivc_inputs)?;

    tracing::info!(
        version = version.map_or("bundled", |v| v.as_str()),
        ?threads,
        "Starting bb prove"
    );

    let mut cmd = tokio::process::Command::new(&bb_path);
    cmd.args([
        "prove",
        "--scheme",
        "chonk",
        "--ivc_inputs_path",
        input_path
            .to_str()
            .ok_or("temp input path contains non-UTF-8 characters")?,
        "-o",
        output_dir
            .to_str()
            .ok_or("temp output path contains non-UTF-8 characters")?,
    ]);
    if let Some(t) = threads {
        // bb uses HARDWARE_CONCURRENCY env var to control thread count.
        // The -t flag was repurposed to --verifier_target in recent versions.
        cmd.env("HARDWARE_CONCURRENCY", t.to_string());
    }
    // kill_on_drop ensures the bb process is killed if the future is cancelled
    // (e.g., client disconnect, timeout). Without it, an orphaned bb would run to
    // completion wasting CPU while holding the prove semaphore.
    cmd.kill_on_drop(true);
    let mut child = spawn_capturing_stderr(&mut cmd)?;
    // B3 (F4): take the piped stderr and drain it CAP-and-CONTINUE, concurrently with waiting for exit.
    // `wait_with_output()` used to buffer ALL of stderr into memory for up to the timeout; draining with
    // a retain cap bounds the allocation while still emptying the pipe so bb never blocks writing to it.
    let stderr_pipe = child
        .stderr
        .take()
        .expect("spawn_capturing_stderr pipes stderr");
    let waited = tokio::time::timeout(timeout, async {
        let (drained, status) =
            tokio::join!(drain_capped(stderr_pipe, STDERR_RETAIN_CAP), child.wait());
        status.map(|s| (s, drained.0, drained.1))
    })
    .await;

    let (status, stderr_retained, stderr_total) = match waited {
        Ok(Ok(triple)) => triple,
        Ok(Err(e)) => return Err(Box::new(e)),
        Err(_) => {
            // Timeout: the outer future is being dropped, so `kill_on_drop` fires on `child`.
            tracing::error!("bb prove timed out after {:?}", timeout);
            return Err("bb prove timed out after 5 minutes".into());
        }
    };

    let stderr = String::from_utf8_lossy(&stderr_retained);
    if !stderr.is_empty() {
        tracing::warn!(
            stderr_total_bytes = stderr_total,
            "bb stderr:\n{}",
            truncate_stderr(&stderr)
        );
    }

    if !status.success() {
        // Log full stderr server-side, but return only a generic error to HTTP clients
        // to avoid leaking bb internals (file paths, witness data) to the browser.
        tracing::error!(exit_code = %status, "bb prove failed");
        return Err(format!("bb prove failed (exit {status})").into());
    }

    // B3 (F5): validate the proof output before trusting it. Exit-code success was the ONLY gate, so an
    // empty or truncated `proof` file produced a "successful" response (a 0-byte proof → a 4-byte
    // header-only body; a non-32-aligned file → silently floor-divided). Validate the size on disk
    // BEFORE reading (the read is otherwise unbounded); the file is in our own temp dir and bb has
    // exited, so the on-disk length equals what we then read.
    let proof_path = output_dir.join("proof");
    let proof_len = std::fs::metadata(&proof_path)?.len();
    validate_proof_len(proof_len)?;
    let raw_proof = std::fs::read(&proof_path)?;

    tracing::debug!(proof_bytes = raw_proof.len(), "bb prove completed");

    Ok(prepend_field_count_header(&raw_proof))
}

/// B3 (F5): a bb proof file must be non-empty, within [`MAX_PROOF_BYTES`], and a whole number of 32-byte
/// field elements. All three checks are on the byte length alone, so they run against the on-disk size
/// before the file is read into memory. Pure, so every rejection is unit-testable.
fn validate_proof_len(len: u64) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if len == 0 {
        return Err("bb reported success but produced an empty proof file".into());
    }
    if len > MAX_PROOF_BYTES {
        return Err(format!(
            "bb proof file is {len} bytes, exceeding the {MAX_PROOF_BYTES}-byte cap"
        )
        .into());
    }
    if len % 32 != 0 {
        return Err(
            format!("bb proof is not a whole number of 32-byte fields ({len} bytes)").into(),
        );
    }
    Ok(())
}

/// B3 (F4): read `reader` to EOF, RETAINING at most `cap` bytes but continuing to drain the rest, and
/// return `(retained, total_bytes_seen)`. Cap-and-continue (not fail-closed): stderr volume is a
/// diagnostic, not an integrity signal, so aborting a valid proof over a chatty bb would be a
/// self-inflicted DoS — but we must keep emptying the pipe so bb never blocks on a full buffer.
async fn drain_capped<R>(mut reader: R, cap: usize) -> (Vec<u8>, u64)
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut retained: Vec<u8> = Vec::new();
    let mut total: u64 = 0;
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break, // EOF
            Ok(n) => {
                total = total.saturating_add(n as u64);
                if retained.len() < cap {
                    let room = cap - retained.len();
                    retained.extend_from_slice(&buf[..room.min(n)]);
                }
            }
            Err(_) => break, // read error — stop draining, return what we have
        }
    }
    (retained, total)
}

/// Prepend a 4-byte big-endian uint32 field count header.
/// Each field is 32 bytes, so field_count = raw_len / 32.
fn prepend_field_count_header(raw_proof: &[u8]) -> Vec<u8> {
    let field_count = (raw_proof.len() / 32) as u32;
    let mut result = Vec::with_capacity(4 + raw_proof.len());
    result.extend_from_slice(&field_count.to_be_bytes());
    result.extend_from_slice(raw_proof);
    result
}

/// Spawn a child with its stderr CAPTURED rather than inherited (F-08b, audit 2026-07-31).
///
/// `std::process::Command` — and therefore `tokio`'s — defaults every stdio handle to
/// `Stdio::inherit()`. With stderr inherited, `wait_with_output()` returns an **always-empty**
/// `Output::stderr` (tokio takes `self.stderr`, which is `None`), so the `if !stderr.is_empty()`
/// guard below never fired: the 500-char truncation and the "log server-side only" containment it
/// implements were dead code that had never executed once. Meanwhile `bb`'s real diagnostics — which
/// the surrounding comments describe as potentially carrying workspace paths and witness-derived
/// material — went straight to whatever stream this process inherited, bypassing both the truncation
/// and the `0700` rolling log directory.
///
/// Piping it is what makes the existing control real. `wait_with_output()` drains the pipe
/// concurrently with the wait, so there is no fill-the-pipe-buffer deadlock. stdout is deliberately
/// left inherited: it carries no diagnostics we redact, and changing it is not this fix's business.
///
/// Configuration and spawn are ONE function on purpose (round 2, codex pass over F-08b). As two, the
/// first version was a config helper that production had to remember to call — the same shape as the
/// bug itself, where the setting was simply never applied. A test that drove the helper directly
/// would have kept passing if the call site were dropped. There is now no call site to drop.
fn spawn_capturing_stderr(
    cmd: &mut tokio::process::Command,
) -> std::io::Result<tokio::process::Child> {
    cmd.stderr(std::process::Stdio::piped());
    cmd.spawn()
}

/// Truncate `bb` stderr for logging, cutting at 500 CHARACTERS (not bytes). `from_utf8_lossy` yields
/// valid UTF-8, but a multibyte codepoint straddling byte 500 would panic a byte slice (`&s[..500]`);
/// char-truncation is panic-safe. Only labels `[truncated]` when it actually cut (a sub-500-char but
/// >500-byte string is left whole, not mislabeled).
fn truncate_stderr(stderr: &str) -> String {
    let char_count = stderr.chars().count();
    if char_count > 500 {
        let head: String = stderr.chars().take(500).collect();
        format!("{head}... [truncated, {char_count} chars total]")
    } else {
        stderr.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// F-003: the per-prove workspace dir is created `0o700` and the witness file `0o600` — at
    /// the creation syscall, so no other local user can read the private witness while proving.
    #[cfg(unix)]
    #[test]
    fn prove_workspace_and_witness_have_private_modes() {
        use std::os::unix::fs::MetadataExt;

        let dir = create_prove_tempdir().unwrap();
        let witness = dir.path().join("ivc-inputs.msgpack");
        write_witness(&witness, b"secret-witness-bytes").unwrap();

        assert_eq!(
            std::fs::metadata(dir.path()).unwrap().mode() & 0o777,
            0o700,
            "prove workspace dir must be owner-only"
        );
        assert_eq!(
            std::fs::metadata(&witness).unwrap().mode() & 0o777,
            0o600,
            "witness file must be owner-only"
        );

        // create_new fails closed on a pre-existing path (no silent overwrite of a planted file).
        assert!(write_witness(&witness, b"again").is_err());
    }

    /// F-003 Windows tail (runs in the `windows-build` CI lane). `create_prove_tempdir` / `write_witness`
    /// apply an owner-only PROTECTED DACL and then READ IT BACK, failing closed if it did not take effect
    /// (`win_acl::verify_owner_only` — catches FAT/exFAT no-op, foreign/world ACEs). So a successful
    /// create+write IS the effective-DACL assertion: owner-only, no `BUILTIN\Users`/`Everyone`. Also
    /// pins reparse/pre-plant rejection (CREATE_NEW / CreateDirectoryW fail if the path already exists).
    #[cfg(windows)]
    #[test]
    fn prove_workspace_and_witness_are_owner_only_windows() {
        let dir =
            create_prove_tempdir().expect("secure prove workspace (owner-only DACL verified)");
        let witness = dir.path().join("ivc-inputs.msgpack");
        write_witness(&witness, b"secret-witness-bytes")
            .expect("secure witness (owner-only DACL verified)");
        assert!(witness.exists());
        // CREATE_NEW must reject a second write to the same path (planted-file / symlink defense).
        assert!(write_witness(&witness, b"again").is_err());
    }

    /// F-08b regression. Pre-fix the child inherited stderr, so `Output::stderr` was unconditionally
    /// empty, `truncate_stderr` was unreachable from production, and `bb`'s output escaped the app's
    /// `0700` log directory entirely. Drives the REAL spawn helper — the same one `prove` uses, so
    /// there is no separate call site that could be removed while this still passed — against a
    /// stand-in child that writes to stderr.
    #[tokio::test]
    async fn child_stderr_is_captured_so_the_truncation_control_can_run() {
        #[cfg(unix)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("/bin/sh");
            c.args(["-c", "echo bb-diagnostic-line >&2"]);
            c
        };
        #[cfg(windows)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("cmd");
            c.args(["/C", "echo bb-diagnostic-line 1>&2"]);
            c
        };

        let output = spawn_capturing_stderr(&mut cmd)
            .unwrap()
            .wait_with_output()
            .await
            .unwrap();

        let captured = String::from_utf8_lossy(&output.stderr);
        assert!(
            captured.contains("bb-diagnostic-line"),
            "stderr was not captured — the truncation/containment path is dead again: {captured:?}"
        );
        // And the captured text is what the (previously unreachable) control operates on.
        assert_eq!(truncate_stderr(captured.trim()), "bb-diagnostic-line");
    }

    #[test]
    fn truncate_stderr_cuts_at_char_boundary_without_panic() {
        // 600 'é' = 1200 bytes / 600 chars → must truncate (char_count > 500); a byte slice at 500
        // would split the 2-byte codepoint and panic.
        let multibyte = "é".repeat(600);
        let out = truncate_stderr(&multibyte);
        assert!(out.contains("[truncated, 600 chars total]"), "got: {out}");
        assert!(out.starts_with(&"é".repeat(500)));

        // 300 emoji = 1200 bytes but only 300 chars → must NOT be labeled truncated.
        let emoji = "😀".repeat(300);
        let out = truncate_stderr(&emoji);
        assert!(
            !out.contains("[truncated"),
            "short-char/long-byte must not truncate: {out}"
        );
        assert_eq!(out, emoji);

        // Exactly 500 chars → boundary, not truncated.
        let exact = "x".repeat(500);
        assert_eq!(truncate_stderr(&exact), exact);
    }

    #[test]
    fn test_prepend_field_count_header() {
        // 64 bytes = 2 fields of 32 bytes each
        let raw = vec![0xAB; 64];
        let result = prepend_field_count_header(&raw);

        assert_eq!(result.len(), 68); // 4 header + 64 data
        assert_eq!(&result[0..4], &[0, 0, 0, 2]); // 2 fields, big-endian
        assert_eq!(&result[4..], &raw[..]);
    }

    #[test]
    fn test_prepend_field_count_header_empty() {
        let raw = vec![];
        let result = prepend_field_count_header(&raw);

        assert_eq!(result.len(), 4);
        assert_eq!(&result[0..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn test_prepend_field_count_header_single_field() {
        let raw = vec![0xFF; 32];
        let result = prepend_field_count_header(&raw);

        assert_eq!(result.len(), 36);
        assert_eq!(&result[0..4], &[0, 0, 0, 1]); // 1 field
    }

    #[test]
    #[serial]
    fn test_find_bb_respects_bb_binary_path_env() {
        // Set BB_BINARY_PATH to the current executable (guaranteed to exist)
        let exe = std::env::current_exe().unwrap();
        std::env::set_var("BB_BINARY_PATH", exe.to_str().unwrap());

        let result = find_bb(None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), exe);

        // Clean up
        std::env::remove_var("BB_BINARY_PATH");
    }

    #[test]
    #[serial]
    fn test_find_bb_ignores_nonexistent_bb_binary_path() {
        std::env::set_var("BB_BINARY_PATH", "/nonexistent/path/to/bb");
        // Should not return the nonexistent path — falls through to other checks
        let result = find_bb(None);
        if let Ok(path) = result {
            assert_ne!(path, PathBuf::from("/nonexistent/path/to/bb"));
        }
        // Err is also fine — bb not found via other methods
        std::env::remove_var("BB_BINARY_PATH");
    }

    #[test]
    fn test_find_bb_resolution_priority() {
        // This test verifies find_bb returns an error when no bb is available,
        // which is the expected state in CI/test environments.
        // When bb IS available (via PATH or ~/.bb/bb), it should succeed.
        let result = find_bb(None);
        // We can't assert Ok/Err since it depends on the environment,
        // but we can verify the function doesn't panic.
        match result {
            Ok(path) => assert!(path.exists()),
            Err(msg) => assert!(msg.contains("bb binary not found")),
        }
    }

    #[test]
    #[serial]
    fn test_find_bb_with_uncached_version_is_fail_closed() {
        // F-007: a requested (non-bundled) version with no marker-verified cache entry MUST hard-error —
        // it never falls through to the sidecar/~/.bb/$PATH (which would run the wrong/unverified bb over
        // the witness). `#[serial]` + clearing the env override keeps this deterministic vs the env tests.
        std::env::remove_var("BB_BINARY_PATH");
        let version = versions::AztecVersion::parse("99.99.99-nonexistent").unwrap();
        let result = find_bb(Some(&version));
        assert!(
            result.is_err(),
            "uncached requested version must fail closed, got {result:?}"
        );
        assert!(result.unwrap_err().contains("integrity verification"));
    }

    // ── F-08a: witness residue (audit 2026-07-31-9c4cb0c) ──
    //
    // A prove workspace holds the private witness and is deleted only by `TempDir`'s `Drop`, which a
    // crash / Quit mid-proof / auto-update restart skips. Nothing swept them at startup.

    /// Backdate a directory's mtime so it reads as abandoned. Same technique as
    /// `reap_stale_stages_spares_recently_active_stages` in `versions/downloader.rs`.
    fn age(path: &Path, by: Duration) {
        let when = std::time::SystemTime::now() - by;
        filetime::set_file_mtime(path, filetime::FileTime::from_system_time(when)).unwrap();
    }

    #[test]
    fn reaps_abandoned_workspaces_but_spares_a_live_one() {
        let parent = tempfile::tempdir().unwrap();
        let floor = Duration::from_secs(3600);

        let abandoned = parent.path().join("prove-deadBEEF");
        std::fs::create_dir_all(&abandoned).unwrap();
        std::fs::write(abandoned.join("ivc-inputs.msgpack"), b"witness").unwrap();
        age(&abandoned, floor + Duration::from_secs(60));

        // A workspace younger than the floor. This is the case that matters: the headless server
        // shares this directory, and `bind_with_retry` can win the port from a predecessor that is
        // still finishing a proof.
        let live = parent.path().join("prove-liveFACE");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(live.join("ivc-inputs.msgpack"), b"in flight").unwrap();

        assert_eq!(reap_prove_dirs_older_than(parent.path(), floor), 1);
        assert!(!abandoned.exists(), "an abandoned workspace must be reaped");
        assert!(
            live.exists(),
            "a workspace younger than the floor must be spared — this is the deletion of a LIVE \
             witness workspace, i.e. a proof failing mid-flight for someone else"
        );
    }

    #[test]
    fn reap_ignores_everything_that_is_not_one_of_our_directories() {
        let parent = tempfile::tempdir().unwrap();
        let floor = Duration::from_secs(3600);
        let old = floor + Duration::from_secs(60);

        // Not our prefix — someone else's directory in the same parent.
        let foreign = parent.path().join("someone-elses-dir");
        std::fs::create_dir_all(&foreign).unwrap();
        age(&foreign, old);

        // Our prefix but a FILE, not a workspace.
        let file = parent.path().join("prove-not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        age(&file, old);

        assert_eq!(reap_prove_dirs_older_than(parent.path(), floor), 0);
        assert!(foreign.exists(), "a non-matching name must be untouched");
        assert!(file.exists(), "a plain file must not be removed");
    }

    /// A symlink must be judged AS a symlink and skipped — never followed to its target's mtime and
    /// then removed. Otherwise the reaper is an arbitrary-delete primitive for anything that can
    /// write into `prove-tmp`.
    #[cfg(unix)]
    #[test]
    fn reap_does_not_follow_symlinks_out_of_the_parent() {
        let parent = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let precious = elsewhere.path().join("precious");
        std::fs::create_dir_all(&precious).unwrap();
        std::fs::write(precious.join("keep-me"), b"important").unwrap();
        let floor = Duration::from_secs(3600);
        age(&precious, floor + Duration::from_secs(60));

        let link = parent.path().join("prove-symlink");
        std::os::unix::fs::symlink(&precious, &link).unwrap();

        assert_eq!(reap_prove_dirs_older_than(parent.path(), floor), 0);
        assert!(precious.exists(), "the symlink target must be untouched");
        assert!(precious.join("keep-me").exists());
    }

    /// The sweep runs on the startup path; an unreadable parent must be a no-op, not a panic.
    #[test]
    fn reap_on_a_missing_parent_is_a_no_op() {
        let parent = tempfile::tempdir().unwrap();
        let missing = parent.path().join("does-not-exist");
        assert_eq!(
            reap_prove_dirs_older_than(&missing, Duration::from_secs(1)),
            0
        );
    }

    // ── B3 (F4): capped stderr drain ──

    #[tokio::test]
    async fn drain_capped_retains_at_most_cap_and_counts_the_full_total() {
        // 1 MiB of input, 64-byte retain cap: we must KEEP only 64 bytes but COUNT all 1 MiB (proving it
        // drained to EOF rather than stopping at the cap). Reverting the `retained.len() < cap` guard so
        // it accumulates everything makes `retained.len()` 1 MiB and fails the cap assertion.
        let data = vec![b'x'; 1024 * 1024];
        let (retained, total) = drain_capped(data.as_slice(), 64).await;
        assert_eq!(retained.len(), 64, "must retain at most the cap");
        assert_eq!(total, 1024 * 1024, "must still count every byte to EOF");
        assert!(retained.iter().all(|&b| b == b'x'));
    }

    #[tokio::test]
    async fn drain_capped_handles_input_smaller_than_cap() {
        let (retained, total) = drain_capped(b"hello".as_slice(), 64).await;
        assert_eq!(retained, b"hello");
        assert_eq!(total, 5);
    }

    // ── B3 (F5): proof-output validation ──

    #[test]
    fn validate_proof_len_accepts_only_nonempty_capped_field_aligned() {
        // The security property: exit-code success is no longer the only gate. Reverting any arm of
        // `validate_proof_len` lets a bad proof through and fails one of these.
        assert!(validate_proof_len(32).is_ok());
        assert!(validate_proof_len(32 * 40).is_ok());
        assert!(validate_proof_len(0).is_err(), "empty must be rejected");
        assert!(
            validate_proof_len(5).is_err(),
            "non-32-aligned must be rejected"
        );
        assert!(validate_proof_len(31).is_err());
        assert!(
            validate_proof_len(MAX_PROOF_BYTES + 32).is_err(),
            "oversized must be rejected"
        );
    }

    // ── B3 (F4/F5): end-to-end through prove() with a fake bb ──

    /// Write an executable fake `bb` at `dir/fake-bb` whose body is `script` (a `/bin/sh` program that
    /// receives bb's real argv, incl. `-o <output_dir>`), and point `BB_BINARY_PATH` at it. Returns an
    /// `EnvGuard` that clears the var on drop. Unix-only (shell script); the prove path is POSIX anyway.
    #[cfg(unix)]
    fn install_fake_bb(dir: &Path, script: &str) -> EnvGuard {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-bb");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("BB_BINARY_PATH", &path);
        EnvGuard
    }

    /// Clears `BB_BINARY_PATH` on drop so a panicking test can't leak it into a sibling (`#[serial]`).
    #[cfg(unix)]
    struct EnvGuard;
    #[cfg(unix)]
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var("BB_BINARY_PATH");
        }
    }

    /// Extract `-o <dir>` from bb's argv, portably, for the fake scripts below.
    #[cfg(unix)]
    const FIND_OUTDIR: &str =
        r#"prev=""; for a in "$@"; do [ "$prev" = "-o" ] && out="$a"; prev="$a"; done"#;

    #[cfg(unix)]
    #[tokio::test]
    #[serial]
    async fn prove_rejects_an_empty_proof_file() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = install_fake_bb(dir.path(), &format!("{FIND_OUTDIR}\n: > \"$out/proof\""));
        let err = prove(b"witness", None, None)
            .await
            .expect_err("an empty proof must be rejected");
        assert!(err.to_string().contains("empty proof"), "got: {err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    #[serial]
    async fn prove_rejects_a_non_field_aligned_proof() {
        let dir = tempfile::tempdir().unwrap();
        // 5 bytes → not a whole number of 32-byte fields.
        let _guard = install_fake_bb(
            dir.path(),
            &format!("{FIND_OUTDIR}\nprintf 'xxxxx' > \"$out/proof\""),
        );
        let err = prove(b"witness", None, None)
            .await
            .expect_err("a misaligned proof must be rejected");
        assert!(err.to_string().contains("32-byte fields"), "got: {err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    #[serial]
    async fn prove_succeeds_with_a_valid_proof_despite_chatty_stderr() {
        let dir = tempfile::tempdir().unwrap();
        // ~600 KiB of stderr (well over the 64 KiB retain cap) plus a valid 32-byte proof. Proves the
        // capped drain neither deadlocks (pipe stays emptied) nor rejects a good proof.
        let _guard = install_fake_bb(
            dir.path(),
            &format!(
                "{FIND_OUTDIR}\ni=0; while [ $i -lt 10000 ]; do echo 'noise noise noise noise noise noise' >&2; i=$((i+1)); done\nprintf '%032d' 0 > \"$out/proof\""
            ),
        );
        let proof = prove(b"witness", None, None)
            .await
            .expect("a valid proof must succeed even with heavy stderr");
        // 4-byte header (field_count = 32/32 = 1) + 32-byte proof.
        assert_eq!(proof.len(), 4 + 32);
    }

    #[cfg(unix)]
    #[tokio::test]
    #[serial]
    async fn prove_times_out_and_errs_when_bb_hangs() {
        let dir = tempfile::tempdir().unwrap();
        // A bb that sleeps far past the injected timeout and never writes a proof.
        let _guard = install_fake_bb(dir.path(), "sleep 30");
        let err = prove_with_timeout(b"witness", None, None, Duration::from_millis(150))
            .await
            .expect_err("a hung bb must time out");
        assert!(err.to_string().contains("timed out"), "got: {err}");
    }
}
