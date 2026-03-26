use std::path::PathBuf;
use std::time::Duration;

use crate::versions;

/// Maximum time to wait for bb prove to complete before killing the process.
const PROVE_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

/// Find the `bb` binary. When `version` is provided, the version cache is checked
/// before the standard search chain.
///
/// Search order:
/// 0. `BB_BINARY_PATH` env var — explicit override (CI, testing, custom installs)
/// 1. Version cache (`~/.aztec-accelerator/versions/{version}/bb`) — when version specified
/// 2. Bundled sidecar (Tauri externalBin) — `binaries/bb-{target-triple}` next to the executable
/// 3. `~/.bb/bb` — user-installed via `bbup`
/// 4. `bb` on `$PATH`
pub fn find_bb(version: Option<&str>) -> Result<PathBuf, String> {
    // 0. Explicit override via environment variable
    if let Ok(path) = std::env::var("BB_BINARY_PATH") {
        let explicit = PathBuf::from(&path);
        if explicit.exists() {
            return Ok(explicit);
        }
    }

    // 1. Version cache (only when a specific version is requested)
    if let Some(v) = version {
        let cached = versions::version_bb_path(v);
        if cached.exists() {
            return Ok(cached);
        }
    }

    // 2. Sidecar: check next to the current executable
    if let Ok(exe) = std::env::current_exe() {
        let sidecar = exe.parent().unwrap_or(&exe).join("bb");
        if sidecar.exists() {
            return Ok(sidecar);
        }
    }

    // 3. ~/.bb/bb (bbup install location)
    if let Some(home) = dirs_next().or_else(home_dir_fallback) {
        let bbup_path = home.join(".bb").join("bb");
        if bbup_path.exists() {
            return Ok(bbup_path);
        }
    }

    // 4. bb on $PATH
    if let Ok(path) = which::which("bb") {
        return Ok(path);
    }

    Err("bb binary not found. Install via bbup or bundle as sidecar.".to_string())
}

fn dirs_next() -> Option<PathBuf> {
    dirs::home_dir()
}

fn home_dir_fallback() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Run `bb prove` on the given IVC inputs (msgpack bytes) and return the proof
/// with a 4-byte BE field-count header suitable for `ChonkProofWithPublicInputs.fromBuffer()`.
///
/// When `version` is specified, searches the version cache for the matching `bb` binary.
/// When `threads` is specified, passes `-t N` to limit parallelism.
pub async fn prove(
    ivc_inputs: &[u8],
    version: Option<&str>,
    threads: Option<usize>,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let bb_path =
        find_bb(version).map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;

    let tmp_dir = tempfile::tempdir()?;
    let input_path = tmp_dir.path().join("ivc-inputs.msgpack");
    let output_dir = tmp_dir.path().join("output");
    std::fs::create_dir_all(&output_dir)?;
    std::fs::write(&input_path, ivc_inputs)?;

    tracing::info!(
        version = version.unwrap_or("bundled"),
        ?threads,
        "Starting bb prove"
    );

    let mut cmd = tokio::process::Command::new(&bb_path);
    cmd.args([
        "prove",
        "--scheme",
        "chonk",
        "--ivc_inputs_path",
        input_path.to_str().unwrap(),
        "-o",
        output_dir.to_str().unwrap(),
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
        let truncated = if stderr.len() > 500 {
            format!(
                "{}... [truncated, {} bytes total]",
                &stderr[..500],
                stderr.len()
            )
        } else {
            stderr.to_string()
        };
        tracing::warn!("bb stderr:\n{truncated}");
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_find_bb_with_version_checks_cache() {
        // Verify that find_bb with a version doesn't panic and follows the chain
        let result = find_bb(Some("99.99.99-nonexistent"));
        match result {
            Ok(path) => assert!(path.exists()),
            Err(msg) => assert!(msg.contains("bb binary not found")),
        }
    }
}
