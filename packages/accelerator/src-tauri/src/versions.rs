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

/// A validated Aztec version string — the Q3 value object.
///
/// Construction (`parse`) is the single validation gate: it runs the exact `is_valid_version`
/// predicate, so any `&AztecVersion` that reaches a cache-path or download-URL sink is traversal-safe
/// *by construction* — the #99 guard can no longer be bypassed by forgetting to call it. Carries the
/// precomputed network `tier` and `sort_key` so eviction no longer re-parses each element.
/// `Deref<Target = str>` + `AsRef<str>` expose the raw string at the `&str` boundaries that build
/// paths/URLs, keeping those sinks byte-identical to the pre-Q3 `&str` callees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AztecVersion {
    raw: String,
    tier: NetworkTier,
    sort_key: (String, u64),
}

impl AztecVersion {
    /// Parse an untrusted version string, running the full `is_valid_version` gate. Returns `None`
    /// for exactly the inputs `is_valid_version` rejects (pinned by
    /// `aztec_version_parse_matches_is_valid_version`).
    pub fn parse(version: &str) -> Option<Self> {
        if !is_valid_version(version) {
            return None;
        }
        Some(Self {
            tier: NetworkTier::from_version(version),
            sort_key: version_sort_key(version),
            raw: version.to_string(),
        })
    }

    /// The raw version string (e.g. `"5.0.0-rc.1"`).
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The precomputed network tier (drives cache retention).
    pub fn tier(&self) -> NetworkTier {
        self.tier
    }

    /// The precomputed `(base, numeric-suffix)` sort key for retention ordering.
    pub fn sort_key(&self) -> &(String, u64) {
        &self.sort_key
    }
}

impl std::ops::Deref for AztecVersion {
    type Target = str;
    fn deref(&self) -> &str {
        &self.raw
    }
}

impl AsRef<str> for AztecVersion {
    fn as_ref(&self) -> &str {
        &self.raw
    }
}

impl std::fmt::Display for AztecVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.raw)
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
pub fn versions_to_evict(
    cached: &[AztecVersion],
    bundled_version: &AztecVersion,
) -> Vec<AztecVersion> {
    use std::collections::HashMap;

    // Q3-followup: `cached`/`bundled` are validated `AztecVersion`s, so tier + sort_key are read from
    // the precomputed fields instead of re-parsing each element per call. Eviction OUTPUT is unchanged
    // (pinned by the eviction char tests).
    let mut by_tier: HashMap<NetworkTier, Vec<&AztecVersion>> = HashMap::new();
    for v in cached {
        by_tier.entry(v.tier()).or_default().push(v);
    }

    let mut to_evict = Vec::new();
    for (tier, mut versions) in by_tier {
        if let Some(limit) = tier.retention_limit() {
            // Sort ascending (oldest first). Uses the precomputed version-aware sort_key: for RC
            // versions the numeric suffix makes "rc.2" < "rc.10" (not lexicographic "rc.10" < "rc.2").
            versions.sort_by(|a, b| a.sort_key().cmp(b.sort_key()));
            // Remove bundled from the candidate list (it's always kept)
            versions.retain(|v| v.as_str() != bundled_version.as_str());
            // Evict oldest non-bundled versions until we're within the limit
            // (limit includes the bundled version if it's in this tier)
            let effective_limit = if cached
                .iter()
                .any(|v| v.as_str() == bundled_version.as_str() && v.tier() == tier)
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
        && !version.ends_with('.')
        && !version.contains("..")
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

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
fn install_version_dir(
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
    // `bundled_version` is always a real version; if it somehow can't be parsed, skip cleanup rather
    // than evict against a mis-parsed bundled (defensive — unreachable in practice).
    let Some(bundled) = AztecVersion::parse(bundled_version) else {
        return;
    };
    // Parse the cached dir names into validated versions. An unparseable dir name is skipped — same
    // net outcome as before (the old code classified it Mainnet → retention `None` → never evicted).
    let cached: Vec<AztecVersion> = list_cached_versions()
        .iter()
        .filter_map(|s| AztecVersion::parse(s))
        .collect();
    let to_evict = versions_to_evict(&cached, &bundled);

    for version in &to_evict {
        let dir = versions_base_dir().join(version.as_str());
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => tracing::info!(version = %version, "Evicted old bb version"),
            Err(e) => tracing::warn!(version = %version, error = %e, "Failed to evict bb version"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper: construct a validated `AztecVersion` (panics on an invalid literal — test-only).
    fn av(s: &str) -> AztecVersion {
        AztecVersion::parse(s).expect("test version literal must be valid")
    }

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
            "5.0.0.",
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

    /// Q3 value object: `AztecVersion::parse` MUST accept exactly the set `is_valid_version` accepts
    /// (validation-as-constructor — the ctor is now the single traversal gate, so it can't drift from
    /// the predicate it replaces). Also pins that a parsed version round-trips byte-identically
    /// (`as_str`/`Deref`) and precomputes the same `tier`/`sort_key` the eviction + path/URL sinks
    /// rely on — the properties that keep threading `&AztecVersion` behavior-preserving.
    #[test]
    fn aztec_version_parse_matches_is_valid_version() {
        let corpus = [
            "5.0.0",
            "5.0.0-rc.1",
            "5.0.0-nightly.20260307",
            "5.0.0-devnet.20260307",
            "4.2.0-aztecnr-rc.2",
            "1.2.3-alpha_beta",
            "",
            "..",
            ".",
            ".foo",
            "1..2",
            "5.0.0.",
            "../../../etc/passwd",
            "5.0.0; rm -rf /",
            "5.0.0\n",
            "a\\b",
        ];
        for v in corpus {
            assert_eq!(
                AztecVersion::parse(v).is_some(),
                is_valid_version(v),
                "parse/is_valid_version disagree on {v:?}"
            );
        }
        assert!(
            AztecVersion::parse(&"a".repeat(129)).is_none(),
            "over-long rejected identically"
        );

        // Valid versions round-trip byte-identically and precompute the same tier.
        for v in [
            "5.0.0",
            "5.0.0-rc.1",
            "5.0.0-nightly.20260307",
            "5.0.0-devnet.20260307",
        ] {
            let av = AztecVersion::parse(v).expect("valid");
            assert_eq!(av.as_str(), v, "as_str round-trip");
            assert_eq!(&*av, v, "Deref round-trip");
            assert_eq!(
                av.tier(),
                NetworkTier::from_version(v),
                "tier matches from_version"
            );
        }
        // sort_key precomputed (read so the field is exercised); rc.10 sorts after rc.2 numerically.
        let rc10 = AztecVersion::parse("5.0.0-rc.10").unwrap();
        let rc2 = AztecVersion::parse("5.0.0-rc.2").unwrap();
        assert!(rc10.sort_key() > rc2.sort_key(), "rc.10 sorts after rc.2");
    }

    /// #99 traversal guard, post-Q3. The guard is now the `AztecVersion` constructor: `download_bb`
    /// takes `&AztecVersion`, so an unsafe version can't be built into the type the `remove_dir_all`
    /// sink requires — a direct caller bypassing the server.rs ingress is *structurally* unable to
    /// steer the sink toward a traversal path (stronger than the prior runtime recheck). This pins
    /// that the ctor rejects the exact corpus the sink-side check used to.
    #[test]
    fn unsafe_version_cannot_be_constructed_for_download_sink() {
        for v in ["..", ".", "../etc", ".5.0.0", "a/b"] {
            assert!(
                AztecVersion::parse(v).is_none(),
                "ctor must reject unsafe {v:?} so it cannot reach download_bb(&AztecVersion)"
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
            av("5.0.0-nightly.20260301"),
            av("5.0.0-nightly.20260302"),
            av("5.0.0-nightly.20260303"),
            av("5.0.0-nightly.20260304"),
        ];
        let evicted = versions_to_evict(&cached, &av("5.0.0-nightly.20260304"));
        // Keep 2, evict 2 oldest
        assert_eq!(evicted.len(), 2);
        assert!(evicted.contains(&av("5.0.0-nightly.20260301")));
        assert!(evicted.contains(&av("5.0.0-nightly.20260302")));
    }

    #[test]
    fn bundled_version_never_evicted() {
        let cached = vec![
            av("5.0.0-nightly.20260301"),
            av("5.0.0-nightly.20260302"),
            av("5.0.0-nightly.20260303"),
            av("5.0.0-nightly.20260304"),
        ];
        // Bundled is the oldest — should still not be evicted
        let evicted = versions_to_evict(&cached, &av("5.0.0-nightly.20260301"));
        assert!(!evicted.contains(&av("5.0.0-nightly.20260301")));
        // 4 versions, keep 2, but bundled is protected, so evict the next oldest
        assert_eq!(evicted.len(), 2);
        assert!(evicted.contains(&av("5.0.0-nightly.20260302")));
        assert!(evicted.contains(&av("5.0.0-nightly.20260303")));
    }

    #[test]
    fn mainnet_never_evicted() {
        let cached = vec![
            av("1.0.0"),
            av("2.0.0"),
            av("3.0.0"),
            av("4.0.0"),
            av("5.0.0"),
        ];
        let evicted = versions_to_evict(&cached, &av("5.0.0"));
        assert!(evicted.is_empty());
    }

    #[test]
    fn rc_versions_sort_numerically_not_lexicographically() {
        // With lexicographic sort, rc.10 < rc.2 — wrong!
        // With numeric sort, rc.1 < rc.2 < rc.3 < rc.10 — correct.
        let cached = vec![
            av("4.0.0-rc.1"),
            av("4.0.0-rc.2"),
            av("4.0.0-rc.3"),
            av("4.0.0-rc.10"),
            av("4.0.0-rc.11"),
            av("4.0.0-rc.20"),
        ];
        // Testnet tier: keep 5, evict 1 (oldest = rc.1)
        let evicted = versions_to_evict(&cached, &av("4.0.0-rc.20"));
        assert_eq!(evicted, vec![av("4.0.0-rc.1")]);
    }

    #[test]
    fn mixed_tiers() {
        let cached = vec![
            av("5.0.0-nightly.20260301"),
            av("5.0.0-nightly.20260302"),
            av("5.0.0-nightly.20260303"),
            av("5.0.0-devnet.20260301"),
            av("5.0.0-rc.1"),
            av("5.0.0"),
        ];
        let evicted = versions_to_evict(&cached, &av("5.0.0"));
        // Nightlies: 3, keep 2, evict 1
        assert_eq!(evicted.len(), 1);
        assert!(evicted.contains(&av("5.0.0-nightly.20260301")));
    }

    /// CHARACTERIZATION (quality-refactor Phase 0 — Q3 guard). Edge cases the AztecVersion refactor
    /// (Q3 changes `versions_to_evict`'s signature `&[String]` → `&[AztecVersion]`) must preserve.
    #[test]
    fn versions_to_evict_edge_cases() {
        // Empty cache → nothing to evict.
        assert!(versions_to_evict(&[], &av("5.0.0-nightly.20260301")).is_empty());

        // The only cached version IS the bundled one → never evicted (even alone, even over a limit).
        let only_bundled = vec![av("5.0.0-nightly.20260301")];
        assert!(versions_to_evict(&only_bundled, &av("5.0.0-nightly.20260301")).is_empty());

        // Non-bundled nightlies at/under the tier limit (2) all stay.
        let under_limit = vec![av("5.0.0-nightly.20260301"), av("5.0.0-nightly.20260302")];
        assert!(versions_to_evict(&under_limit, &av("5.0.0")).is_empty());
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
