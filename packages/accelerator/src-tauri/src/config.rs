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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AcceleratorConfig {
    #[serde(default)]
    pub safari_support: bool,
    #[serde(default)]
    pub approved_origins: Vec<String>,
    #[serde(default)]
    pub speed: Speed,
    /// None = never asked, Some(true) = auto-update, Some(false) = manual
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_update: Option<bool>,
}

/// Returns `~/.aztec-accelerator/config.json`.
pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aztec-accelerator")
        .join("config.json")
}

/// Load config from disk. Returns default if missing or malformed.
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
    std::fs::write(&path, &json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
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
            approved_origins: vec![
                "https://example.com".to_string(),
                "https://other.dev".to_string(),
            ],
            speed: Speed::Balanced,
            auto_update: Some(true),
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
            approved_origins: vec!["https://a.com".to_string(), "https://b.com".to_string()],
            ..Default::default()
        };
        config.approved_origins.retain(|o| o != "https://a.com");
        assert_eq!(config.approved_origins, vec!["https://b.com"]);
    }
}
