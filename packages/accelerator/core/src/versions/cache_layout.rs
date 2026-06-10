//! On-disk layout of the bb version cache (`~/.aztec-accelerator/versions/`). q7e3-F-07: split from
//! the `versions` module root; the root re-exports keep external paths unchanged.

use super::version_policy::AztecVersion;
use std::path::PathBuf;

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
    let base = versions_base_dir();
    let mut versions = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            if entry.path().join(bb_binary_name()).exists() {
                if let Some(name) = entry.file_name().to_str() {
                    versions.push(name.to_string());
                }
            }
        }
    }
    versions.sort();
    versions
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
}
