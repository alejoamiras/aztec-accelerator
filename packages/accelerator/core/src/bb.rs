use std::path::PathBuf;
use std::time::Duration;

use crate::versions;

/// Maximum time to wait for bb prove to complete before killing the process.
const PROVE_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

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
    let child = cmd.spawn()?;
    let output = match tokio::time::timeout(PROVE_TIMEOUT, child.wait_with_output()).await {
        Ok(result) => result?,
        Err(_) => {
            tracing::error!("bb prove timed out after {:?}", PROVE_TIMEOUT);
            return Err("bb prove timed out after 5 minutes".into());
        }
    };

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        tracing::warn!("bb stderr:\n{}", truncate_stderr(&stderr));
    }

    if !output.status.success() {
        // Log full stderr server-side, but return only a generic error to HTTP clients
        // to avoid leaking bb internals (file paths, witness data) to the browser.
        tracing::error!(exit_code = %output.status, "bb prove failed");
        return Err(format!("bb prove failed (exit {})", output.status).into());
    }

    let proof_path = output_dir.join("proof");
    let raw_proof = std::fs::read(&proof_path)?;

    tracing::debug!(proof_bytes = raw_proof.len(), "bb prove completed");

    Ok(prepend_field_count_header(&raw_proof))
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
}
