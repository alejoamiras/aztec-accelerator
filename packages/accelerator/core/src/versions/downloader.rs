//! bb tarball download → integrity-verify → atomic install pipeline.
//!
//! F-04: extracted from `versions.rs` — the heaviest, most self-contained responsibility (network
//! download + digest verification + filesystem install), plus the macOS Gatekeeper finalize tail
//! that was bolted onto the otherwise cross-platform flow (now its own `finalize_downloaded_binary`).
//! The smaller identity/platform/layout/cache concerns stay in the `versions` module root.

use super::cache_layout::{bb_binary_name, version_bb_path, versions_base_dir};
use super::release_metadata::{
    current_platform, download_url, fetch_github_asset_digest, http_client, sha256_hex,
};
use super::version_policy::AztecVersion;
use std::error::Error;
use std::path::PathBuf;

pub async fn download_bb(version: &AztecVersion) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    // The `&AztecVersion` parameter IS the #99 traversal guard: a value of this type can only have
    // been built by `AztecVersion::parse`, which ran `is_valid_version`. An unsafe version therefore
    // cannot reach this sink (`remove_dir_all` below) — the bypass the old sink-side recheck defended
    // against is now structurally impossible. Deref to `&str` once so the existing path/URL/log sites
    // stay byte-identical to the pre-Q3 callee.
    let version: &str = version;
    let bb_path = version_bb_path(version);
    if bb_path.exists() {
        tracing::info!(version, "bb already cached");
        return Ok(bb_path);
    }

    // Download the tarball (bounded streaming) and verify its integrity before touching the fs
    // (Q11: extracted to `download_tarball` + `verify_digest`; the digest→extract ordering — verify
    // BEFORE install — is preserved here in the orchestrator).
    let bytes = download_tarball(version).await?;
    tracing::info!(
        version,
        bytes = bytes.len(),
        "Download complete, verifying integrity"
    );
    verify_digest(version, &bytes).await?;

    // Extract into the version's cache dir via temp dir + atomic rename (Q11: extracted to
    // `install_version_dir` so the cleanup/rename is unit-testable without the network).
    let version_dir = versions_base_dir().join(version);
    install_version_dir(&version_dir, &bytes)?;

    let final_path = version_dir.join(bb_binary_name());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&final_path, std::fs::Permissions::from_mode(0o755))?;
    }

    // macOS: clear quarantine xattrs + ad-hoc re-sign so Gatekeeper doesn't SIGKILL the chmod'd
    // binary. Extracted to `finalize_downloaded_binary` (a no-op off macOS) — F-04 folds the
    // "macOS tail bolted onto the cross-platform flow" sub-finding.
    finalize_downloaded_binary(&final_path, &version_dir, version)?;

    tracing::info!(version, "bb cached successfully");
    Ok(final_path)
}

/// macOS: clear extended attributes (quarantine, provenance) and ad-hoc re-sign the binary so
/// Gatekeeper doesn't SIGKILL it (chmod after the original signature invalidates it). On codesign
/// failure the partial cache dir is removed and an error returned — we never cache an unsignable
/// binary. No-op on other platforms.
#[cfg(target_os = "macos")]
fn finalize_downloaded_binary(
    final_path: &std::path::Path,
    version_dir: &std::path::Path,
    version: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let xattr_out = std::process::Command::new("xattr")
        .args(["-cr"])
        .arg(final_path)
        .output();
    if let Err(e) = &xattr_out {
        tracing::warn!(version, error = %e, "Failed to clear quarantine xattrs");
    } else if let Ok(out) = &xattr_out {
        if !out.status.success() {
            tracing::warn!(version, "xattr -cr failed with status {}", out.status);
        }
    }

    let codesign_out = std::process::Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(final_path)
        .output();
    match &codesign_out {
        Err(e) => {
            tracing::error!(version, error = %e, "Failed to ad-hoc sign bb binary");
            // Clean up — don't cache a binary that can't be signed
            let _ = std::fs::remove_dir_all(version_dir);
            Err(format!("Failed to sign bb v{version}: {e}").into())
        }
        Ok(out) if !out.status.success() => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            tracing::error!(version, stderr = %stderr, "codesign failed");
            let _ = std::fs::remove_dir_all(version_dir);
            Err(format!("codesign failed for bb v{version}: {}", out.status).into())
        }
        Ok(_) => Ok(()),
    }
}

#[cfg(not(target_os = "macos"))]
fn finalize_downloaded_binary(
    _final_path: &std::path::Path,
    _version_dir: &std::path::Path,
    _version: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    Ok(())
}

/// Download the bb tarball for `version` with a bounded-streaming read. The 64 MB cap (advertised
/// Content-Length is an early fail-fast; the running per-chunk counter is the real ceiling) stops a
/// Content-Length-omitting server from OOM-ing us by streaming gigabytes. Mirrors copy-bb.ts
/// `MAX_BB_TARBALL_BYTES`. Byte-identical to the pre-Q11 inline block.
async fn download_tarball(version: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
    let url = download_url(version);
    tracing::info!(version, %url, "Downloading bb");

    let response = http_client().get(&url).send().await?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to download bb v{version}: HTTP {}",
            response.status()
        )
        .into());
    }

    const MAX_DOWNLOAD_BYTES: usize = 64 * 1024 * 1024;
    if let Some(len) = response.content_length() {
        if len > MAX_DOWNLOAD_BYTES as u64 {
            return Err(format!(
                "bb v{version} download too large (advertised {len} bytes, max {MAX_DOWNLOAD_BYTES})"
            )
            .into());
        }
    }
    let mut response = response; // chunk() takes &mut self
    let mut bytes: Vec<u8> = Vec::with_capacity(8 * 1024 * 1024);
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > MAX_DOWNLOAD_BYTES {
            return Err(format!(
                "bb v{version} download exceeded {MAX_DOWNLOAD_BYTES} bytes — aborting"
            )
            .into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Verify the downloaded `bytes` against the GitHub release asset's published SHA-256 digest.
/// **Fail-closed:** a missing digest (`Ok(None)`) or a fetch error is an error, not a skip — we never
/// install unverified code. The bundled sidecar path never reaches here. Byte-identical to the
/// pre-Q11 inline block.
async fn verify_digest(version: &str, bytes: &[u8]) -> Result<(), Box<dyn Error + Send + Sync>> {
    let asset_name = format!("barretenberg-{}.tar.gz", current_platform());
    match fetch_github_asset_digest(version, &asset_name).await {
        Ok(Some(expected)) => {
            let actual = sha256_hex(bytes);
            if actual != expected {
                return Err(format!(
                    "Integrity check failed for bb v{version}: expected sha256:{expected}, got sha256:{actual}"
                )
                .into());
            }
            tracing::info!(version, digest = %actual, "Download integrity verified");
            Ok(())
        }
        Ok(None) => {
            Err(format!("Cannot verify bb v{version}: no digest available from GitHub API").into())
        }
        Err(e) => Err(format!("Cannot verify bb v{version}: digest fetch failed: {e}").into()),
    }
}

/// Install an extracted bb tarball into `version_dir` via a temp dir + atomic rename.
///
/// Cleans up any stale partial-download temp dir, extracts into it, then removes any pre-existing
/// `version_dir` and renames the temp dir into place — so the cache swap is atomic and a previously
/// corrupt entry is replaced wholesale. The temp dir is a sibling named `.{name}.tmp` (derived from
/// `version_dir`'s file name), matching the pre-Q11 inline behavior byte-for-byte.
///
/// Private + single-caller by design: `download_bb` passes `versions_base_dir().join(version)` where
/// `version` came from a validated `AztecVersion`, so the `remove_dir_all` here never sees an
/// attacker-derived path. Pinned by `install_version_dir_replaces_stale_and_extracts_atomically`.
pub(crate) fn install_version_dir(
    version_dir: &std::path::Path,
    bytes: &[u8],
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let name = version_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("invalid version dir (no file name)")?;
    let tmp_dir = version_dir.with_file_name(format!(".{name}.tmp"));

    // Clean up any leftover partial download
    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir)?;
    }
    std::fs::create_dir_all(&tmp_dir)?;

    extract_bb_from_tarball(bytes, &tmp_dir)?;

    // Atomic rename — replace any pre-existing cache entry wholesale
    if version_dir.exists() {
        std::fs::remove_dir_all(version_dir)?;
    }
    std::fs::rename(&tmp_dir, version_dir)?;

    Ok(())
}

/// Extract the `bb` binary from a gzipped tarball.
///
/// Hard ceiling on the DECOMPRESSED tarball size (SEC-07). The compressed download is already capped
/// at 64 MB (`MAX_DOWNLOAD_BYTES`); 512 MB decompressed is ~8x that — well above any legit `bb` (a
/// ≤64 MB-compressed binary inflates to at most a few hundred MB) yet far below a gzip bomb's
/// potential (64 MB of zeros → tens of GB). Without it, `entry.unpack` would stream a bomb to disk.
const MAX_DECOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;

/// A reader that errors once more than `cap` bytes have passed through it — the real backstop against
/// a decompression bomb (the per-entry `header().size()` is attacker-controlled, so it is necessary
/// but not sufficient on its own).
struct CappedReader<R> {
    inner: R,
    read: u64,
    cap: u64,
}

impl<R: std::io::Read> std::io::Read for CappedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read = self.read.saturating_add(n as u64);
        if self.read > self.cap {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "decompressed tarball exceeds {} byte cap (decompression bomb?)",
                    self.cap
                ),
            ));
        }
        Ok(n)
    }
}

/// Looks for an entry named `bb` (at any nesting level) in the archive.
pub(crate) fn extract_bb_from_tarball(
    data: &[u8],
    dest: &std::path::Path,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    extract_bb_from_tarball_capped(data, dest, MAX_DECOMPRESSED_BYTES)
}

/// Inner, cap-parameterized for testing (a real 512 MB bomb is impractical to build in a unit test).
fn extract_bb_from_tarball_capped(
    data: &[u8],
    dest: &std::path::Path,
    cap: u64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    // SEC-07: cap cumulative decompressed bytes so a gzip bomb can't fill the disk via `unpack`.
    let decoder = CappedReader {
        inner: GzDecoder::new(data),
        read: 0,
        cap,
    };
    let mut archive = Archive::new(decoder);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;

        // Look for the bb binary (bb, or bb.exe on Windows) at any level in the archive
        if path.file_name().and_then(|n| n.to_str()) == Some(bb_binary_name()) {
            if entry.header().entry_type() != tar::EntryType::Regular {
                return Err(format!(
                    "bb entry in tarball is not a regular file (type: {:?})",
                    entry.header().entry_type()
                )
                .into());
            }
            // Cheap pre-check: reject a header DECLARING more than the cap. The CappedReader is the
            // real backstop against a lying header that under-declares then over-streams.
            let declared = entry.header().size()?;
            if declared > cap {
                return Err(
                    format!("bb entry declares {declared} bytes, exceeds {cap} byte cap").into(),
                );
            }
            entry.unpack(dest.join(bb_binary_name()))?;
            return Ok(());
        }
    }

    Err("bb binary not found in tarball".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build a gzipped tar with the given entries: `(path, size, byte)`.
    fn make_targz(entries: &[(&str, usize, u8)]) -> Vec<u8> {
        let mut tar_buf = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_buf);
            for (path, size, byte) in entries {
                let data = vec![*byte; *size];
                let mut header = tar::Header::new_gnu();
                header.set_size(*size as u64);
                header.set_entry_type(tar::EntryType::Regular);
                header.set_mode(0o755);
                header.set_cksum();
                builder.append_data(&mut header, path, &data[..]).unwrap();
            }
            builder.finish().unwrap();
        }
        let mut gz = Vec::new();
        let mut enc = flate2::write::GzEncoder::new(&mut gz, flate2::Compression::default());
        enc.write_all(&tar_buf).unwrap();
        enc.finish().unwrap();
        gz
    }

    #[test]
    fn extracts_bb_within_cap() {
        let dir = tempfile::tempdir().unwrap();
        let entry = format!("barretenberg/{}", bb_binary_name());
        let gz = make_targz(&[(&entry, 4096, 0)]);
        extract_bb_from_tarball_capped(&gz, dir.path(), 1024 * 1024).unwrap();
        let out = dir.path().join(bb_binary_name());
        assert_eq!(std::fs::metadata(&out).unwrap().len(), 4096);
    }

    #[test]
    fn rejects_bb_entry_declaring_over_cap() {
        // The per-entry declared-size pre-check: a 2 MB bb against a 1 MB cap is rejected before unpack.
        let dir = tempfile::tempdir().unwrap();
        let entry = format!("barretenberg/{}", bb_binary_name());
        let gz = make_targz(&[(&entry, 2 * 1024 * 1024, 0)]);
        let err = extract_bb_from_tarball_capped(&gz, dir.path(), 1024 * 1024).unwrap_err();
        assert!(err.to_string().contains("cap"), "got: {err}");
        assert!(!dir.path().join(bb_binary_name()).exists());
    }

    #[test]
    fn capped_reader_trips_on_cumulative_decompressed_bytes() {
        // The real bomb backstop: a junk entry BEFORE bb whose data alone exceeds the cap → the
        // CappedReader aborts as the archive advances past it (proves the running counter, not just
        // the per-entry declared size, is enforced).
        let dir = tempfile::tempdir().unwrap();
        let bb = format!("x/{}", bb_binary_name());
        let gz = make_targz(&[("junk.bin", 2 * 1024 * 1024, 7), (&bb, 16, 0)]);
        let err = extract_bb_from_tarball_capped(&gz, dir.path(), 1024 * 1024).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("cap") || msg.contains("bomb"), "got: {msg}");
    }

    #[test]
    fn extract_bb_from_synthetic_tarball() {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        // Create a synthetic tar.gz containing a file named "bb"
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            let bb_content = b"#!/bin/sh\necho hello\n";
            let mut header = tar::Header::new_gnu();
            header.set_size(bb_content.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, bb_binary_name(), &bb_content[..])
                .unwrap();
            builder.finish().unwrap();
        }
        let tarball = encoder.finish().unwrap();

        let tmp = tempfile::tempdir().unwrap();
        extract_bb_from_tarball(&tarball, tmp.path()).unwrap();

        let bb = tmp.path().join(bb_binary_name());
        assert!(bb.exists());
        let contents = std::fs::read_to_string(&bb).unwrap();
        assert!(contents.contains("echo hello"));
    }

    /// Q11 atomic-rename-cleanup: `install_version_dir` extracts into a sibling temp dir then renames
    /// it into place, replacing any stale cache entry wholesale and leaving no temp dir behind. Pins
    /// the behavior the pre-Q11 inline block in `download_bb` had, now that it's a testable unit.
    #[test]
    fn install_version_dir_replaces_stale_and_extracts_atomically() {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        // Minimal valid tarball containing the platform bb binary name.
        let bb_content = b"#!/bin/sh\necho fake-bb\n";
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            let mut header = tar::Header::new_gnu();
            header.set_size(bb_content.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, bb_binary_name(), &bb_content[..])
                .unwrap();
            builder.finish().unwrap();
        }
        let tarball = encoder.finish().unwrap();

        let base = tempfile::tempdir().unwrap();
        let version_dir = base.path().join("5.0.0-test");

        // Fresh install → bb present.
        install_version_dir(&version_dir, &tarball).unwrap();
        assert!(
            version_dir.join(bb_binary_name()).exists(),
            "bb extracted on fresh install"
        );

        // A stale cache entry (junk file) is replaced wholesale by the atomic rename.
        std::fs::write(version_dir.join("STALE_JUNK"), b"old").unwrap();
        install_version_dir(&version_dir, &tarball).unwrap();
        assert!(
            version_dir.join(bb_binary_name()).exists(),
            "bb re-extracted after replace"
        );
        assert!(
            !version_dir.join("STALE_JUNK").exists(),
            "stale entry removed by atomic replace"
        );

        // No leftover temp dir after the rename.
        assert!(
            !version_dir.with_file_name(".5.0.0-test.tmp").exists(),
            "temp dir cleaned up after rename"
        );
    }

    #[test]
    fn extract_bb_from_nested_tarball() {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        // Archive with bb nested under a directory: "barretenberg/bb"
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            let bb_content = b"nested-bb";
            let mut header = tar::Header::new_gnu();
            header.set_size(bb_content.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(
                    &mut header,
                    format!("barretenberg/{}", bb_binary_name()),
                    &bb_content[..],
                )
                .unwrap();
            builder.finish().unwrap();
        }
        let tarball = encoder.finish().unwrap();

        let tmp = tempfile::tempdir().unwrap();
        extract_bb_from_tarball(&tarball, tmp.path()).unwrap();

        let bb = tmp.path().join(bb_binary_name());
        assert!(bb.exists());
        assert_eq!(std::fs::read_to_string(&bb).unwrap(), "nested-bb");
    }

    #[test]
    fn extract_bb_fails_when_no_bb_in_archive() {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            let content = b"not-bb";
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_cksum();
            builder
                .append_data(&mut header, "other-file", &content[..])
                .unwrap();
            builder.finish().unwrap();
        }
        let tarball = encoder.finish().unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let result = extract_bb_from_tarball(&tarball, tmp.path());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not found in tarball"));
    }

    #[test]
    fn extract_bb_rejects_symlink_entry() {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        // Create a tar.gz with a symlink named "bb" pointing to /etc/passwd
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_cksum();
            builder
                .append_link(&mut header, bb_binary_name(), "/etc/passwd")
                .unwrap();
            builder.finish().unwrap();
        }
        let tarball = encoder.finish().unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let result = extract_bb_from_tarball(&tarball, tmp.path());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not a regular file"));
    }

    #[test]
    fn extract_bb_fails_on_corrupted_gzip() {
        let corrupted = b"this is not valid gzip data at all";
        let tmp = tempfile::tempdir().unwrap();
        let result = extract_bb_from_tarball(corrupted, tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn extract_bb_fails_on_empty_input() {
        let tmp = tempfile::tempdir().unwrap();
        let result = extract_bb_from_tarball(&[], tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn extract_bb_cleans_up_on_missing_bb() {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        // Valid tar.gz with no "bb" entry — should fail and leave no artifacts
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        {
            let mut builder = tar::Builder::new(&mut encoder);
            let content = b"not-bb";
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_cksum();
            builder
                .append_data(&mut header, "other-file", &content[..])
                .unwrap();
            builder.finish().unwrap();
        }
        let tarball = encoder.finish().unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let result = extract_bb_from_tarball(&tarball, tmp.path());
        assert!(result.is_err());
        // No "bb" file should have been created
        assert!(!tmp.path().join("bb").exists());
    }

    /// Full download E2E: download a real bb binary from GitHub, verify SHA-256,
    /// extract, and confirm the binary is cached and executable.
    /// Gated behind ACCELERATOR_DOWNLOAD_TEST to avoid network calls in regular CI.
    #[tokio::test]
    async fn download_and_verify_bb() {
        if std::env::var("ACCELERATOR_DOWNLOAD_TEST").is_err() {
            eprintln!(
                "Skipping download_and_verify_bb (set ACCELERATOR_DOWNLOAD_TEST=1 to enable)"
            );
            return;
        }

        // Use the bundled version — guaranteed to exist on GitHub releases
        let version = std::env::var("AZTEC_BB_VERSION").unwrap_or("4.2.0-aztecnr-rc.2".to_string());

        // Delete cached version to force a fresh download
        let cached_dir = versions_base_dir().join(&version);
        if cached_dir.exists() {
            std::fs::remove_dir_all(&cached_dir).unwrap();
        }
        assert!(
            !version_bb_path(&version).exists(),
            "cache should be cleared"
        );

        // Download — exercises the full pipeline: HTTP GET → SHA-256 → extract → codesign
        let av = AztecVersion::parse(&version).expect("bundled version is valid");
        let bb_path = download_bb(&av)
            .await
            .unwrap_or_else(|e| panic!("download_bb({version}) failed: {e}"));

        // Verify the binary was cached in the right location
        assert_eq!(bb_path, version_bb_path(&version));
        assert!(bb_path.exists(), "bb binary should exist after download");

        // Verify it's a real file (not a directory or symlink)
        let metadata = std::fs::metadata(&bb_path).unwrap();
        assert!(metadata.is_file(), "bb should be a regular file");
        assert!(
            metadata.len() > 1_000_000,
            "bb binary should be >1MB (got {} bytes)",
            metadata.len()
        );

        // Verify it's executable (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode();
            assert!(
                mode & 0o111 != 0,
                "bb should be executable (mode: {mode:#o})"
            );
        }

        // Clean up — don't leave test artifacts in the user's cache
        if cached_dir.exists() {
            std::fs::remove_dir_all(&cached_dir).unwrap();
        }
    }
}
