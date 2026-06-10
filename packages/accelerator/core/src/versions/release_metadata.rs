//! Aztec release metadata: platform naming, download URLs, the shared HTTP client, and the GitHub
//! asset-digest lookup (SEC-02 caveat inline). q7e3-F-07: split from the `versions` module root; the
//! root re-exports keep external paths unchanged.

use super::version_policy::AztecVersion;
use std::error::Error;
use std::time::Duration;

/// HTTP client with reasonable timeouts for downloading bb binaries and checking digests.
pub(crate) fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(30))
        .user_agent("aztec-accelerator")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
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
/// q7e3-F-08: takes the validated `&AztecVersion` — an unvalidated string can no longer reach this
/// URL-building sink.
pub fn download_url(version: &AztecVersion) -> String {
    format!(
        "https://github.com/AztecProtocol/aztec-packages/releases/download/v{}/barretenberg-{}.tar.gz",
        version,
        current_platform(),
    )
}

/// Compute SHA-256 hex digest of the given bytes.
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(data))
}

/// Fetch the expected SHA-256 digest for a release asset from the GitHub API.
///
/// GitHub stores a `digest` field (e.g. `"sha256:abcd..."`) on every release asset. This catches
/// download corruption and CDN issues.
///
/// SECURITY (SEC-02, deferred — circular trust): the digest is fetched from the SAME GitHub control
/// plane (`api.github.com`) that serves the binary, so an attacker who compromises the upstream
/// release (account/CI) — or MITMs both endpoints — can serve a malicious `bb` tarball AND a matching
/// digest; the check passes and the binary is installed + executed. A pure network MITM is blocked
/// (both hops are HTTPS), but supply-chain compromise is NOT. The real fix is verifying an UPSTREAM
/// PUBLISHER SIGNATURE pinned in the shipped app (minisign/cosign/TUF), the way our own auto-updater
/// already does — but Aztec does not yet sign `bb` releases. Pinning known-good digests in the app is
/// NOT a workaround: barretenberg nightlies ship EVERY night, so a pinned-digest manifest would be
/// perpetually stale. Revisit once Aztec signs `bb`.
/// Tracking: `implementations-plan/security-hardening-2026-06-09` (SEC-02) + a GitHub issue.
pub(crate) async fn fetch_github_asset_digest(
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_url_format() {
        let version = AztecVersion::parse("5.0.0-nightly.20260307").unwrap();
        let url = download_url(&version);
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
        let version = AztecVersion::parse(&version).expect("test version is valid");
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
}
