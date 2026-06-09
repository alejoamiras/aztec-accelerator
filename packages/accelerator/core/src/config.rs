use crate::authorization::CanonicalOrigin;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Proving speed level — controls how many CPU cores are used for proving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Speed {
    Low,
    Light,
    Balanced,
    High,
    #[default]
    Full,
}

impl Speed {
    /// Convert to thread count based on available CPU cores.
    pub fn to_threads(self) -> usize {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        match self {
            Speed::Low => (cpus / 4).max(1),
            Speed::Light => (cpus * 3 / 8).max(1),
            Speed::Balanced => (cpus / 2).max(1),
            Speed::High => (cpus * 3 / 4).max(1),
            Speed::Full => cpus,
        }
    }

    /// Returns true if this is the "full" speed (bb should use its default).
    pub fn is_full(self) -> bool {
        self == Speed::Full
    }
}

/// Current config schema version. Bump when fields are removed or renamed.
/// Added fields with `#[serde(default)]` don't require a version bump.
const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceleratorConfig {
    /// Schema version for future migration support.
    #[serde(default = "default_config_version")]
    pub config_version: u32,
    #[serde(default)]
    pub safari_support: bool,
    #[serde(default, deserialize_with = "de_approved_origins")]
    pub approved_origins: Vec<CanonicalOrigin>,
    #[serde(default)]
    pub speed: Speed,
    /// None = never asked, Some(true) = auto-update, Some(false) = manual
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_update: Option<bool>,
    /// SEC-04: when `true`, any `localhost`/`127.0.0.1`/`[::1]` origin is auto-approved with no
    /// prompt. Defaults to **`false`** on desktop (a localhost page gets one remembered approval
    /// prompt instead — closes the silent local-page hole); the headless binary sets it `true` (it
    /// has no popup). Existing on-disk configs lacking the field deserialize to `false` (secure).
    #[serde(default)]
    pub auto_approve_localhost: bool,
}

impl Default for AcceleratorConfig {
    fn default() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            safari_support: false,
            approved_origins: Vec::new(),
            speed: Speed::default(),
            auto_update: None,
            auto_approve_localhost: false,
        }
    }
}

fn default_config_version() -> u32 {
    CONFIG_VERSION
}

/// Returns `~/.aztec-accelerator/config.json`.
pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aztec-accelerator")
        .join("config.json")
}

/// Load config from disk. Returns default if missing or malformed.
///
/// `approved_origins` is canonicalized at the serde boundary by [`de_approved_origins`]
/// (drop-invalid + dedupe), so already-canonical entries load 1:1 and no migration or
/// on-disk resave is needed (F-02).
pub fn load() -> AcceleratorConfig {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|e| {
            tracing::warn!(path = %path.display(), error = %e, "Malformed config, using defaults");
            AcceleratorConfig::default()
        }),
        Err(_) => AcceleratorConfig::default(),
    }
}

/// Lenient deserializer for `approved_origins`: reads `Vec<String>`, canonicalizes each via
/// [`CanonicalOrigin`], DROPS (with a warning) entries that fail, and dedupes survivors
/// order-preserving. Replaces the old load-time `migrate_approved_origins` + resave —
/// canonicalization happens here, idempotently, so existing canonical configs deserialize
/// 1:1. A single bad entry can't fail the whole config load (matches the prior tolerance).
fn de_approved_origins<'de, D>(d: D) -> Result<Vec<CanonicalOrigin>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = <Vec<String> as Deserialize>::deserialize(d)?;
    let mut out: Vec<CanonicalOrigin> = Vec::with_capacity(raw.len());
    let mut dropped: Vec<String> = Vec::new();
    for entry in raw {
        match CanonicalOrigin::try_from(entry) {
            Ok(canon) if !out.contains(&canon) => out.push(canon),
            Ok(_) => {} // duplicate, drop silently
            Err(e) => dropped.push(e.0),
        }
    }
    if !dropped.is_empty() {
        tracing::warn!(count = dropped.len(), dropped = ?dropped, "Dropped un-canonicalizable approved_origins entries on load");
    }
    Ok(out)
}

/// Save config to disk. Creates parent directories if needed.
/// Sets file permissions to 0o600 on Unix (owner read/write only).
pub fn save(config: &AcceleratorConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    let json = serde_json::to_string_pretty(config)?;

    // Write to a temp file then rename for atomicity — if the process crashes
    // mid-write, the original config.json is untouched.
    let tmp_path = path.with_extension("json.tmp");
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp_path)?;
        file.write_all(json.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&tmp_path, &json)?;
    }
    std::fs::rename(&tmp_path, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_safari_support_false() {
        let config = AcceleratorConfig::default();
        assert!(!config.safari_support);
        assert!(config.approved_origins.is_empty());
        assert_eq!(config.speed, Speed::Full);
    }

    #[test]
    fn config_roundtrip_via_save_load() {
        // Override config_path by writing/reading directly through save()/load()
        // using a temp HOME so we don't touch the real config.
        let dir = tempfile::tempdir().unwrap();
        let cfg_dir = dir.path().join(".aztec-accelerator");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        let cfg_path = cfg_dir.join("config.json");

        let original = AcceleratorConfig {
            safari_support: true,
            approved_origins: vec![co("https://example.com"), co("https://other.dev")],
            speed: Speed::Balanced,
            auto_update: Some(true),
            ..Default::default()
        };

        // Write via serde (same as save()) and read back
        let json = serde_json::to_string_pretty(&original).unwrap();
        std::fs::write(&cfg_path, &json).unwrap();
        let loaded: AcceleratorConfig =
            serde_json::from_str(&std::fs::read_to_string(&cfg_path).unwrap()).unwrap();

        assert_eq!(loaded.safari_support, original.safari_support);
        assert_eq!(loaded.approved_origins, original.approved_origins);
        assert_eq!(loaded.speed, original.speed);
        assert_eq!(loaded.auto_update, original.auto_update);
    }

    #[test]
    fn config_roundtrip_auto_update_none() {
        // Ensure None survives roundtrip (skip_serializing_if + serde default)
        let original = AcceleratorConfig {
            auto_update: None,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&original).unwrap();
        let loaded: AcceleratorConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.auto_update, None);
    }

    #[test]
    fn config_roundtrip_auto_update_false() {
        // Some(false) must survive — distinct from None (never asked)
        let original = AcceleratorConfig {
            auto_update: Some(false),
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&original).unwrap();
        let loaded: AcceleratorConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.auto_update, Some(false));
    }

    #[test]
    fn speed_to_threads_returns_valid_counts() {
        let full = Speed::Full.to_threads();
        let high = Speed::High.to_threads();
        let balanced = Speed::Balanced.to_threads();
        let light = Speed::Light.to_threads();
        let low = Speed::Low.to_threads();
        assert!(full >= 1);
        assert!(high >= 1);
        assert!(balanced >= 1);
        assert!(light >= 1);
        assert!(low >= 1);
        assert!(full >= high);
        assert!(high >= balanced);
        assert!(balanced >= light);
        assert!(light >= low);
    }

    #[test]
    fn load_returns_default_for_missing_file() {
        let config: AcceleratorConfig = serde_json::from_str("{}").unwrap_or_default();
        assert!(!config.safari_support);
    }

    #[test]
    fn load_returns_default_for_malformed_json() {
        let config: AcceleratorConfig = serde_json::from_str("not json").unwrap_or_default();
        assert!(!config.safari_support);
    }

    #[test]
    fn speed_serializes_as_lowercase() {
        let json = serde_json::to_string(&Speed::Balanced).unwrap();
        assert_eq!(json, "\"balanced\"");
    }

    #[test]
    fn speed_deserializes_from_lowercase() {
        let speed: Speed = serde_json::from_str("\"low\"").unwrap();
        assert_eq!(speed, Speed::Low);
    }

    #[test]
    fn speed_invalid_string_fails_deserialization() {
        let result: Result<Speed, _> = serde_json::from_str("\"turbo\"");
        assert!(result.is_err());
    }

    #[test]
    fn auto_update_defaults_to_none() {
        let config = AcceleratorConfig::default();
        assert_eq!(config.auto_update, None);
    }

    #[test]
    fn auto_update_none_not_serialized() {
        // None should be omitted from JSON (skip_serializing_if)
        let config = AcceleratorConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("auto_update"));
    }

    #[test]
    fn auto_update_some_serialized() {
        let config = AcceleratorConfig {
            auto_update: Some(true),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"auto_update\":true"));
    }

    #[test]
    fn auto_update_missing_deserializes_as_none() {
        let config: AcceleratorConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.auto_update, None);
    }

    #[test]
    fn approved_origins_removal() {
        let mut config = AcceleratorConfig {
            approved_origins: vec![co("https://a.com"), co("https://b.com")],
            ..Default::default()
        };
        config
            .approved_origins
            .retain(|o| o.as_str() != "https://a.com");
        assert_eq!(config.approved_origins, vec![co("https://b.com")]);
    }

    // ─── de_approved_origins (F-02 — replaces migrate_approved_origins) ──

    fn co(s: &str) -> CanonicalOrigin {
        CanonicalOrigin::parse(s).expect("canonical test origin")
    }

    /// Deserialize a JSON array literal through `de_approved_origins`.
    fn de_origins(json_array: &str) -> Vec<CanonicalOrigin> {
        #[derive(Deserialize)]
        struct W {
            #[serde(deserialize_with = "de_approved_origins")]
            v: Vec<CanonicalOrigin>,
        }
        serde_json::from_str::<W>(&format!("{{\"v\":{json_array}}}"))
            .unwrap()
            .v
    }

    #[test]
    fn de_origins_keeps_canonical() {
        assert_eq!(
            de_origins(r#"["https://nulo.sh","chrome-extension://abc"]"#),
            vec![co("https://nulo.sh"), co("chrome-extension://abc")],
        );
    }

    #[test]
    fn de_origins_canonicalizes_mixed_case_and_default_port() {
        assert_eq!(
            de_origins(r#"["HTTPS://NULO.SH:443","https://faucet.nulo.sh/"]"#),
            vec![co("https://nulo.sh"), co("https://faucet.nulo.sh")],
        );
    }

    #[test]
    fn de_origins_dedupes() {
        assert_eq!(
            de_origins(r#"["https://nulo.sh","HTTPS://nulo.sh","https://nulo.sh:443"]"#),
            vec![co("https://nulo.sh")],
        );
    }

    #[test]
    fn de_origins_drops_uncanonicalizable() {
        assert_eq!(
            de_origins(r#"["https://nulo.sh","not a url","https://nulo.sh/admin"]"#),
            vec![co("https://nulo.sh")],
        );
    }

    #[test]
    fn de_origins_preserves_order() {
        assert_eq!(
            de_origins(r#"["https://b.com","https://a.com"]"#),
            vec![co("https://b.com"), co("https://a.com")],
        );
    }

    #[test]
    fn raw_non_canonical_on_disk_roundtrips_to_canonical_in_memory() {
        // opus M3: proves the deleted load-time resave is unnecessary — a non-canonical persisted
        // entry deserializes to the canonical in-memory form, so compare-based remove/is_approved
        // still work without rewriting the file.
        let cfg: AcceleratorConfig =
            serde_json::from_str(r#"{"approved_origins":["HTTPS://NULO.SH:443"]}"#).unwrap();
        assert_eq!(cfg.approved_origins, vec![co("https://nulo.sh")]);
    }
}
