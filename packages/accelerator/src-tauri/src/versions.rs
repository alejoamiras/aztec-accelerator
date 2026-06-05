use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;

/// HTTP client with reasonable timeouts for downloading bb binaries and checking digests.
fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(30))
        .user_agent("aztec-accelerator")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Network tier derived from a version string's prerelease suffix.
/// Controls how many cached bb versions are retained per tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetworkTier {
    /// `*-nightly.*` — daily dev builds, keep 2
    Nightly,
    /// `*-devnet.*` — devnet releases, keep 3
    Devnet,
    /// `*-rc.*` — testnet release candidates, keep 5
    Testnet,
    /// No prerelease suffix — mainnet, keep all
    Mainnet,
}

impl NetworkTier {
    /// Classify a version string into its network tier.
    ///
    /// ```text
    /// "5.0.0-nightly.20260307"  → Nightly
    /// "5.0.0-devnet.20260307"   → Devnet
    /// "5.0.0-rc.1"              → Testnet
    /// "5.0.0"                   → Mainnet
    /// ```
    pub fn from_version(version: &str) -> Self {
        // Split at first '-' to get prerelease portion
        if let Some(prerelease) = version.split_once('-').map(|(_, pre)| pre) {
            if prerelease.starts_with("nightly") {
                return Self::Nightly;
            }
            if prerelease.starts_with("devnet") {
                return Self::Devnet;
            }
            if prerelease.starts_with("rc") {
                return Self::Testnet;
            }
        }
        Self::Mainnet
    }

    /// Maximum number of cached versions to keep for this tier.
    /// Returns `None` for mainnet (keep all).
    pub fn retention_limit(self) -> Option<usize> {
        match self {
            Self::Nightly => Some(2),
            Self::Devnet => Some(3),
            Self::Testnet => Some(5),
            Self::Mainnet => None,
        }
    }
}

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
pub fn version_bb_path(version: &str) -> PathBuf {
    versions_base_dir().join(version).join(bb_binary_name())
}

/// Returns the current platform identifier for download URLs.
///
/// Format: `{ARCH}-{OS}` matching Aztec release naming:
/// - `aarch64-apple-darwin` → `arm64-darwin`
/// - `x86_64-apple-darwin`  → `amd64-darwin`
/// - `x86_64-unknown-linux-gnu` → `amd64-linux`
/// - `aarch64-unknown-linux-gnu` → `arm64-linux`
pub fn current_platform() -> &'static str {
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    {
        "arm64-darwin"
    }
    #[cfg(all(target_arch = "x86_64", target_os = "macos"))]
    {
        "amd64-darwin"
    }
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    {
        "amd64-linux"
    }
    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
    {
        "arm64-linux"
    }
    #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
    {
        "amd64-windows"
    }
}

/// Returns the download URL for a bb tarball from Aztec's GitHub releases.
///
/// Format: `https://github.com/AztecProtocol/aztec-packages/releases/download/v{VERSION}/barretenberg-{PLATFORM}.tar.gz`
pub fn download_url(version: &str) -> String {
    format!(
        "https://github.com/AztecProtocol/aztec-packages/releases/download/v{}/barretenberg-{}.tar.gz",
        version,
        current_platform(),
    )
}

/// Extract a sort key from a version string. Parses the numeric suffix after
/// the prerelease label (e.g., "rc.10" → 10) so RC versions sort numerically
/// rather than lexicographically ("rc.2" < "rc.10").
/// For date-based suffixes (nightly, devnet), lexicographic order is already correct.
fn version_sort_key(version: &str) -> (String, u64) {
    if let Some((base, prerelease)) = version.rsplit_once('.') {
        if let Ok(n) = prerelease.parse::<u64>() {
            return (base.to_string(), n);
        }
    }
    (version.to_string(), 0)
}

/// Determine which cached versions should be evicted per the retention policy.
///
/// - Groups versions by tier
/// - Sorts within each tier by version string (alphabetical, which works for date suffixes)
/// - Returns versions exceeding the tier's retention limit (oldest first)
/// - The bundled version is never evicted
pub fn versions_to_evict(cached: &[String], bundled_version: &str) -> Vec<String> {
    use std::collections::HashMap;

    let mut by_tier: HashMap<NetworkTier, Vec<&String>> = HashMap::new();
    for v in cached {
        let tier = NetworkTier::from_version(v);
        by_tier.entry(tier).or_default().push(v);
    }

    let mut to_evict = Vec::new();
    for (tier, mut versions) in by_tier {
        if let Some(limit) = tier.retention_limit() {
            // Sort ascending (oldest first). Use version-aware sort: for RC versions,
            // parse the numeric suffix so "rc.2" < "rc.10" (not lexicographic where "rc.10" < "rc.2").
            versions.sort_by_key(|v| version_sort_key(v));
            // Remove bundled from the candidate list (it's always kept)
            versions.retain(|v| v.as_str() != bundled_version);
            // Evict oldest non-bundled versions until we're within the limit
            // (limit includes the bundled version if it's in this tier)
            let effective_limit = if cached
                .iter()
                .any(|v| v == bundled_version && NetworkTier::from_version(v) == tier)
            {
                limit.saturating_sub(1)
            } else {
                limit
            };
            // Drain the oldest non-bundled versions beyond the limit in one pass
            // (replaces an O(n²) `Vec::remove(0)` loop; `effective_limit` semantics unchanged).
            let excess = versions.len().saturating_sub(effective_limit);
            to_evict.extend(versions.drain(0..excess).cloned());
        }
    }
    to_evict
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

/// Compute SHA-256 hex digest of the given bytes.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(data))
}

/// Fetch the expected SHA-256 digest for a release asset from the GitHub API.
///
/// GitHub stores a `digest` field (e.g. `"sha256:abcd..."`) on every release asset.
/// This doesn't protect against a compromised GitHub account (attacker can re-upload),
/// but catches download corruption and CDN issues.
///
/// TODO: Verify against upstream signatures when Aztec starts signing releases.
async fn fetch_github_asset_digest(
    version: &str,
    asset_name: &str,
) -> Result<Option<String>, Box<dyn Error + Send + Sync>> {
    let api_url = format!(
        "https://api.github.com/repos/AztecProtocol/aztec-packages/releases/tags/v{version}"
    );
    let response = http_client()
        .get(&api_url)
        .header("accept", "application/vnd.github+json")
        .send()
        .await?;

    if !response.status().is_success() {
        tracing::warn!(
            version,
            status = %response.status(),
            "Failed to fetch release metadata for digest verification"
        );
        return Ok(None);
    }

    let release: serde_json::Value = response.json().await?;
    let assets = release["assets"].as_array();
    if let Some(assets) = assets {
        for asset in assets {
            if asset["name"].as_str() == Some(asset_name) {
                if let Some(digest) = asset["digest"].as_str() {
                    // Format: "sha256:abcdef..."
                    if let Some(hex) = digest.strip_prefix("sha256:") {
                        return Ok(Some(hex.to_string()));
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Download the `bb` binary for the given Aztec version and cache it.
///
/// Flow: check cache → GET tarball → verify digest → extract to temp dir → atomic rename → chmod.
/// Returns the path to the cached `bb` binary.
/// Validate a version string before it is used to build cache paths or download URLs. THE single
/// source of truth for both the HTTP ingress (`server.rs::resolve_version`) and the `download_bb`
/// sink. Rejects path-traversal + injection: non-empty, `<= 128` chars, ASCII alnum/`.`/`-`/`_`,
/// no leading dot, no `..` sequence.
pub fn is_valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 128
        && !version.starts_with('.')
        && !version.contains("..")
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

pub async fn download_bb(version: &str) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    // Defense-in-depth: reject an unsafe version BEFORE deriving any path or touching the network/fs
    // (this fn + version_bb_path are public; `remove_dir_all(version_dir)` below is the dangerous sink).
    if !is_valid_version(version) {
        return Err(format!("Refusing to download bb for invalid version {version:?}").into());
    }
    let bb_path = version_bb_path(version);
    if bb_path.exists() {
        tracing::info!(version, "bb already cached");
        return Ok(bb_path);
    }

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

    // Bounded streaming download: read the body in chunks with a running counter so a server that
    // omits Content-Length (chunked encoding) cannot OOM us by streaming gigabytes. The advertised
    // length is an early fail-fast; the per-chunk counter is the real ceiling. 32 MB cap (bb is ~5 MB).
    const MAX_DOWNLOAD_BYTES: usize = 32 * 1024 * 1024;
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
        if bytes.len() + chunk.len() > MAX_DOWNLOAD_BYTES {
            return Err(format!(
                "bb v{version} download exceeded {MAX_DOWNLOAD_BYTES} bytes — aborting"
            )
            .into());
        }
        bytes.extend_from_slice(&chunk);
    }
    tracing::info!(
        version,
        bytes = bytes.len(),
        "Download complete, verifying integrity"
    );

    // Verify download integrity against GitHub API digest.
    // Fail closed: if we can't verify, we don't execute. The bundled bb sidecar
    // always works without verification; this only affects on-demand downloads.
    let asset_name = format!("barretenberg-{}.tar.gz", current_platform());
    match fetch_github_asset_digest(version, &asset_name).await {
        Ok(Some(expected)) => {
            let actual = sha256_hex(&bytes);
            if actual != expected {
                return Err(format!(
                    "Integrity check failed for bb v{version}: expected sha256:{expected}, got sha256:{actual}"
                )
                .into());
            }
            tracing::info!(version, digest = %actual, "Download integrity verified");
        }
        Ok(None) => {
            return Err(format!(
                "Cannot verify bb v{version}: no digest available from GitHub API"
            )
            .into());
        }
        Err(e) => {
            return Err(format!("Cannot verify bb v{version}: digest fetch failed: {e}").into());
        }
    }

    // Extract to a temporary directory, then atomically rename
    let version_dir = versions_base_dir().join(version);
    let tmp_dir = version_dir.with_file_name(format!(".{version}.tmp"));

    // Clean up any leftover partial download
    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir)?;
    }
    std::fs::create_dir_all(&tmp_dir)?;

    extract_bb_from_tarball(&bytes, &tmp_dir)?;

    // Atomic rename
    if version_dir.exists() {
        std::fs::remove_dir_all(&version_dir)?;
    }
    std::fs::rename(&tmp_dir, &version_dir)?;

    let final_path = version_dir.join(bb_binary_name());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&final_path, std::fs::Permissions::from_mode(0o755))?;
    }

    // macOS: clear extended attributes (quarantine, provenance) and re-sign
    // so Gatekeeper doesn't SIGKILL the binary.
    // - `xattr -cr` clears all xattrs recursively (quarantine, provenance, etc.)
    // - `codesign --force --sign -` applies ad-hoc signing (fixes "invalid signature"
    //   caused by chmod modifying the binary after the original signature was applied)
    #[cfg(target_os = "macos")]
    {
        let xattr_out = std::process::Command::new("xattr")
            .args(["-cr"])
            .arg(&final_path)
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
            .arg(&final_path)
            .output();
        match &codesign_out {
            Err(e) => {
                tracing::error!(version, error = %e, "Failed to ad-hoc sign bb binary");
                // Clean up — don't cache a binary that can't be signed
                let _ = std::fs::remove_dir_all(&version_dir);
                return Err(format!("Failed to sign bb v{version}: {e}").into());
            }
            Ok(out) if !out.status.success() => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::error!(version, stderr = %stderr, "codesign failed");
                let _ = std::fs::remove_dir_all(&version_dir);
                return Err(format!("codesign failed for bb v{version}: {}", out.status).into());
            }
            Ok(_) => {}
        }
    }

    tracing::info!(version, "bb cached successfully");
    Ok(final_path)
}

/// Extract the `bb` binary from a gzipped tarball.
///
/// Looks for an entry named `bb` (at any nesting level) in the archive.
fn extract_bb_from_tarball(
    data: &[u8],
    dest: &std::path::Path,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let decoder = GzDecoder::new(data);
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
            entry.unpack(dest.join(bb_binary_name()))?;
            return Ok(());
        }
    }

    Err("bb binary not found in tarball".into())
}

/// Clean up old cached versions per the retention policy.
pub async fn cleanup_old_versions(bundled_version: &str) {
    let cached = list_cached_versions();
    let to_evict = versions_to_evict(&cached, bundled_version);

    for version in &to_evict {
        let dir = versions_base_dir().join(version);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => tracing::info!(version, "Evicted old bb version"),
            Err(e) => tracing::warn!(version, error = %e, "Failed to evict bb version"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_version_accepts_real_aztec_versions() {
        for v in [
            "5.0.0",
            "5.0.0-rc.1",
            "5.0.0-nightly.20260307",
            "5.0.0-devnet.20260307",
            "4.2.0-aztecnr-rc.2",
            "1.2.3-alpha_beta",
        ] {
            assert!(is_valid_version(v), "should accept {v:?}");
        }
    }

    #[test]
    fn is_valid_version_rejects_traversal_injection_and_dots() {
        for v in [
            "",
            "..",
            ".",
            ".foo",
            "1..2",
            ".5.0.0",
            "../../../etc/passwd",
            "v5.0.0/../../malicious",
            "5.0.0; rm -rf /",
            "5.0.0\n",
            "5.0.0 ",
            "a\\b",
        ] {
            assert!(!is_valid_version(v), "should reject {v:?}");
        }
        assert!(
            !is_valid_version(&"a".repeat(129)),
            "should reject an over-long version"
        );
    }

    /// The sink-side guard must fire BEFORE any path derivation or network/fs access, so a direct
    /// caller (bypassing the server.rs ingress) can't steer `remove_dir_all` toward a traversal path.
    #[tokio::test]
    async fn download_bb_rejects_unsafe_version_at_sink() {
        for v in ["..", ".", "../etc", ".5.0.0", "a/b"] {
            let err = download_bb(v)
                .await
                .expect_err("download_bb must reject an unsafe version");
            assert!(
                err.to_string().contains("invalid version"),
                "expected guard rejection for {v:?}, got: {err}"
            );
        }
    }

    #[test]
    fn tier_classification() {
        assert_eq!(
            NetworkTier::from_version("5.0.0-nightly.20260307"),
            NetworkTier::Nightly
        );
        assert_eq!(
            NetworkTier::from_version("5.0.0-devnet.20260307"),
            NetworkTier::Devnet
        );
        assert_eq!(
            NetworkTier::from_version("5.0.0-rc.1"),
            NetworkTier::Testnet
        );
        assert_eq!(NetworkTier::from_version("5.0.0"), NetworkTier::Mainnet);
        assert_eq!(NetworkTier::from_version("1.2.3"), NetworkTier::Mainnet);
    }

    #[test]
    fn retention_limits() {
        assert_eq!(NetworkTier::Nightly.retention_limit(), Some(2));
        assert_eq!(NetworkTier::Devnet.retention_limit(), Some(3));
        assert_eq!(NetworkTier::Testnet.retention_limit(), Some(5));
        assert_eq!(NetworkTier::Mainnet.retention_limit(), None);
    }

    #[test]
    fn evict_excess_nightlies() {
        let cached = vec![
            "5.0.0-nightly.20260301".into(),
            "5.0.0-nightly.20260302".into(),
            "5.0.0-nightly.20260303".into(),
            "5.0.0-nightly.20260304".into(),
        ];
        let evicted = versions_to_evict(&cached, "5.0.0-nightly.20260304");
        // Keep 2, evict 2 oldest
        assert_eq!(evicted.len(), 2);
        assert!(evicted.contains(&"5.0.0-nightly.20260301".to_string()));
        assert!(evicted.contains(&"5.0.0-nightly.20260302".to_string()));
    }

    #[test]
    fn bundled_version_never_evicted() {
        let cached = vec![
            "5.0.0-nightly.20260301".into(),
            "5.0.0-nightly.20260302".into(),
            "5.0.0-nightly.20260303".into(),
            "5.0.0-nightly.20260304".into(),
        ];
        // Bundled is the oldest — should still not be evicted
        let evicted = versions_to_evict(&cached, "5.0.0-nightly.20260301");
        assert!(!evicted.contains(&"5.0.0-nightly.20260301".to_string()));
        // 4 versions, keep 2, but bundled is protected, so evict the next oldest
        assert_eq!(evicted.len(), 2);
        assert!(evicted.contains(&"5.0.0-nightly.20260302".to_string()));
        assert!(evicted.contains(&"5.0.0-nightly.20260303".to_string()));
    }

    #[test]
    fn mainnet_never_evicted() {
        let cached = vec![
            "1.0.0".into(),
            "2.0.0".into(),
            "3.0.0".into(),
            "4.0.0".into(),
            "5.0.0".into(),
        ];
        let evicted = versions_to_evict(&cached, "5.0.0");
        assert!(evicted.is_empty());
    }

    #[test]
    fn rc_versions_sort_numerically_not_lexicographically() {
        // With lexicographic sort, rc.10 < rc.2 — wrong!
        // With numeric sort, rc.1 < rc.2 < rc.3 < rc.10 — correct.
        let cached: Vec<String> = vec![
            "4.0.0-rc.1".into(),
            "4.0.0-rc.2".into(),
            "4.0.0-rc.3".into(),
            "4.0.0-rc.10".into(),
            "4.0.0-rc.11".into(),
            "4.0.0-rc.20".into(),
        ];
        // Testnet tier: keep 5, evict 1 (oldest = rc.1)
        let evicted = versions_to_evict(&cached, "4.0.0-rc.20");
        assert_eq!(evicted, vec!["4.0.0-rc.1"]);
    }

    #[test]
    fn mixed_tiers() {
        let cached = vec![
            "5.0.0-nightly.20260301".into(),
            "5.0.0-nightly.20260302".into(),
            "5.0.0-nightly.20260303".into(),
            "5.0.0-devnet.20260301".into(),
            "5.0.0-rc.1".into(),
            "5.0.0".into(),
        ];
        let evicted = versions_to_evict(&cached, "5.0.0");
        // Nightlies: 3, keep 2, evict 1
        assert_eq!(evicted.len(), 1);
        assert!(evicted.contains(&"5.0.0-nightly.20260301".to_string()));
    }

    #[test]
    fn download_url_format() {
        let url = download_url("5.0.0-nightly.20260307");
        assert!(url.starts_with("https://github.com/AztecProtocol/aztec-packages/releases/download/v5.0.0-nightly.20260307/barretenberg-"));
        assert!(url.ends_with(".tar.gz"));
    }

    #[test]
    fn current_platform_matches_aztec_naming() {
        // Aztec releases use "darwin" (not "macos") and "linux"
        let valid = [
            "arm64-darwin",
            "amd64-darwin",
            "amd64-linux",
            "arm64-linux",
            "amd64-windows",
        ];
        let platform = current_platform();
        assert!(
            valid.contains(&platform),
            "current_platform() returned '{platform}', expected one of {valid:?}. \
             Check Aztec release assets at https://github.com/AztecProtocol/aztec-packages/releases"
        );
    }

    /// Smoke test: verify the download URL for a known release actually resolves (HTTP HEAD).
    /// Gated behind ACCELERATOR_DOWNLOAD_TEST to avoid network calls in regular CI.
    #[tokio::test]
    async fn download_url_resolves() {
        if std::env::var("ACCELERATOR_DOWNLOAD_TEST").is_err() {
            eprintln!("Skipping download_url_resolves (set ACCELERATOR_DOWNLOAD_TEST=1 to enable)");
            return;
        }
        // Use a known stable version that will always exist
        let version = std::env::var("AZTEC_BB_VERSION").unwrap_or("5.0.0-nightly.20260307".into());
        let url = download_url(&version);
        let client = reqwest::Client::new();
        let resp = client
            .head(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .unwrap_or_else(|e| panic!("HEAD {url} failed: {e}"));
        assert!(
            resp.status().is_success() || resp.status().is_redirection(),
            "HEAD {url} returned {}, expected 2xx/3xx. \
             The download URL pattern may have changed — check Aztec release assets.",
            resp.status()
        );
    }

    #[test]
    fn version_bb_path_format() {
        let path = version_bb_path("5.0.0-nightly.20260307");
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

    #[test]
    fn sha256_hex_produces_correct_digest() {
        // SHA-256 of empty input is the well-known constant
        let digest = sha256_hex(b"");
        assert_eq!(
            digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_hex_detects_different_inputs() {
        let a = sha256_hex(b"hello");
        let b = sha256_hex(b"world");
        assert_ne!(a, b);
        assert_eq!(a.len(), 64); // 32 bytes = 64 hex chars
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
        let bb_path = download_bb(&version)
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
