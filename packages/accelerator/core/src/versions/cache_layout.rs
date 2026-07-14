//! On-disk layout of the bb version cache (`~/.aztec-accelerator/versions/`). q7e3-F-07: split from
//! the `versions` module root; the root re-exports keep external paths unchanged.

use super::release_metadata::current_platform;
use super::version_policy::AztecVersion;
use std::path::{Path, PathBuf};

/// Returns the base directory for cached bb versions: `~/.aztec-accelerator/versions/`.
pub fn versions_base_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aztec-accelerator")
        .join("versions")
}

/// The bb binary filename on the current platform (`bb.exe` on Windows, `bb` elsewhere).
pub fn bb_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "bb.exe"
    } else {
        "bb"
    }
}

/// Returns the path to a cached bb binary for a given version.
/// q7e3-F-08: takes the validated `&AztecVersion` — an unvalidated string can no longer reach this
/// path-building sink (the #99 traversal guard holds by construction).
pub fn version_bb_path(version: &AztecVersion) -> PathBuf {
    versions_base_dir()
        .join(version.as_str())
        .join(bb_binary_name())
}

/// List all cached bb versions by scanning `versions_base_dir()`.
pub fn list_cached_versions() -> Vec<String> {
    list_cached_versions_in(&versions_base_dir())
}

/// Inner, base-dir-parameterized for testing. A directory counts as a cached version ONLY if its name
/// is a VALID version (skips dot-prefixed `.{v}.tmp.<rand>` staging dirs + junk) AND it holds BOTH the
/// binary and its integrity marker. Cheap stat only — NEVER rehashes: this feeds the hot inventory
/// (`/health` + tray), so it must not read binary bytes. Execution paths rehash via `verify_cached_bb`
/// (F-007).
pub(crate) fn list_cached_versions_in(base: &Path) -> Vec<String> {
    let mut versions = Vec::new();
    if let Ok(entries) = std::fs::read_dir(base) {
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            // Exclude staging dirs (leading dot) and any non-version-named directory.
            if name.starts_with('.') || AztecVersion::parse(&name).is_none() {
                continue;
            }
            let dir = entry.path();
            if dir.join(bb_binary_name()).exists() && dir.join(MARKER_NAME).exists() {
                versions.push(name);
            }
        }
    }
    versions.sort();
    versions
}

// ---------------------------------------------------------------------------
// F-007 integrity marker: every cached bb carries a `bb.sha256.json` recording the verified archive
// digest + the FINAL-binary digest (post macOS codesign). The runtime rehashes the cached binary
// against this marker on every use (`verify_cached_bb`); a missing/malformed/mismatched marker fails
// closed and triggers a verified re-download.
// ---------------------------------------------------------------------------

/// Marker filename, colocated with `bb` in each version dir.
pub(crate) const MARKER_NAME: &str = "bb.sha256.json";
/// Marker schema tag — `read_bb_marker` rejects anything else (forward-compatible evolution).
pub(crate) const MARKER_SCHEMA: &str = "aztec-accelerator/bb-cache-marker@1";

/// Path to a cached version's integrity marker.
pub(crate) fn version_bb_marker_path(version: &AztecVersion) -> PathBuf {
    versions_base_dir().join(version.as_str()).join(MARKER_NAME)
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Streamed SHA-256 of a file (never loads the whole binary into memory).
pub(crate) fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Write the marker at mode `0600`-at-creation (Unix) into `dir` (the staging dir, pre-publish). Both
/// digests are recorded (archive = provenance; binary = what the runtime re-verifies).
pub(crate) fn write_bb_marker(
    dir: &Path,
    version: &str,
    archive_sha256: &str,
    binary_sha256: &str,
) -> std::io::Result<()> {
    use std::io::Write;
    let body = serde_json::json!({
        "schema": MARKER_SCHEMA,
        "version": version,
        "platform": current_platform(),
        "archive_sha256": archive_sha256,
        "binary_sha256": binary_sha256,
    })
    .to_string();

    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(dir.join(MARKER_NAME))?;
    file.write_all(body.as_bytes())
}

/// Read + structurally validate a marker file, returning the verified binary digest. Fail-closed:
/// missing, oversized, malformed, unknown-schema, version/platform-mismatch, or noncanonical-hex ⇒
/// `Err`. Path-parameterized so it is unit-testable without the home-bound cache dir. (Archive digest
/// is validated as canonical hex but not returned — the runtime re-verifies the binary; the archive
/// digest is provenance recorded in the file.)
fn read_bb_marker_at(
    marker_path: &Path,
    expect_version: &str,
    expect_platform: &str,
) -> Result<String, String> {
    let meta = std::fs::metadata(marker_path).map_err(|e| format!("marker missing: {e}"))?;
    if meta.len() > 4096 {
        return Err(format!(
            "marker is implausibly large ({} bytes)",
            meta.len()
        ));
    }
    let raw = std::fs::read_to_string(marker_path).map_err(|e| format!("marker read: {e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("marker parse: {e}"))?;

    let field = |k: &str| v.get(k).and_then(|x| x.as_str());
    if field("schema") != Some(MARKER_SCHEMA) {
        return Err("marker has an unknown schema".to_string());
    }
    if field("version") != Some(expect_version) {
        return Err(format!("marker version does not match {expect_version}"));
    }
    if field("platform") != Some(expect_platform) {
        return Err(format!("marker platform does not match {expect_platform}"));
    }
    let archive = field("archive_sha256").unwrap_or_default();
    let binary = field("binary_sha256").unwrap_or_default();
    if !is_hex64(archive) || !is_hex64(binary) {
        return Err("marker has non-canonical digest(s)".to_string());
    }
    Ok(binary.to_string())
}

/// Fail-closed integrity check for a bb at explicit paths: the binary must be a present regular file
/// whose streamed SHA-256 matches its valid marker's `binary_sha256`. Path-parameterized for testing.
fn verify_bb_entry(
    bb_path: &Path,
    marker_path: &Path,
    expect_version: &str,
    expect_platform: &str,
) -> Result<(), String> {
    let meta =
        std::fs::symlink_metadata(bb_path).map_err(|e| format!("cached bb unreadable: {e}"))?;
    if !meta.file_type().is_file() {
        return Err("cached bb is not a regular file".to_string());
    }
    let expected = read_bb_marker_at(marker_path, expect_version, expect_platform)?;
    let actual = sha256_file(bb_path).map_err(|e| format!("hash cached bb: {e}"))?;
    if actual != expected {
        return Err(format!(
            "SECURITY: cached bb {expect_version} failed integrity check (marker {expected}, actual {actual})"
        ));
    }
    Ok(())
}

/// Fail-closed integrity check for a cached bb: the binary must be a present regular file whose streamed
/// SHA-256 matches its valid marker's `binary_sha256`. Returns the verified path on success. This is the
/// SINGLE authority the runtime trusts before executing a cached bb over the witness (F-007).
pub fn verify_cached_bb(version: &AztecVersion) -> Result<PathBuf, String> {
    let bb_path = version_bb_path(version);
    verify_bb_entry(
        &bb_path,
        &version_bb_marker_path(version),
        version.as_str(),
        current_platform(),
    )
    .map_err(|e| format!("bb {version}: {e}"))?;
    Ok(bb_path)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_bb_path_format() {
        let version = AztecVersion::parse("5.0.0-nightly.20260307").unwrap();
        let path = version_bb_path(&version);
        // Separator-agnostic: compare path components, and use the platform's bb name.
        let tail: std::path::PathBuf = [
            ".aztec-accelerator",
            "versions",
            "5.0.0-nightly.20260307",
            bb_binary_name(),
        ]
        .iter()
        .collect();
        assert!(
            path.ends_with(&tail),
            "got {path:?}, expected to end with {tail:?}"
        );
    }

    #[test]
    fn list_cached_versions_with_temp_dir() {
        // This test creates a temp dir mimicking the versions cache structure
        let tmp = tempfile::tempdir().unwrap();
        let v1_dir = tmp.path().join("5.0.0-nightly.20260301");
        let v2_dir = tmp.path().join("5.0.0-nightly.20260302");
        let v3_dir = tmp.path().join("5.0.0-incomplete"); // no bb file

        std::fs::create_dir_all(&v1_dir).unwrap();
        std::fs::write(v1_dir.join("bb"), b"fake").unwrap();
        std::fs::create_dir_all(&v2_dir).unwrap();
        std::fs::write(v2_dir.join("bb"), b"fake").unwrap();
        std::fs::create_dir_all(&v3_dir).unwrap();
        // v3 has no bb file — should not be listed

        // We can't easily test list_cached_versions() since it uses a fixed base dir,
        // but the core logic (dir scan + bb existence check) is validated by the
        // versions_to_evict tests. Here we validate the dir structure assumption.
        assert!(v1_dir.join("bb").exists());
        assert!(v2_dir.join("bb").exists());
        assert!(!v3_dir.join("bb").exists());
    }

    // ----- F-007 marker + inventory ---------------------------------------------------------------

    /// Write a `{ bb, marker }` cache entry into `base/<version>`; returns the version dir.
    fn write_entry(base: &std::path::Path, version: &str, bb_bytes: &[u8]) -> PathBuf {
        let dir = base.join(version);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(bb_binary_name()), bb_bytes).unwrap();
        let bin = super::sha256_file(&dir.join(bb_binary_name())).unwrap();
        // archive digest is arbitrary provenance for the test (a valid 64-hex).
        write_bb_marker(&dir, version, &"a".repeat(64), &bin).unwrap();
        dir
    }

    #[test]
    fn verify_bb_entry_accepts_a_matching_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_entry(tmp.path(), "5.0.0-rc.2", b"the-bb-bytes");
        assert!(verify_bb_entry(
            &dir.join(bb_binary_name()),
            &dir.join(MARKER_NAME),
            "5.0.0-rc.2",
            current_platform(),
        )
        .is_ok());
    }

    #[test]
    fn verify_bb_entry_is_fail_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_entry(tmp.path(), "5.0.0-rc.2", b"original");
        let bb = dir.join(bb_binary_name());
        let marker = dir.join(MARKER_NAME);
        let plat = current_platform();

        // Tampered binary ⇒ hash mismatch.
        std::fs::write(&bb, b"tampered").unwrap();
        assert!(verify_bb_entry(&bb, &marker, "5.0.0-rc.2", plat).is_err());
        std::fs::write(&bb, b"original").unwrap(); // restore for the remaining checks

        // Version / platform mismatch.
        assert!(verify_bb_entry(&bb, &marker, "9.9.9", plat).is_err());
        assert!(verify_bb_entry(&bb, &marker, "5.0.0-rc.2", "arm64-solaris").is_err());

        // Missing marker.
        assert!(verify_bb_entry(&bb, &tmp.path().join("nope.json"), "5.0.0-rc.2", plat).is_err());

        // Unknown schema / noncanonical hex.
        std::fs::write(&marker, r#"{"schema":"other@9"}"#).unwrap();
        assert!(read_bb_marker_at(&marker, "5.0.0-rc.2", plat).is_err());
        std::fs::write(
            &marker,
            format!(
                r#"{{"schema":"{MARKER_SCHEMA}","version":"5.0.0-rc.2","platform":"{plat}","archive_sha256":"NOTHEX","binary_sha256":"{}"}}"#,
                "b".repeat(64)
            ),
        )
        .unwrap();
        assert!(read_bb_marker_at(&marker, "5.0.0-rc.2", plat).is_err());

        // Oversized marker.
        std::fs::write(&marker, " ".repeat(5000)).unwrap();
        assert!(read_bb_marker_at(&marker, "5.0.0-rc.2", plat).is_err());
    }

    #[test]
    fn verify_bb_entry_rejects_a_symlink_binary() {
        #[cfg(unix)]
        {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("5.0.0-rc.2");
            std::fs::create_dir_all(&dir).unwrap();
            std::os::unix::fs::symlink("/etc/passwd", dir.join(bb_binary_name())).unwrap();
            write_bb_marker(&dir, "5.0.0-rc.2", &"a".repeat(64), &"b".repeat(64)).unwrap();
            assert!(verify_bb_entry(
                &dir.join(bb_binary_name()),
                &dir.join(MARKER_NAME),
                "5.0.0-rc.2",
                current_platform(),
            )
            .is_err());
        }
    }

    #[test]
    fn list_cached_versions_in_excludes_stages_unmarked_and_junk() {
        let base = tempfile::tempdir().unwrap();
        // A valid, marked entry — listed.
        write_entry(base.path(), "5.0.0-rc.2", b"x");
        // Unmarked (bb but no marker) — NOT listed.
        let unmarked = base.path().join("5.0.0-rc.3");
        std::fs::create_dir_all(&unmarked).unwrap();
        std::fs::write(unmarked.join(bb_binary_name()), b"y").unwrap();
        // A crash-stale staging dir (dot-prefixed) WITH a marker — NOT listed.
        let stage = base.path().join(".5.0.0-rc.9.tmp.1234-5-0");
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::write(stage.join(bb_binary_name()), b"z").unwrap();
        write_bb_marker(&stage, "5.0.0-rc.9", &"a".repeat(64), &"b".repeat(64)).unwrap();
        // A junk-named dir — NOT listed.
        std::fs::create_dir_all(base.path().join("not a version!")).unwrap();

        assert_eq!(
            list_cached_versions_in(base.path()),
            vec!["5.0.0-rc.2".to_string()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn marker_is_written_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        write_bb_marker(tmp.path(), "5.0.0-rc.2", &"a".repeat(64), &"b".repeat(64)).unwrap();
        let mode = std::fs::metadata(tmp.path().join(MARKER_NAME))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "marker must be owner-only");
    }

    /// Cross-language contract: the committed fixture the TS suite also loads must parse under the Rust
    /// marker schema (schema tag + canonical-hex digest fields).
    #[test]
    fn shared_fixture_marker_matches_schema() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../scripts/__fixtures__/bb-cache-marker.json");
        let raw = std::fs::read_to_string(&path).expect("fixture bb-cache-marker.json present");
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["schema"].as_str(), Some(MARKER_SCHEMA));
        for f in ["archive_sha256", "binary_sha256"] {
            assert!(is_hex64(v[f].as_str().unwrap()), "{f} must be 64-lc-hex");
        }
        // read_bb_marker_at accepts it when told the fixture's own version + platform.
        let (ver, plat) = (
            v["version"].as_str().unwrap(),
            v["platform"].as_str().unwrap(),
        );
        assert!(read_bb_marker_at(&path, ver, plat).is_ok());
    }
}
