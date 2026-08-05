//! Version validation, the `AztecVersion` value object, tier classification, and retention/eviction
//! policy. q7e3-F-07: split from the `versions` module root; the root re-exports keep external paths
//! unchanged.

use super::cache_layout::{list_cached_versions, version_dir_size, versions_base_dir};

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

/// Hard ceiling on the TOTAL size of `~/.aztec-accelerator/versions/` (F-06, audit 2026-07-31).
///
/// The per-tier retention limits above are a *count* policy, and `Mainnet` has none at all —
/// `retention_limit()` returns `None`, "keep all". Every stable Aztec release is Mainnet, and so is
/// every version whose prerelease suffix we do not recognise, because [`NetworkTier::from_version`]
/// falls through to it. `x-aztec-version` is an ordinary request header, so an approved origin could
/// name a new Mainnet-classified version per request and each one that resolves to a real Aztec
/// release downloads another ~37 MB `bb` that NOTHING would ever evict — the cache grew without
/// bound and the only limit was the disk.
///
/// A total-size cap is the honest control here: it binds regardless of tier, needs no notion of
/// "accepted" versions, and does not care how a version got classified. 2 GiB is roughly 55 cached
/// binaries — far past any real workflow, and small enough that filling it is not a disk-exhaustion
/// event.
pub const CACHE_MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Which versions to evict to bring the cache back under `max_total`, given each candidate's on-disk
/// size. Oldest first.
///
/// `sized` is `(version, bytes)` for everything currently cached. Two versions are pinned — the
/// BUNDLED one (it is what the app falls back TO) and the IN-USE one (deleting the binary a
/// concurrent `bb::prove` is about to execute turns a fail-closed `find_bb` into a spurious hard
/// error; same guard as [`evictions`]). Both still COUNT toward the total, because they really do
/// occupy the disk — pinning them means the cap may be missed, never that it is miscounted. Sorting
/// uses the same version-aware `sort_key` as the tier policy, so "rc.2" precedes "rc.10".
pub fn versions_to_evict_for_size(
    sized: &[(AztecVersion, u64)],
    bundled: &AztecVersion,
    in_use: Option<&AztecVersion>,
    max_total: u64,
) -> Vec<AztecVersion> {
    let mut total: u64 = sized
        .iter()
        .fold(0u64, |acc, (_, n)| acc.saturating_add(*n));
    if total <= max_total {
        return Vec::new();
    }
    let pinned = |v: &AztecVersion| {
        v.as_str() == bundled.as_str() || in_use.is_some_and(|u| u.as_str() == v.as_str())
    };
    let mut candidates: Vec<&(AztecVersion, u64)> =
        sized.iter().filter(|(v, _)| !pinned(v)).collect();
    candidates.sort_by(|a, b| a.0.sort_key().cmp(b.0.sort_key()));

    let mut to_evict = Vec::new();
    for (version, bytes) in candidates {
        if total <= max_total {
            break;
        }
        total = total.saturating_sub(*bytes);
        to_evict.push(version.clone());
    }
    to_evict
}

/// Aztec versions whose bundled `bb` is KNOWN to be vulnerable — a REVOCATION list, **EMPTY by default**.
///
/// `x-aztec-version` carries the **Aztec** version (the SDK's `@aztec/stdlib` dependency version), NOT a
/// barretenberg/`bb` version — and many Aztec releases ship the *same* `bb`. So a "newer-is-safer" floor
/// keyed on this string would wrongly reject a legitimate older-but-compatible dApp (e.g. an Aztec 5.0.1
/// app talking to a 5.1.0-bundled accelerator). Instead this is a targeted denylist: the app owner adds a
/// version string here ONLY if a security defect is found in the `bb` shipped with that Aztec release,
/// and a remote request for it is then refused (403). Empty ⇒ no restriction — any version the dApp
/// requests is allowed, and it is still digest-verified against Aztec's published hash on download (so it
/// is always an AUTHENTIC Aztec `bb`). COMPILE-TIME so a remote dApp cannot change it. Entries must be the
/// exact wire version string the SDK sends.
pub const KNOWN_VULNERABLE_VERSIONS: &[&str] = &[];

/// Why a remote-requested version was refused by [`check_version_selectable`]. Carried into the 403
/// body + a `warn!` so a legitimate integrator sees WHY their pin was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionRejection {
    /// Passed the `is_valid_version` charset gate but is not a canonical strict semver — a syntactic
    /// alias (`latest`, `5`, `5.0`) or a non-canonical spelling. Rejected so only well-formed versions
    /// are ever fetched or exact-matched against the revocation list (codex denylist-review #2).
    NotSemver,
    /// Carries `+build` metadata. Two builds with equal SemVer precedence are distinct release
    /// identities we can't reason about — refuse them.
    HasBuildMetadata,
    /// The requested Aztec version is on the [`KNOWN_VULNERABLE_VERSIONS`] revocation list.
    KnownVulnerable,
}

impl VersionRejection {
    /// Short, stable reason string for logs + the error body.
    pub fn reason(self) -> &'static str {
        match self {
            Self::NotSemver => "not a canonical strict semver version",
            Self::HasBuildMetadata => "carries build metadata (ambiguous release identity)",
            Self::KnownVulnerable => "on the known-vulnerable revocation list",
        }
    }
}

/// Gate a REMOTE `x-aztec-version` request.
///
/// The header is the **Aztec** version, not a `bb` version, and the accelerator downloads the `bb`
/// tarball attached to the Aztec release of that exact version. We therefore CANNOT impose a
/// "newer-is-safer" floor here — many Aztec versions share one `bb`, so a floor would break a legitimate
/// older-but-compatible dApp (this was the over-blocking bug this function replaces). Every download is
/// already digest-verified against Aztec's published hash, so a requested version is always an authentic
/// Aztec `bb`. The only residual risk — an approved dApp pinning an Aztec version whose bundled `bb` is
/// *later* found vulnerable — is handled REACTIVELY via the [`KNOWN_VULNERABLE_VERSIONS`] revocation list
/// (empty by default). Everything not on that list is allowed.
///
/// Well-formedness is STILL enforced (codex denylist-review #2): the request must be canonical strict
/// semver with no build metadata. This rejects aliases (`latest`) the looser `is_valid_version` charset
/// gate lets through, and keeps the exact-string revocation match meaningful.
///
/// (A precise FUTURE gate could run `bb --version` on the downloaded binary to obtain the true
/// barretenberg build id — distinct from the Aztec/npm version — and denylist on THAT, or ship a signed
/// updateable revocation manifest. Larger cross-package changes, tracked separately.)
pub fn check_version_selectable(requested: &str) -> Result<(), VersionRejection> {
    check_version_against(requested, KNOWN_VULNERABLE_VERSIONS)
}

/// Inner, denylist-parameterized core of [`check_version_selectable`] — unit-testable against an
/// arbitrary revocation list (the production const is empty). Validates well-formedness FIRST, then the
/// revocation match.
fn check_version_against(requested: &str, denylist: &[&str]) -> Result<(), VersionRejection> {
    // Canonical strict semver, no build metadata — same well-formedness the earlier gate enforced.
    let v = semver::Version::parse(requested).map_err(|_| VersionRejection::NotSemver)?;
    if v.to_string() != requested {
        return Err(VersionRejection::NotSemver); // non-canonical spelling
    }
    if !v.build.is_empty() {
        return Err(VersionRejection::HasBuildMetadata);
    }
    if denylist.contains(&requested) {
        return Err(VersionRejection::KnownVulnerable);
    }
    Ok(())
}

/// Validate a version string before it is used to build cache paths or download URLs. THE single
/// source of truth for both the HTTP ingress (`server::prove::resolve_version`) and the `download_bb`
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

/// Clean up old cached versions per the retention policy.
/// q7e3-F-08: takes the validated `&AztecVersion` (callers parse; an unparseable bundled skips
/// cleanup at the call site — same defensive outcome as the old internal parse-else-return).
pub async fn cleanup_old_versions(bundled: &AztecVersion, in_use: Option<&AztecVersion>) {
    // Parse the cached dir names into validated versions. An unparseable dir name is skipped — same
    // net outcome as before (the old code classified it Mainnet → retention `None` → never evicted).
    let cached: Vec<AztecVersion> = list_cached_versions()
        .iter()
        .filter_map(|s| AztecVersion::parse(s))
        .collect();
    // codex #4: no resolvable home ⇒ no trusted cache root ⇒ nothing to evict (and never join onto a
    // CWD fallback). `list_cached_versions` is already empty in this case, but guard the base dir too.
    let Some(base) = versions_base_dir() else {
        return;
    };
    // Tier-count evictions first, then whatever the SIZE cap still demands of the survivors. Order
    // matters: the count policy is the cheaper, more targeted one, and doing it first means the size
    // cap usually finds nothing left to do.
    let mut planned = evictions(&cached, bundled, in_use);
    let evicted_by_count: std::collections::HashSet<String> =
        planned.iter().map(|v| v.as_str().to_string()).collect();
    let survivors: Vec<(AztecVersion, u64)> = cached
        .iter()
        .filter(|v| !evicted_by_count.contains(v.as_str()))
        .map(|v| (v.clone(), version_dir_size(&base.join(v.as_str()))))
        .collect();
    for version in versions_to_evict_for_size(&survivors, bundled, in_use, CACHE_MAX_TOTAL_BYTES) {
        tracing::warn!(version = %version, "Evicting to stay under the cache size cap");
        planned.push(version);
    }

    let mut deferred_by_active_window = false;
    for version in planned {
        let dir = base.join(version.as_str());
        // B2 (full-branch audit): skip a version whose dir was touched within the active window — it was
        // just downloaded (and is likely about to be proved by a CONCURRENT request whose own cleanup
        // exempts a DIFFERENT in_use version). Age-gating stops two concurrent detached cleanups from
        // evicting each other's fresh-in-use binary. A truly-old in-use version is still a narrow,
        // recoverable (re-download) TOCTOU — a full cross-request lease is deferred (see FINDINGS.md B2).
        //
        // F-06 note: this exemption applies to the SIZE-cap evictions too, so a burst of downloads can
        // transiently overshoot `CACHE_MAX_TOTAL_BYTES` by whatever lands inside one active window —
        // everything in it is "recently active" and therefore unevictable. Correctness wins here:
        // overriding the guard would let one request delete the binary another is about to execute. The
        // overshoot is bounded by the burst rate and reclaimed by the next cleanup, which is a different
        // thing from the unbounded growth this cap exists to stop.
        if super::downloader::recently_active(&dir) {
            tracing::debug!(version = %version, "Skipping eviction of a recently-active version");
            deferred_by_active_window = true;
            continue;
        }
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => tracing::info!(version = %version, "Evicted old bb version"),
            Err(e) => tracing::warn!(version = %version, error = %e, "Failed to evict bb version"),
        }
    }

    // Round 2 (codex pass over F-06): if the active-window guard deferred anything, this pass may have
    // left the cache OVER the cap — and cleanup only runs after a download, so an attacker who fills
    // the cache inside one window and then simply STOPS leaves the excess there indefinitely. That
    // falsifies the "reclaimed by the next cleanup" reasoning this fix originally shipped with. Wait
    // out the window once and finish the job; the caller already runs us detached.
    if deferred_by_active_window {
        tokio::time::sleep(super::downloader::CACHE_ENTRY_ACTIVE_WINDOW).await;
        Box::pin(cleanup_after_active_window(bundled, in_use)).await;
    }
}

/// The retry half of the above, split out so the recursion is explicit and can only happen once:
/// this pass does NOT re-arm the timer, so a cache that is still over the cap waits for the next
/// download rather than looping.
async fn cleanup_after_active_window(bundled: &AztecVersion, in_use: Option<&AztecVersion>) {
    let cached: Vec<AztecVersion> = list_cached_versions()
        .iter()
        .filter_map(|s| AztecVersion::parse(s))
        .collect();
    let Some(base) = versions_base_dir() else {
        return;
    };
    let sized: Vec<(AztecVersion, u64)> = cached
        .iter()
        .map(|v| (v.clone(), version_dir_size(&base.join(v.as_str()))))
        .collect();
    for version in versions_to_evict_for_size(&sized, bundled, in_use, CACHE_MAX_TOTAL_BYTES) {
        let dir = base.join(version.as_str());
        if super::downloader::recently_active(&dir) {
            continue;
        }
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {
                tracing::info!(version = %version, "Evicted to stay under the cache size cap (deferred pass)")
            }
            Err(e) => tracing::warn!(version = %version, error = %e, "Failed to evict bb version"),
        }
    }
}

/// The versions to actually delete: the retention-policy evictions MINUS the in-use version.
///
/// F-007 race guard: `cleanup_old_versions` is spawned (detached) right after a download and races
/// `bb::prove`. Without this, a freshly-downloaded old nightly (the oldest excess in its tier) could be
/// deleted before it executes, turning the now-fail-closed `find_bb` into a spurious hard error. Keeping
/// the in-use version one round over the limit is harmless — the next cleanup (when it is no longer in
/// use) evicts it.
fn evictions(
    cached: &[AztecVersion],
    bundled: &AztecVersion,
    in_use: Option<&AztecVersion>,
) -> Vec<AztecVersion> {
    versions_to_evict(cached, bundled)
        .into_iter()
        .filter(|v| !in_use.is_some_and(|u| u.as_str() == v.as_str()))
        .collect()
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

    // ── F-06 regression (audit 2026-07-31) ─────────────────────────────────────────────────────
    // `x-aztec-version` is an ordinary request header, and `NetworkTier::from_version` routes every
    // stable release — AND every prerelease suffix it does not recognise — to `Mainnet`, whose
    // `retention_limit()` is `None` ("keep all"). So the count policy below never evicted them: each
    // distinct version an origin named downloaded another ~37 MB `bb` that NOTHING would ever remove.
    // These fail against the pre-fix code, where no size bound existed anywhere.

    /// 37 MB — one real `bb`, so the arithmetic below reads in units of actual cached binaries.
    const BB: u64 = 37 * 1024 * 1024;

    /// The tier policy leaves every one of these in place forever; the size cap is what bounds them.
    #[test]
    fn mainnet_versions_grow_without_bound_until_the_size_cap_binds() {
        let versions: Vec<AztecVersion> = (0..60).map(|i| av(&format!("5.0.{i}"))).collect();
        let bundled = av("5.0.0");

        // The count policy declines to evict a single one — this is the defect, pinned.
        assert!(versions_to_evict(&versions, &bundled).is_empty());

        let sized: Vec<(AztecVersion, u64)> = versions.iter().map(|v| (v.clone(), BB)).collect();
        let evicted = versions_to_evict_for_size(&sized, &bundled, None, CACHE_MAX_TOTAL_BYTES);
        assert!(
            !evicted.is_empty(),
            "the size cap must bound an all-Mainnet cache"
        );

        // What survives is genuinely under the cap.
        let remaining: u64 = sized
            .iter()
            .filter(|(v, _)| !evicted.iter().any(|e| e.as_str() == v.as_str()))
            .map(|(_, n)| n)
            .sum();
        assert!(
            remaining <= CACHE_MAX_TOTAL_BYTES,
            "{remaining} bytes left, cap is {CACHE_MAX_TOTAL_BYTES}"
        );
        // Oldest go first, and the bundled version is never one of them.
        assert!(evicted.contains(&av("5.0.1")));
        assert!(!evicted.iter().any(|v| v.as_str() == "5.0.0"));
    }

    /// An unrecognised prerelease suffix also lands in `Mainnet` — so a remote origin does not even
    /// need real releases to name new keep-forever versions. The cap does not care how a version got
    /// classified, which is exactly why it is the right control here.
    #[test]
    fn the_cap_binds_regardless_of_how_a_version_was_classified() {
        assert_eq!(
            NetworkTier::from_version("5.0.0-quux.7"),
            NetworkTier::Mainnet
        );
        let sized = vec![
            (av("5.0.0-quux.1"), BB),
            (av("5.0.0-quux.2"), BB),
            (av("5.0.0-quux.3"), BB),
        ];
        let evicted = versions_to_evict_for_size(&sized, &av("9.9.9"), None, 2 * BB);
        assert_eq!(evicted, vec![av("5.0.0-quux.1")]);
    }

    /// Under the cap, the cap does nothing — it must not fight the tier policy.
    #[test]
    fn a_cache_under_the_cap_is_left_alone() {
        let sized = vec![(av("5.0.0"), BB), (av("5.0.1"), BB)];
        assert!(
            versions_to_evict_for_size(&sized, &av("5.0.0"), None, CACHE_MAX_TOTAL_BYTES)
                .is_empty()
        );
    }

    /// Neither pinned version may be evicted to satisfy the cap — the bundled one is the fallback,
    /// and the in-use one is about to be executed by a concurrent prove (F-007's race).
    #[test]
    fn the_bundled_and_in_use_versions_are_never_evicted_for_size() {
        let sized = vec![
            (av("5.0.0"), BB),
            (av("5.0.1"), BB),
            (av("5.0.2"), BB),
            (av("5.0.3"), BB),
        ];
        // Cap of one binary against four: everything evictable must go, and only those two survive.
        let evicted = versions_to_evict_for_size(&sized, &av("5.0.0"), Some(&av("5.0.1")), BB);
        assert_eq!(evicted, vec![av("5.0.2"), av("5.0.3")]);
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
    fn evictions_exempt_the_in_use_version() {
        // F-007: the version being proved right now must never be evicted by the cleanup that races it.
        let cached = vec![
            av("5.0.0-nightly.20260301"),
            av("5.0.0-nightly.20260302"),
            av("5.0.0-nightly.20260303"),
            av("5.0.0-nightly.20260304"),
        ];
        let bundled = av("4.2.0-aztecnr-rc.2"); // not in this tier

        // No protection: the two oldest are evicted (retention keeps 2).
        let unprotected = evictions(&cached, &bundled, None);
        assert!(unprotected.contains(&av("5.0.0-nightly.20260301")));
        assert!(unprotected.contains(&av("5.0.0-nightly.20260302")));

        // Protecting the OLDEST (the just-downloaded, in-use version): it is exempted, while the next
        // non-protected version is still evicted.
        let in_use = av("5.0.0-nightly.20260301");
        let protected = evictions(&cached, &bundled, Some(&in_use));
        assert!(
            !protected.contains(&in_use),
            "the in-use version must be exempted from eviction"
        );
        assert!(
            protected.contains(&av("5.0.0-nightly.20260302")),
            "a non-protected excess version is still evicted"
        );
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

    // ─── remote version selection: known-vulnerable revocation denylist ─────────

    #[test]
    fn version_policy_allows_everything_by_default() {
        // The header is the Aztec version (not a bb version), and many Aztec releases share one bb, so
        // there is NO floor: any well-formed version a dApp requests is allowed — including OLDER ones —
        // so a legitimate older-but-compatible app is never broken. Default (empty) production denylist.
        for v in [
            "5.0.0",
            "5.1.0",
            "5.0.1", // older than a 5.1.0 bundle — MUST still be allowed (the bug this replaces)
            "5.0.0-rc.1", // older rc
            "5.0.0-nightly.20260307",
            "4.2.0-aztecnr-rc.2",
        ] {
            assert_eq!(check_version_selectable(v), Ok(()), "{v} must be allowed");
        }
    }

    #[test]
    fn version_policy_refuses_a_denylisted_version() {
        // Exercise the REAL implementation (check_version_against) with a simulated revocation list —
        // the production const is empty, so this is how we cover the refusal path (codex review #3).
        let deny = &["5.0.7", "5.1.3-rc.1"];
        assert_eq!(
            check_version_against("5.0.7", deny),
            Err(VersionRejection::KnownVulnerable)
        );
        assert_eq!(
            check_version_against("5.1.3-rc.1", deny),
            Err(VersionRejection::KnownVulnerable)
        );
        // Neighbouring versions are unaffected — the denylist is exact, not a range.
        assert_eq!(check_version_against("5.0.8", deny), Ok(()));
        assert_eq!(check_version_against("5.0.6", deny), Ok(()));
    }

    #[test]
    fn version_policy_rejects_aliases_and_build_metadata() {
        // Well-formedness is still enforced (codex review #2): aliases the charset gate lets through, a
        // non-canonical spelling, and build metadata are all rejected BEFORE any download / revocation
        // match — regardless of the denylist.
        for alias in ["latest", "5", "5.0", "5.0.0-alpha_beta"] {
            assert_eq!(
                check_version_selectable(alias),
                Err(VersionRejection::NotSemver),
                "alias {alias:?} must be rejected"
            );
        }
        assert_eq!(
            check_version_selectable("5.1.0+build9"),
            Err(VersionRejection::HasBuildMetadata)
        );
    }

    #[test]
    fn version_policy_denylist_entries_are_wellformed_if_present() {
        // If the owner ever populates the list, each entry must be a canonical strict semver so it
        // matches the exact wire version the SDK sends. Does NOT assert emptiness (codex review #4: an
        // emptiness assertion would break CI exactly when an emergency revocation is added).
        for v in KNOWN_VULNERABLE_VERSIONS {
            let parsed = semver::Version::parse(v)
                .unwrap_or_else(|_| panic!("KNOWN_VULNERABLE_VERSIONS entry {v:?} must be semver"));
            assert_eq!(
                &parsed.to_string(),
                v,
                "KNOWN_VULNERABLE_VERSIONS entry {v:?} must be canonical"
            );
        }
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
}
