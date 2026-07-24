//! Version validation, the `AztecVersion` value object, tier classification, and retention/eviction
//! policy. q7e3-F-07: split from the `versions` module root; the root re-exports keep external paths
//! unchanged.

use super::cache_layout::{list_cached_versions, versions_base_dir};

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

/// Older bb versions that are BELOW the bundled floor yet explicitly vetted-safe, so a remote
/// `x-aztec-version` request for one is still honoured. **EMPTY by default** — the app owner curates
/// this list after confirming each entry has no known proving-soundness or security defect. It is a
/// COMPILE-TIME constant on purpose: a remote dApp must never be able to widen the set of selectable
/// versions at runtime (that would defeat the whole downgrade gate). See CONVERGED-SCOPE "NEEDS THE
/// OWNER — vetted bb-version policy content".
pub const VETTED_OLDER_VERSIONS: &[&str] = &[];

/// Why a remote-requested version was refused by [`check_version_selectable`]. Carried into the 403
/// body + a `warn!` so a legitimate integrator can see WHY their pin was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionRejection {
    /// Passed the `is_valid_version` charset gate but is not a strict semver — a syntactic alias
    /// (`latest`, `5`, `5.0`, `5.0.0-alpha_beta`, …). Rejected so only well-formed versions are ever
    /// floored/compared.
    NotSemver,
    /// Carries `+build` metadata. SemVer precedence IGNORES build metadata, so two builds with equal
    /// precedence are distinct release identities we cannot reason about — refuse them (codex audit #3).
    /// NOTE: over HTTP this is unreachable — `is_valid_version` rejects `+` first (400 `invalid_version`);
    /// this arm is for direct/future callers and keeps the policy self-contained (codex r2 #7).
    HasBuildMetadata,
    /// Not strictly newer than the bundled baseline (by SemVer *precedence*) — a downgrade/sidegrade to
    /// a potentially-vulnerable bb. Blocked.
    BelowFloor,
    /// A prerelease on a channel the bundled baseline is NOT on (e.g. a `nightly`/`devnet` dev build, or
    /// any prerelease requested against a stable bundled). Only stable, or the bundled's own prerelease
    /// channel, is a valid forward target.
    ChannelNotAllowed,
}

impl VersionRejection {
    /// Short, stable reason string for logs + the error body.
    pub fn reason(self) -> &'static str {
        match self {
            Self::NotSemver => "not a strict semver version",
            Self::HasBuildMetadata => "carries build metadata (ambiguous precedence)",
            Self::BelowFloor => "not strictly newer than the bundled version (downgrade refused)",
            Self::ChannelNotAllowed => "prerelease channel not selectable for this release",
        }
    }
}

/// The release "channel" of a version, for the forward-target rule. `Stable` = no prerelease; a
/// prerelease's channel is its FIRST dot-separated prerelease identifier (`rc`, `nightly`, `devnet`,
/// `aztecnr-rc`, …). Compared verbatim (case-sensitive) so `RC` ≠ `rc`.
fn channel(v: &semver::Version) -> Option<&str> {
    if v.pre.is_empty() {
        None // stable
    } else {
        Some(v.pre.as_str().split('.').next().unwrap_or(v.pre.as_str()))
    }
}

/// Gate a REMOTE `x-aztec-version` request against a safe-default downgrade policy.
///
/// `x-aztec-version` is remote-controlled, and any requested version is downloaded AUTHENTICALLY
/// (GitHub asset digest-checked). The residual risk is therefore NOT a tampered binary but a remote
/// dApp forcing a *downgrade* to an authentic-but-known-vulnerable old bb, or coercing an arbitrary
/// nightly/devnet dev build. The caller MUST have already normalized an exact-bundled request to the
/// sidecar path — this function only ever sees NON-bundled requests.
///
/// Policy (safe default — every selectable version must already be safe; refined with codex #3 / r2 #2):
/// 1. An explicitly owner-vetted older version ([`VETTED_OLDER_VERSIONS`]) is always allowed.
/// 2. If the BUNDLED baseline does not parse as semver, there is no floor to enforce (the normal
///    headless case — no shipped bb to downgrade FROM), so allow: the request is already traversal-
///    validated and the download is digest-verified. Desktop always has a compile-time baseline.
/// 3. The request must parse as STRICT semver (rejects syntactic aliases the looser [`is_valid_version`]
///    charset gate lets through) and carry no `+build` metadata (ambiguous precedence).
/// 4. Floor: the request must be STRICTLY NEWER than bundled by SemVer *precedence* (`cmp_precedence`,
///    which ignores build metadata — not Rust's total `Ord`). This is the downgrade block. Note SemVer
///    precedence is a label order, not a chronology/safety order, hence rules 1 & 5 on top of it.
/// 5. Forward target: the request must be stable, OR share the bundled baseline's exact prerelease
///    channel. This drops nightly/devnet dev builds, unknown prerelease channels, and stable→prerelease
///    unless the shipped baseline is itself on that channel.
pub fn check_version_selectable(requested: &str, bundled: &str) -> Result<(), VersionRejection> {
    use std::cmp::Ordering;

    // 1. Owner allowlist wins outright (may be below the floor by design).
    if VETTED_OLDER_VERSIONS.contains(&requested) {
        return Ok(());
    }
    // 2. Validate the REQUEST unconditionally (codex r2 #2 / r3 #2): strict semver + no `+build`
    //    metadata. These do NOT depend on the baseline, so they must run even when the bundled version
    //    is unknown — otherwise a headless server (no `AZTEC_BB_VERSION`) could select a syntactic alias
    //    (`latest`) or a build-metadata version. A real SDK always pins a strict semver, so this only
    //    rejects garbage earlier.
    let req = semver::Version::parse(requested).map_err(|_| VersionRejection::NotSemver)?;
    if !req.build.is_empty() {
        return Err(VersionRejection::HasBuildMetadata);
    }
    // 3. No parseable bundled baseline ⇒ NO floor/channel to compare against. This is the normal
    //    headless case (no shipped bb to downgrade FROM), so failing closed would brick its documented
    //    first-request-download mode. The request is already validated above + digest-verified on
    //    download. The DESKTOP app always has a compile-time `AZTEC_BB_VERSION`, so its floor holds below.
    let Ok(base) = semver::Version::parse(bundled) else {
        return Ok(());
    };
    // 4. Strictly-newer by PRECEDENCE (ignores build metadata). Exact-bundled is handled upstream, so
    //    an equal-precedence request here is a sidegrade — refuse it.
    if req.cmp_precedence(&base) != Ordering::Greater {
        return Err(VersionRejection::BelowFloor);
    }
    // 5. Stable, or the bundled baseline's own prerelease channel — nothing else.
    match channel(&req) {
        None => Ok(()), // stable is always a safe forward target
        Some(req_ch) if channel(&base) == Some(req_ch) => Ok(()),
        Some(_) => Err(VersionRejection::ChannelNotAllowed),
    }
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
    for version in evictions(&cached, bundled, in_use) {
        let dir = base.join(version.as_str());
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => tracing::info!(version = %version, "Evicted old bb version"),
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

    // ─── remote version-downgrade policy (codex audit #3) ───────────────────

    #[test]
    fn version_policy_allows_strictly_newer_same_channel_and_stable() {
        // Newer rc, same channel as an rc baseline → allowed (the legit "dApp on a newer testnet" case).
        assert_eq!(check_version_selectable("5.0.0-rc.5", "5.0.0-rc.2"), Ok(()));
        assert_eq!(check_version_selectable("5.1.0-rc.1", "5.0.0-rc.2"), Ok(()));
        // Stable is always a safe forward target above the floor, whatever the baseline channel.
        assert_eq!(check_version_selectable("5.0.0", "5.0.0-rc.2"), Ok(()));
        assert_eq!(check_version_selectable("5.1.0", "5.0.0"), Ok(()));
    }

    #[test]
    fn version_policy_blocks_downgrade_and_sidegrade() {
        // Older rc, older stable, and the SAME precedence (sidegrade) are all refused.
        assert_eq!(
            check_version_selectable("5.0.0-rc.1", "5.0.0-rc.2"),
            Err(VersionRejection::BelowFloor)
        );
        assert_eq!(
            check_version_selectable("4.9.0", "5.0.0"),
            Err(VersionRejection::BelowFloor)
        );
        // A stable request equals the FINAL release precedence-wise but is strictly greater than its
        // own rc, so this pair specifically checks the equal-precedence sidegrade guard:
        assert_eq!(
            check_version_selectable("5.0.0-rc.2", "5.0.0-rc.2"),
            Err(VersionRejection::BelowFloor),
            "exact-bundled is normalized upstream; reaching here with equal precedence is a sidegrade"
        );
    }

    #[test]
    fn version_policy_blocks_dev_and_unknown_channels() {
        // nightly/devnet dev builds, even when strictly newer, are refused against an rc/stable baseline.
        assert_eq!(
            check_version_selectable("5.1.0-nightly.20260307", "5.0.0-rc.2"),
            Err(VersionRejection::ChannelNotAllowed)
        );
        assert_eq!(
            check_version_selectable("6.0.0-devnet.20260307", "5.0.0"),
            Err(VersionRejection::ChannelNotAllowed)
        );
        // An unknown/foreign prerelease channel above the floor is refused too.
        assert_eq!(
            check_version_selectable("6.0.0-alpha.1", "5.0.0-rc.2"),
            Err(VersionRejection::ChannelNotAllowed)
        );
        // …but if the baseline is ITSELF a nightly dev build, a newer nightly on the same channel is fine.
        assert_eq!(
            check_version_selectable("5.0.0-nightly.20260310", "5.0.0-nightly.20260307"),
            Ok(())
        );
    }

    #[test]
    fn version_policy_rejects_aliases_and_build_metadata() {
        // Syntactic aliases that pass is_valid_version's charset gate but are not strict semver.
        for alias in ["latest", "5", "5.0", "5.0.0-alpha_beta"] {
            assert_eq!(
                check_version_selectable(alias, "5.0.0-rc.2"),
                Err(VersionRejection::NotSemver),
                "alias {alias:?} must be rejected"
            );
        }
        // Build metadata is refused (ambiguous precedence) even when otherwise newer.
        assert_eq!(
            check_version_selectable("5.1.0+build9", "5.0.0-rc.2"),
            Err(VersionRejection::HasBuildMetadata)
        );
    }

    #[test]
    fn version_policy_unknown_bundled_has_no_floor_but_still_validates_request() {
        // codex r2 #2: a headless server without AZTEC_BB_VERSION has bundled = "unknown" (unparseable).
        // There is no shipped bb to downgrade FROM, so the policy imposes NO FLOOR/CHANNEL — otherwise
        // the documented first-request-download mode would be bricked (every real version → 403). Any
        // strict-semver version (even a nightly / an older one) is allowed.
        assert_eq!(check_version_selectable("5.0.0", "unknown"), Ok(()));
        assert_eq!(check_version_selectable("4.0.0", "unknown"), Ok(()));
        assert_eq!(
            check_version_selectable("5.0.0-nightly.20260307", "unknown"),
            Ok(())
        );
        // codex r3 #2: but the REQUEST is still validated even without a baseline — aliases and build
        // metadata are rejected (a headless caller can't select `latest` or a `+build` version).
        assert_eq!(
            check_version_selectable("latest", "unknown"),
            Err(VersionRejection::NotSemver)
        );
        assert_eq!(
            check_version_selectable("5.0.0+build9", "unknown"),
            Err(VersionRejection::HasBuildMetadata)
        );
    }

    #[test]
    fn version_policy_allowlist_overrides_floor() {
        // The default allowlist is empty, so an older version is refused…
        assert!(VETTED_OLDER_VERSIONS.is_empty());
        assert_eq!(
            check_version_selectable("4.0.0", "5.0.0"),
            Err(VersionRejection::BelowFloor)
        );
        // …and every allowlist entry (if the owner adds any later) must itself be strict semver so the
        // exact-match compare is meaningful.
        for v in VETTED_OLDER_VERSIONS {
            assert!(
                semver::Version::parse(v).is_ok(),
                "VETTED_OLDER_VERSIONS entry {v:?} must be strict semver"
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
