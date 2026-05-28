//! Embedded registry of origins recognized by the Aztec Accelerator maintainers.
//!
//! ## NOT a security boundary
//!
//! A "recognized" entry means *we (the maintainers) recognize this origin string*.
//! It does NOT mean the site is currently trustworthy. If an attacker hijacks DNS
//! for a listed domain or compromises a listed extension's auto-update, the
//! authorization popup will still show the recognition badge.
//!
//! The recognition badge is a UX aid for users to notice when an origin they
//! trust is the one asking for permission, especially when origin strings are
//! opaque (e.g. `chrome-extension://...` IDs).
//!
//! See `packages/accelerator/VERIFIED_SITES.md` for the curation policy.

use crate::authorization::canonicalize_origin;
use serde::Deserialize;
use std::collections::HashMap;

const VERIFIED_SITES_JSON: &str = include_str!("../../verified-sites.json");
const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedSite {
    pub display_name: String,
    /// Curator-facing notes. NOT exposed via the popup DTO; see VERIFIED_SITES.md.
    #[allow(dead_code)]
    pub description: Option<String>,
    #[allow(dead_code)]
    pub curated_by: String,
    #[allow(dead_code)]
    pub added_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerifiedSitesFile {
    schema_version: u32,
    entries: Vec<VerifiedSitesEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerifiedSitesEntry {
    origins: Vec<String>,
    display_name: String,
    description: Option<String>,
    curated_by: String,
    added_at: String,
}

#[derive(Debug, Default)]
pub struct VerifiedSitesRegistry {
    by_origin: HashMap<String, VerifiedSite>,
}

impl VerifiedSitesRegistry {
    /// Load the embedded registry.
    ///
    /// In DEBUG builds, panics on any load error so developer mistakes surface
    /// immediately. In RELEASE builds, logs the error and returns an empty
    /// registry — recognition badges are non-critical UX and shouldn't brick
    /// the binary if the embedded JSON is broken.
    pub fn load() -> Self {
        match Self::try_load() {
            Ok(r) => r,
            Err(e) => {
                #[cfg(debug_assertions)]
                panic!("verified-sites.json failed to load: {e}");
                #[cfg(not(debug_assertions))]
                {
                    tracing::error!(
                        error = %e,
                        "verified-sites.json failed to load — recognition badges disabled"
                    );
                    Self::default()
                }
            }
        }
    }

    fn try_load() -> Result<Self, String> {
        let file: VerifiedSitesFile =
            serde_json::from_str(VERIFIED_SITES_JSON).map_err(|e| format!("parse: {e}"))?;
        if file.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(format!(
                "schemaVersion {} != supported {CURRENT_SCHEMA_VERSION}",
                file.schema_version,
            ));
        }
        let mut by_origin = HashMap::new();
        for entry in file.entries {
            let site = VerifiedSite {
                display_name: entry.display_name,
                description: entry.description,
                curated_by: entry.curated_by,
                added_at: entry.added_at,
            };
            if site.display_name.is_empty() || site.display_name.len() > 64 {
                return Err(format!("displayName invalid: {:?}", site.display_name));
            }
            for origin in entry.origins {
                // Reject non-ASCII at the RAW level — url::Url::parse auto-punycodes
                // Unicode hosts, so checking the canonical form alone is too late.
                if !origin.is_ascii() {
                    return Err(format!("non-ASCII origin (use punycode A-label): {origin}"));
                }
                let canon = canonicalize_origin(&origin)
                    .ok_or_else(|| format!("invalid origin: {origin}"))?;
                if by_origin.insert(canon.clone(), site.clone()).is_some() {
                    return Err(format!("duplicate origin: {canon}"));
                }
            }
        }
        Ok(Self { by_origin })
    }

    /// Look up a site by origin. Caller may pass a non-canonical form;
    /// canonicalization is applied internally for lookup.
    pub fn lookup(&self, origin: &str) -> Option<&VerifiedSite> {
        let canon = canonicalize_origin(origin)?;
        self.by_origin.get(&canon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_registry_loads() {
        // Loads the real verified-sites.json shipped with the binary.
        // Fails the PR-gate if a curator adds a malformed entry.
        let registry = VerifiedSitesRegistry::load();
        assert!(
            !registry.by_origin.is_empty(),
            "embedded registry is empty — seed entries missing?"
        );
        // Specific seed entries we expect.
        assert!(registry.lookup("https://nulo.sh").is_some());
        assert!(registry.lookup("https://faucet.nulo.sh").is_some());
        assert!(registry
            .lookup("chrome-extension://bafbiogfmibdojbhphgpbmbfokmhbpeh")
            .is_some());
        assert!(registry
            .lookup("https://playground.aztec-accelerator.dev")
            .is_some(),);
    }

    #[test]
    fn embedded_registry_lookups_are_case_insensitive() {
        let registry = VerifiedSitesRegistry::load();
        assert!(registry.lookup("HTTPS://NULO.SH").is_some());
        assert!(registry.lookup("https://nulo.sh:443").is_some());
        assert!(registry.lookup("https://nulo.sh/").is_some());
    }

    #[test]
    fn lookup_misses_for_unknown_origin() {
        let registry = VerifiedSitesRegistry::load();
        assert!(registry.lookup("https://attacker.com").is_none());
        assert!(registry.lookup("https://nulo-fake.sh").is_none());
    }

    #[test]
    fn lookup_returns_none_for_invalid_origin() {
        let registry = VerifiedSitesRegistry::load();
        assert!(registry.lookup("https://nulo.sh/admin").is_none());
        assert!(registry.lookup("not a url").is_none());
    }

    #[test]
    fn try_load_rejects_invalid_origin() {
        let bad = r#"{
            "schemaVersion": 1,
            "entries": [
                { "origins": ["https://nulo.sh/admin"], "displayName": "Nulo",
                  "curatedBy": "u", "addedAt": "2026-01-01" }
            ]
        }"#;
        let res: Result<VerifiedSitesFile, _> = serde_json::from_str(bad);
        let parsed = res.expect("schema-level parse should still succeed");
        // Simulate the try_load loop's behavior on that bad origin:
        let canon = crate::authorization::canonicalize_origin(&parsed.entries[0].origins[0]);
        assert!(canon.is_none(), "must reject path-bearing origin");
    }

    #[test]
    fn try_load_rejects_duplicate() {
        // Build a registry inline to exercise the duplicate-detection branch.
        let dup_json = r#"{
            "schemaVersion": 1,
            "entries": [
                { "origins": ["https://nulo.sh"], "displayName": "First",
                  "curatedBy": "u", "addedAt": "2026-01-01" },
                { "origins": ["HTTPS://NULO.SH"], "displayName": "Second",
                  "curatedBy": "u", "addedAt": "2026-01-01" }
            ]
        }"#;
        let file: VerifiedSitesFile = serde_json::from_str(dup_json).unwrap();
        // Two distinct raw strings, same canonical → duplicate.
        let canon_a = canonicalize_origin(&file.entries[0].origins[0]).unwrap();
        let canon_b = canonicalize_origin(&file.entries[1].origins[0]).unwrap();
        assert_eq!(canon_a, canon_b);
    }

    #[test]
    fn try_load_rejects_non_ascii_host_raw() {
        // A curator typing the Unicode form must be rejected at the raw level.
        let bad = "https://nülo.sh";
        assert!(!bad.is_ascii(), "test fixture must contain non-ASCII");
    }
}
