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
    // kill_on_drop ensures the DIRECT bb process is killed if the future is cancelled (e.g. client
    // disconnect, timeout). Without it, an orphaned bb would run to completion wasting CPU while holding
    // the prove semaphore. B3 (F6): `configure` additionally puts bb in its own process group (Unix) so
    // the whole TREE — including any process bb forks — can be reaped, and the guard below covers the
    // exit/restart/update paths `kill_on_drop` cannot.
    cmd.kill_on_drop(true);
    containment::configure(&mut cmd);
    // B3 (F6 + codex r3 H2a): SPAWN and register the child's group ATOMICALLY under the containment lock so
    // a quit/restart/update (via `terminate_inflight`) — or a cancellation/timeout (via the guard's Drop) —
    // reaps the whole bb tree, AND so the spawn→contain window is serialized against `begin_quiesce` (no bb
    // can exist un-tracked while an install confirms nothing is running). Fail-closed: if the child can't be
    // contained, or an install is quiescing, don't prove.
    let (mut child, guard) = containment::spawn_and_register(&mut cmd)?;
    // B3 (F4): drain stderr CAP-and-CONTINUE in a CONCURRENT TASK so the pipe never fills (bb would block
    // writing) — and, importantly, so `child.wait()` (which does NOT depend on stderr EOF) can clear the
    // containment registration the instant bb exits rather than after the drain. Without that split, a
    // grandchild that inherits and holds the stderr pipe would keep the drain — and thus the now-stale
    // pgid — alive for the whole timeout (codex M4). `wait_with_output()` also buffered ALL of stderr in
    // memory up to the timeout; the retain cap bounds that.
    let stderr_pipe = child
        .stderr
        .take()
        .expect("spawn_capturing_stderr pipes stderr");
    // codex r3 M4b: the drain writes into a SHARED accumulator so the partial stderr survives an abort.
    let stderr_acc: DrainAcc = std::sync::Arc::new(std::sync::Mutex::new((Vec::new(), 0)));
    let mut drain = AbortingDrain(tokio::spawn(drain_capped_into(
        stderr_pipe,
        STDERR_RETAIN_CAP,
        stderr_acc.clone(),
    )));

    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        // bb may still be running on these paths; returning drops `guard`, whose Drop reaps the whole tree
        // (Unix: SIGKILLs the group; Windows: TerminateJobObject), and `kill_on_drop` reaps the direct
        // child.
        Ok(Err(e)) => return Err(Box::new(e)),
        Err(_) => {
            tracing::error!("bb prove timed out after {:?}", timeout);
            return Err("bb prove timed out after 5 minutes".into());
        }
    };
    // bb has EXITED (reaped by child.wait()). Finish the guard IMMEDIATELY — before the stderr drain
    // necessarily finishes — so the pgid can't go stale while a lingering pipe-holder keeps draining. On
    // Unix `finish()` SIGKILLs the group first (reaping any child bb orphaned before we clear the registry —
    // codex r2 M4a) then clears; that kill is also what makes the drain above EOF promptly.
    guard.finish();

    // Wait (bounded) for the concurrent drain to finish: `finish()` above SIGKILLed the group, so any
    // grandchild holding the pipe open is now dead and stderr EOFs promptly — the 2s bound is a backstop,
    // not the expected path. Whether it completes or times out, the retained bytes are in `stderr_acc`
    // (codex r3 M4b), and `drain` drops + aborts the task on the way out / on any early return.
    let _ = tokio::time::timeout(Duration::from_secs(2), &mut drain.0).await;
    let (stderr_retained, stderr_total) = {
        let g = stderr_acc.lock().unwrap();
        (g.0.clone(), g.1)
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
    // header-only body; a non-32-aligned file → silently floor-divided). Read through a capped reader in a
    // SINGLE open (no metadata-then-read TOCTOU — codex M5): at most MAX_PROOF_BYTES+1 bytes, so an
    // oversized file reads as MAX+1 and is rejected. Then validate the ACTUAL bytes read.
    let proof_path = output_dir.join("proof");
    let raw_proof = read_capped(&proof_path, MAX_PROOF_BYTES)?;
    validate_proof_len(raw_proof.len() as u64)?;

    tracing::debug!(proof_bytes = raw_proof.len(), "bb prove completed");

    Ok(prepend_field_count_header(&raw_proof))
}

/// B3 (F5): read `path` in ONE open, at most `cap`+1 bytes (no metadata/read TOCTOU). Reading `cap`+1
/// lets [`validate_proof_len`] distinguish "exactly `cap`" (accept) from "over `cap`" (reject).
fn read_capped(path: &Path, cap: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)?
        .take(cap + 1)
        .read_to_end(&mut buf)?;
    Ok(buf)
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

/// Shared stderr accumulator: `(retained ≤cap bytes, total bytes seen)`. Written incrementally by
/// [`drain_capped_into`] so a reader can recover the PARTIAL diagnostics even if the drain task is later
/// aborted (codex r3 M4b) — a plain return value would be lost on abort.
type DrainAcc = std::sync::Arc<std::sync::Mutex<(Vec<u8>, u64)>>;

/// B3 (F4): read `reader` to EOF, RETAINING at most `cap` bytes into `acc` but continuing to drain the
/// rest, updating `acc` after every chunk. Cap-and-continue (not fail-closed): stderr volume is a
/// diagnostic, not an integrity signal, so aborting a valid proof over a chatty bb would be a
/// self-inflicted DoS — but we must keep emptying the pipe so bb never blocks on a full buffer.
async fn drain_capped_into<R>(mut reader: R, cap: usize, acc: DrainAcc)
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break, // EOF
            Ok(n) => {
                let mut g = acc.lock().unwrap();
                g.1 = g.1.saturating_add(n as u64);
                if g.0.len() < cap {
                    let room = cap - g.0.len();
                    g.0.extend_from_slice(&buf[..room.min(n)]);
                }
            }
            Err(_) => break, // read error — stop draining, keep what we have
        }
    }
}

/// Thin wrapper returning the accumulated `(retained, total)` — used by the unit tests; the production path
/// keeps its own [`DrainAcc`] so it can read the partial result on an abort.
#[cfg(test)]
async fn drain_capped<R>(reader: R, cap: usize) -> (Vec<u8>, u64)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let acc: DrainAcc = std::sync::Arc::new(std::sync::Mutex::new((Vec::new(), 0)));
    drain_capped_into(reader, cap, acc.clone()).await;
    let g = acc.lock().unwrap();
    (g.0.clone(), g.1)
}

/// Owns the spawned stderr-drain task and ABORTS it on drop (codex r2 M4b). Without this, the early-return
/// paths of `prove_with_timeout` (timeout, wait error) would DETACH the task rather than tear it down. The
/// retained stderr lives in the shared [`DrainAcc`], not this handle, so the caller reads it independently
/// of whether the task finished or was aborted.
struct AbortingDrain(tokio::task::JoinHandle<()>);

impl Drop for AbortingDrain {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// B3 (F6): kill the WHOLE in-flight bb process tree (bb + anything it spawns) when the app exits,
/// restarts, updates, or the prove is cancelled/times out. `kill_on_drop` only reaps the direct child
/// on an in-process future-drop; it does nothing for `std::process::exit`/`app.exit()`/`app.restart()`
/// or for grandchildren. The mechanism is per-OS but the API — [`configure`], a [`Guard`] returned by
/// `Guard::register`, and [`terminate_inflight`] — is uniform.
///
/// [`configure`]: containment::configure
/// [`terminate_inflight`]: containment::terminate_inflight
pub use containment::{terminate_and_confirm, terminate_inflight};

/// B3 (codex r2 H2a): while an update installs, NO new bb may start — a live `bb.exe` blocks the Windows
/// installer from replacing it, and proving over a half-installed app is undefined everywhere. Flip a
/// process-global latch (making every subsequent [`Guard::register`](containment) FAIL) and hold the
/// returned guard across terminate-and-confirm + install. Dropping it — an ABORTED install — re-opens
/// proving. Together with [`terminate_and_confirm`] this closes the window between "confirmed dead" and
/// "installed": a prove that registered just before the latch is killed+confirmed by the terminator; one
/// that races after is refused registration.
#[must_use = "hold the guard across the install; dropping it immediately re-opens proving"]
pub fn begin_quiesce() -> QuiesceGuard {
    containment::begin_quiesce();
    QuiesceGuard(())
}

/// RAII latch from [`begin_quiesce`]. Re-opens proving on drop (only reached when the install ABORTS — a
/// successful install exits/restarts the process, so the drop never runs).
pub struct QuiesceGuard(());

impl Drop for QuiesceGuard {
    fn drop(&mut self) {
        containment::end_quiesce();
    }
}

#[cfg(unix)]
mod containment {
    use std::sync::Mutex;

    /// The in-flight-bb registry, under ONE mutex so every operation is serialized:
    /// - `pgid`: the process-group id of the running bb (== bb's pid, because we spawn it as its own group
    ///   leader). `None` when no prove is running. `kill(-pgid)` SIGKILLs bb AND every descendant it forked
    ///   in one call.
    /// - `quiescing`: set by [`begin_quiesce`] while an update installs. `register` refuses new bb while
    ///   set, so — because setting it and taking the pgid both happen under this lock —
    ///   [`terminate_and_confirm`] observes a registry that CANNOT grow under it: any bb registered before
    ///   the latch is killed+confirmed; any that races after fails to register. Closes the "a prove starts
    ///   between confirm and install" hole (codex r2 H2a).
    struct State {
        pgid: Option<i32>,
        quiescing: bool,
    }
    static STATE: Mutex<State> = Mutex::new(State {
        pgid: None,
        quiescing: false,
    });

    /// Put the child in its OWN process group so the whole tree can be signalled at once.
    pub(super) fn configure(cmd: &mut tokio::process::Command) {
        cmd.process_group(0);
    }

    /// Stop accepting new proves (an update is about to install). Held via an RAII guard in the caller;
    /// [`end_quiesce`] re-opens proving if the install ABORTS.
    pub(super) fn begin_quiesce() {
        STATE.lock().unwrap().quiescing = true;
    }
    pub(super) fn end_quiesce() {
        STATE.lock().unwrap().quiescing = false;
    }

    /// Spawn bb and register its group ATOMICALLY under the state lock (codex r3 H2a). Registering AFTER a
    /// separate spawn left a hole: a prove that had already spawned — with a grandchild `kill_on_drop`
    /// can't reach — would run during install, because the quiesce latch only gated *registration*. Holding
    /// the lock ACROSS the (synchronous, no-`.await`) spawn closes it: `begin_quiesce` either takes the lock
    /// first (this then refuses to spawn) or must wait out the in-flight spawn, after which the terminator
    /// sees the registered pgid and kills it. Fail-closed on a quiescing install.
    pub(super) fn spawn_and_register(
        cmd: &mut tokio::process::Command,
    ) -> std::io::Result<(tokio::process::Child, Guard)> {
        let mut st = STATE.lock().unwrap();
        if st.quiescing {
            return Err(std::io::Error::other(
                "the accelerator is installing an update; proving is paused",
            ));
        }
        let child = super::spawn_capturing_stderr(cmd)?;
        let pgid = child
            .id()
            .ok_or_else(|| std::io::Error::other("bb child has no pid immediately after spawn"))?
            as i32;
        st.pgid = Some(pgid);
        Ok((child, Guard { pgid }))
    }

    /// RAII registration for the spawned child's group. On drop it SIGKILLs the group **iff still
    /// registered** — i.e. the prove did NOT complete normally (client-disconnect future-drop, timeout,
    /// panic). On the normal path [`finish`](Guard::finish) reaps+clears first, so drop is a no-op.
    /// Registration/clear/kill all happen under one lock, so `terminate_inflight` and this guard are
    /// serialized (no double kill; whoever clears first wins).
    pub(super) struct Guard {
        pgid: i32,
    }

    impl Guard {
        /// Normal completion: bb has already exited (we hold its status). SIGKILL the group FIRST — to reap
        /// any child process bb orphaned (normally none, but once we clear the registry they'd be
        /// unkillable) — THEN clear (codex r2 M4a). bb's pid was reaped microseconds ago by `child.wait()`;
        /// we kill a process GROUP under the lock, so for this to hit an unrelated process the OS would have
        /// to recycle bb's exact pid INTO a new group leader within those few instructions — physically
        /// impossible, documented as residual.
        pub(super) fn finish(self) {
            let mut st = STATE.lock().unwrap();
            if st.pgid == Some(self.pgid) {
                unsafe {
                    libc::kill(-self.pgid, libc::SIGKILL);
                }
                st.pgid = None;
            }
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let mut st = STATE.lock().unwrap();
            if st.pgid == Some(self.pgid) {
                st.pgid = None;
                // SIGKILL the whole group (bb here is likely still alive — timeout/cancel/panic path).
                unsafe {
                    libc::kill(-self.pgid, libc::SIGKILL);
                }
            }
        }
    }

    /// SIGKILL the in-flight bb tree, if any. Fire-and-forget — for the QUIT path, which must never
    /// block. (The pre-update path uses [`terminate_and_confirm`] instead.)
    pub fn terminate_inflight() {
        let mut st = STATE.lock().unwrap();
        if let Some(pgid) = st.pgid.take() {
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
        }
    }

    /// SIGKILL the in-flight bb tree and WAIT (bounded) until the group is CONFIRMED gone, so the pre-update
    /// caller can ABORT the install if bb can't be confirmed dead (codex H2: `kill` reaps asynchronously;
    /// installing over a still-live prover risks file/lock conflicts). `Ok(())` if nothing was in flight.
    pub async fn terminate_and_confirm(timeout: std::time::Duration) -> Result<(), String> {
        let pgid = STATE.lock().unwrap().pgid.take();
        let Some(pgid) = pgid else {
            return Ok(());
        };
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
        let deadline = std::time::Instant::now() + timeout;
        loop {
            // Confirm ONLY on ESRCH — the group is gone AND reaped. bb is reaped by the concurrent prove's
            // `child.wait()`; grandchildren are reaped by init after reparenting. codex r2 H2b: treating
            // EVERY `kill(..., 0) == -1` as success was fail-open (e.g. EPERM would falsely "confirm"); a
            // non-ESRCH error is NOT a confirmation, so we keep polling until ESRCH or the deadline.
            if unsafe { libc::kill(-pgid, 0) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(format!(
                    "bb process group {pgid} not confirmed dead after SIGKILL"
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }
}

#[cfg(windows)]
mod containment {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
        JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
        QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};

    /// A process-lifetime Job Object handle (stored as `isize` so it is `Send`/`Sync` in the static).
    /// Deliberately NEVER closed on the SUCCESS path: `KILL_ON_JOB_CLOSE` fires exactly when this process
    /// exits and the OS closes the last handle, so every bb in the job — and every process bb spawned —
    /// is killed on quit/restart/update/crash. 0 means creation FAILED (fail-closed: register() errors).
    static JOB: OnceLock<isize> = OnceLock::new();

    /// `quiescing` flag under a mutex (codex r3 H2a). `spawn_and_register` holds this lock ACROSS
    /// spawn+assign and `begin_quiesce` takes it, so the two are serialized: a spawn in flight when an
    /// install begins is waited out (its bb lands in the job, which `terminate_and_confirm`'s poll then sees
    /// → the update aborts over it, fail-safe), and once quiescing is set no new bb is spawned. Replaces the
    /// r2 atomic, whose check/assign could not be made atomic without this lock.
    static GATE: Mutex<bool> = Mutex::new(false);

    /// Set when a spawn-cleanup could NOT confirm the child dead (codex r5). It poisons
    /// `terminate_and_confirm`, so updates ABORT rather than install over a possibly-live uncontained bb —
    /// until the process restarts (which recreates the job fresh, clearing this). Never cleared in-process;
    /// fail-closed.
    static POISONED: AtomicBool = AtomicBool::new(false);

    fn job_handle() -> HANDLE {
        let raw = *JOB.get_or_init(|| unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return 0;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // Codex H3: a DISCARDED return here would leave the job WITHOUT kill-on-close — assignment and
            // IsProcessInJob would still succeed while exit/crash containment was silently OFF. Fail
            // closed: on failure close the handle and cache 0, so every register() errors rather than
            // running an uncontained bb.
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                CloseHandle(job);
                return 0;
            }
            job as isize
        });
        raw as HANDLE
    }

    /// Active process count in the job — `Some(0)` once every bb + descendant is gone, `None` if the query
    /// itself FAILED. codex r2 H2b: mapping a failed query to `0` was fail-open — it would let
    /// `terminate_and_confirm` "confirm" an empty job it could not actually read, and install over a live
    /// bb. A `None` is NOT a confirmation.
    fn active_processes(job: HANDLE) -> Option<u32> {
        let mut acct: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            QueryInformationJobObject(
                job,
                JobObjectBasicAccountingInformation,
                &mut acct as *mut _ as *mut c_void,
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            None
        } else {
            Some(acct.ActiveProcesses)
        }
    }

    /// No spawn-time configuration on Windows — the child is added to the job AFTER it exists.
    pub(super) fn configure(_cmd: &mut tokio::process::Command) {}

    /// Stop accepting new proves (an update is about to install); [`end_quiesce`] re-opens on abort. Taking
    /// `GATE` here waits out any spawn currently mid-assignment (codex r3 H2a).
    pub(super) fn begin_quiesce() {
        *GATE.lock().unwrap() = true;
    }
    pub(super) fn end_quiesce() {
        *GATE.lock().unwrap() = false;
    }

    /// Armed on register; on ABNORMAL drop (cancellation / timeout / panic — `finish` NOT called) it
    /// `TerminateJobObject`s the tree NOW rather than leaving grandchildren alive in the still-open job
    /// until the process exits (codex H1). Serialized proving means the job holds only this one bb.
    pub(super) struct Guard {
        armed: bool,
    }

    /// Synchronously destroy a bb we spawned but FAILED to contain, BEFORE releasing `GATE` (codex r4). If
    /// we just returned Err, `kill_on_drop` would only *initiate* async direct-child termination — a
    /// `begin_quiesce` waiting on `GATE` could then confirm the job empty and install while the uncontained
    /// bb.exe is still dying (or has forked an uncontained descendant). So: `TerminateJobObject` (reaps
    /// anything that DID make it into the job — the assign-succeeded-but-verify-failed case) AND
    /// `TerminateProcess` + `WaitForSingleObject` on the direct handle (kills it even when assignment never
    /// took, and WAITS so bb.exe is gone before an install can touch it).
    ///
    /// Returns `true` ONLY when the wait actually SIGNALED the child dead. codex r5: ignoring the wait
    /// result was fail-open — a `WAIT_TIMEOUT`/`WAIT_FAILED` (or a failed `TerminateProcess`, which then
    /// times out the wait) would release `GATE` with the child un-confirmed. A `false` return MUST poison.
    pub(super) fn contain_spawn_failure(job: HANDLE, raw: HANDLE) -> bool {
        unsafe {
            TerminateJobObject(job, 1);
            TerminateProcess(raw, 1);
            // 5s is a generous bound; a force-killed process signals near-instantly. Only WAIT_OBJECT_0 is a
            // confirmed exit — WAIT_TIMEOUT / WAIT_FAILED are NOT.
            WaitForSingleObject(raw, 5000) == WAIT_OBJECT_0
        }
    }

    /// Test-only accessor for the process-lifetime job handle (so a Windows unit test can drive
    /// [`contain_spawn_failure`] against a real child).
    #[cfg(test)]
    pub(super) fn job_handle_for_test() -> HANDLE {
        job_handle()
    }

    /// Spawn bb and assign it to the job ATOMICALLY under `GATE` (codex r3 H2a — see [`GATE`]). Holding the
    /// lock across the (synchronous) spawn+assign is what makes `begin_quiesce` exclude or wait out an
    /// in-flight spawn; the r2 before/after atomic checks could not close that race. On ANY post-spawn
    /// failure we destroy the bb synchronously before releasing `GATE` (codex r4 — see
    /// [`contain_spawn_failure`]).
    pub(super) fn spawn_and_register(
        cmd: &mut tokio::process::Command,
    ) -> std::io::Result<(tokio::process::Child, Guard)> {
        let gate = GATE.lock().unwrap();
        if *gate {
            return Err(std::io::Error::other(
                "the accelerator is installing an update; proving is paused",
            ));
        }
        let job = job_handle();
        if job.is_null() {
            return Err(std::io::Error::other("failed to create the bb Job Object"));
        }
        let child = super::spawn_capturing_stderr(cmd)?;
        // tokio's Child exposes the raw process handle on Windows.
        let Some(raw) = child.raw_handle().map(|h| h as HANDLE) else {
            // No handle to assign OR to terminate directly; kill_on_drop reaps the direct child async and
            // UNCONFIRMED, so poison installs until restart (codex r5).
            unsafe { TerminateJobObject(job, 1) };
            POISONED.store(true, Ordering::SeqCst);
            return Err(std::io::Error::other(
                "bb child has no OS handle after spawn",
            ));
        };
        unsafe {
            if AssignProcessToJobObject(job, raw) == 0 {
                let e = std::io::Error::last_os_error();
                if !contain_spawn_failure(job, raw) {
                    POISONED.store(true, Ordering::SeqCst);
                }
                return Err(e);
            }
            // Fail-closed: prove must not proceed on a bb that is NOT actually contained (codex).
            let mut in_job: i32 = 0;
            if IsProcessInJob(raw, job, &mut in_job) == 0 || in_job == 0 {
                if !contain_spawn_failure(job, raw) {
                    POISONED.store(true, Ordering::SeqCst);
                }
                return Err(std::io::Error::other(
                    "bb child is not in the Job Object after assignment",
                ));
            }
        }
        drop(gate); // release only after the bb is contained, so a waiting begin_quiesce sees it in the job
        Ok((child, Guard { armed: true }))
    }

    impl Guard {
        pub(super) fn finish(mut self) {
            self.armed = false;
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if self.armed {
                let job = job_handle();
                if !job.is_null() {
                    unsafe {
                        TerminateJobObject(job, 1);
                    }
                }
            }
        }
    }

    /// Kill every process currently in the job (only ever one bb, since proving is serialized). The job
    /// itself persists for the next prove. Fire-and-forget — for the QUIT path.
    pub fn terminate_inflight() {
        let job = job_handle();
        if !job.is_null() {
            unsafe {
                TerminateJobObject(job, 1);
            }
        }
    }

    /// `TerminateJobObject` is ASYNCHRONOUS (like `TerminateProcess`), so the pre-update path must WAIT
    /// until the job is empty before installing — else NSIS could touch files a still-dying bb holds
    /// (codex H2). Returns Err if the job still has processes after `timeout`.
    pub async fn terminate_and_confirm(timeout: std::time::Duration) -> Result<(), String> {
        // codex r5: if an earlier spawn-cleanup could not confirm a bb dead, we may have a live uncontained
        // bb.exe that is NOT in this job — refuse to install over it until the app restarts.
        if POISONED.load(Ordering::SeqCst) {
            return Err(
                "a previously spawned bb could not be confirmed terminated; refusing to install until the accelerator restarts".to_string(),
            );
        }
        // Hold the job handle as an `isize` (Send) across the `.await` below, materializing the raw `HANDLE`
        // (`*mut c_void`, which is !Send) only transiently for each synchronous WinAPI call. If the raw
        // HANDLE were live across the await, this whole future would be !Send and `perform_update` — which
        // `tauri::async_runtime::spawn`s it (Send + 'static bound) — would fail to compile on Windows. NB: a
        // core `--lib` cross-check does NOT surface this; only the src-tauri spawn site enforces the bound
        // (guarded now by `_assert_terminate_and_confirm_future_is_send`).
        let job_addr = job_handle() as isize;
        if job_addr == 0 {
            return Ok(());
        }
        // `TerminateJobObject` is ASYNCHRONOUS (like `TerminateProcess`). codex r2 H2b: capture its result
        // rather than discarding it, but the poll below is the real confirmation — a failed terminate just
        // means the poll never reaches Some(0) and we abort the install.
        let term_ok = unsafe { TerminateJobObject(job_addr as HANDLE, 1) } != 0;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            // Confirm ONLY on a SUCCESSFUL query that reads 0 (codex r2 H2b: a failed query — `None` — is
            // NOT an empty job).
            if active_processes(job_addr as HANDLE) == Some(0) {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(format!(
                    "bb Job Object not confirmed empty after TerminateJobObject (terminate_ok={term_ok})"
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    /// Compile-time guard: `terminate_and_confirm`'s future MUST stay `Send`, because `perform_update`
    /// `tauri::async_runtime::spawn`s it (`Future + Send + 'static`). A raw `HANDLE` (`*mut c_void`, !Send)
    /// held across the `.await` would silently make it !Send — and a core `--lib` cross-check would STILL
    /// pass, only the MSVC src-tauri build failing 15 minutes into CI. This never runs; it exists so
    /// `cargo check --target x86_64-pc-windows-gnu` fails FAST on a regression.
    #[allow(dead_code)]
    fn _assert_terminate_and_confirm_future_is_send() {
        fn assert_send<T: Send>(_: T) {}
        assert_send(terminate_and_confirm(std::time::Duration::from_secs(0)));
    }
}

#[cfg(not(any(unix, windows)))]
mod containment {
    pub(super) fn configure(_cmd: &mut tokio::process::Command) {}
    pub(super) fn begin_quiesce() {}
    pub(super) fn end_quiesce() {}
    pub(super) struct Guard;
    pub(super) fn spawn_and_register(
        cmd: &mut tokio::process::Command,
    ) -> std::io::Result<(tokio::process::Child, Guard)> {
        let child = super::spawn_capturing_stderr(cmd)?;
        Ok((child, Guard))
    }
    impl Guard {
        pub(super) fn finish(self) {}
    }
    pub fn terminate_inflight() {}
    pub async fn terminate_and_confirm(_timeout: std::time::Duration) -> Result<(), String> {
        Ok(())
    }
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

    // ── B3 (F6): terminate the whole bb PROCESS TREE, not just the direct child ──

    #[cfg(unix)]
    #[tokio::test]
    #[serial]
    async fn terminate_inflight_kills_bb_and_its_grandchild() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("grandchild.pid");
        // A bb that spawns a long-lived GRANDCHILD (which `kill_on_drop` would NOT reap), records its
        // pid, then blocks. Removing `process_group(0)` in `containment::configure` leaves the grandchild
        // in the test's own group, so `kill(-bb_pid)` finds no such group → the grandchild survives and
        // this test fails: the mutation proof for the process-group containment.
        let script = format!(
            "{FIND_OUTDIR}\nsleep 300 & echo $! > \"{}\"\nwait",
            pidfile.display()
        );
        let _env = install_fake_bb(dir.path(), &script);

        // Run prove concurrently — it blocks until we terminate the tree.
        let prove_task = tokio::spawn(async { prove(b"witness", None, None).await });

        // Wait (bounded) for the grandchild to report its pid.
        let mut gpid = None;
        for _ in 0..250 {
            if let Ok(s) = std::fs::read_to_string(&pidfile) {
                if let Ok(p) = s.trim().parse::<i32>() {
                    gpid = Some(p);
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let gpid = gpid.expect("fake bb should have spawned a grandchild and written its pid");
        assert_eq!(
            unsafe { libc::kill(gpid, 0) },
            0,
            "the grandchild should be alive before terminate"
        );

        // Kill the whole tree, as a quit/restart/update would.
        terminate_inflight();
        // Stop waiting on prove regardless of outcome — the property under test is that the GRANDCHILD
        // died. Aborting here means a regression (grandchild survives) fails the assertion below FAST
        // instead of hanging the test on a prove that never returns. On the correct path prove is already
        // finishing (bb was killed); its own `kill_on_drop` reaps the direct child on abort either way.
        prove_task.abort();

        // The grandchild must be gone (SIGKILL to the group). Poll briefly for the reap.
        let mut dead = false;
        for _ in 0..100 {
            if unsafe { libc::kill(gpid, 0) } == -1 {
                dead = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // Best-effort cleanup so a FAILED run (mutation) doesn't leak the surviving grandchild.
        if !dead {
            unsafe { libc::kill(gpid, libc::SIGKILL) };
        }
        assert!(
            dead,
            "the grandchild must be killed with the group by terminate_inflight"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    #[serial]
    async fn terminate_and_confirm_kills_the_tree_and_confirms_it() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("grandchild.pid");
        let script = format!(
            "{FIND_OUTDIR}\nsleep 300 & echo $! > \"{}\"\nwait",
            pidfile.display()
        );
        let _env = install_fake_bb(dir.path(), &script);
        let prove_task = tokio::spawn(async { prove(b"witness", None, None).await });

        let mut gpid = None;
        for _ in 0..250 {
            if let Ok(s) = std::fs::read_to_string(&pidfile) {
                if let Ok(p) = s.trim().parse::<i32>() {
                    gpid = Some(p);
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let gpid = gpid.expect("fake bb should have spawned a grandchild");

        // Kill + WAIT for confirmation. bb is reaped by the concurrent prove task's `child.wait()`, so the
        // group empties and this returns Ok well within the bound. A broken confirm (never sees the group
        // gone) would return Err after the timeout.
        let result = terminate_and_confirm(Duration::from_secs(3)).await;
        prove_task.abort();
        assert!(
            result.is_ok(),
            "terminate_and_confirm must confirm the kill, got {result:?}"
        );
        assert_eq!(
            unsafe { libc::kill(gpid, 0) },
            -1,
            "the grandchild must be dead once terminate_and_confirm returns Ok"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    #[serial]
    async fn terminate_and_confirm_is_ok_when_nothing_is_running() {
        // No prove in flight → nothing to kill → immediate Ok (the updater must not abort a legitimate
        // update just because no proof happened to be running).
        assert!(terminate_and_confirm(Duration::from_millis(50))
            .await
            .is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    #[serial]
    async fn spawn_is_refused_while_quiescing_then_reopens() {
        // codex r3 H2a: once an install quiesces, NO new bb may even SPAWN (the check is under the same lock
        // held across spawn+register), so no fresh bb.exe can hold the file the installer must replace.
        // Mutation proof: delete the `if st.quiescing` guard in `spawn_and_register` and the first call
        // below SUCCEEDS (spawning a bb during a quiesced install).
        use super::containment::spawn_and_register;
        let mk = || {
            let mut cmd = tokio::process::Command::new("sleep");
            cmd.arg("30").kill_on_drop(true);
            super::containment::configure(&mut cmd);
            cmd
        };

        let quiesce = begin_quiesce();
        let mut cmd_a = mk();
        let err = match spawn_and_register(&mut cmd_a) {
            Ok(_) => panic!("spawn must be refused while quiescing"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("paused"),
            "unexpected error: {err}"
        );

        // Dropping the guard (an ABORTED install) must re-open proving.
        drop(quiesce);
        let mut cmd_b = mk();
        let (_child, guard) = spawn_and_register(&mut cmd_b)
            .expect("spawn must work again once the quiesce guard drops");
        guard.finish(); // reaps the sleep's group
    }

    #[cfg(unix)]
    #[tokio::test]
    #[serial]
    async fn finish_reaps_a_straggler_grandchild_on_the_success_path() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("grandchild.pid");
        // bb spawns a GRANDCHILD that outlives it, writes a VALID 32-byte proof, then exits 0 — orphaning
        // the grandchild into bb's still-live process group. On the SUCCESS path `finish()` must SIGKILL
        // that group BEFORE clearing the registry (codex r2 M4a); otherwise the grandchild leaks,
        // unkillable. Mutation proof: drop the `kill(-pgid)` from `finish()` and this assertion fails.
        let script = format!(
            "{FIND_OUTDIR}\nsleep 300 & echo $! > \"{}\"\nprintf '%032d' 0 > \"$out/proof\"\nexit 0",
            pidfile.display()
        );
        let _env = install_fake_bb(dir.path(), &script);

        let proof = prove(b"witness", None, None)
            .await
            .expect("a valid proof must succeed");
        assert_eq!(proof.len(), 4 + 32);

        let gpid: i32 = std::fs::read_to_string(&pidfile)
            .expect("bb should have written the grandchild pid")
            .trim()
            .parse()
            .expect("valid pid");

        // Once prove returned, finish() should already have reaped the group. Poll briefly for the kill.
        let mut dead = false;
        for _ in 0..100 {
            if unsafe { libc::kill(gpid, 0) } == -1 {
                dead = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        if !dead {
            unsafe { libc::kill(gpid, libc::SIGKILL) };
        }
        assert!(
            dead,
            "finish() must SIGKILL the group, reaping the orphaned grandchild"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn contain_spawn_failure_confirms_the_direct_child_is_dead() {
        // The Windows spawn-failure cleanup (codex r4/r5): `TerminateProcess` + `WaitForSingleObject` must
        // actually kill AND confirm the direct child dead — a `false` return is what poisons installs.
        // cargo-check can't validate the wait semantics, so this runs in the Windows CI job. Spawn a
        // long-lived child NOT assigned to the job (as an assignment failure would leave it) and assert the
        // helper reports a CONFIRMED exit.
        let mut cmd = tokio::process::Command::new("ping");
        cmd.args(["-n", "30", "127.0.0.1"]);
        cmd.stdout(std::process::Stdio::null());
        cmd.kill_on_drop(true);
        let child = cmd.spawn().expect("spawn ping");
        let handle = child
            .raw_handle()
            .expect("child has a raw handle on Windows");
        let job = super::containment::job_handle_for_test();
        assert!(
            super::containment::contain_spawn_failure(job, handle as _),
            "contain_spawn_failure must TerminateProcess + confirm the direct child dead"
        );
    }
}
