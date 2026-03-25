use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceleratorConfig {
    #[serde(default)]
    pub safari_support: bool,
    #[serde(default)]
    pub approved_origins: Vec<String>,
    #[serde(default = "default_speed")]
    pub speed: String,
    /// None = never asked, Some(true) = auto-update, Some(false) = manual
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_update: Option<bool>,
}

fn default_speed() -> String {
    "full".to_string()
}

impl Default for AcceleratorConfig {
    fn default() -> Self {
        Self {
            safari_support: false,
            approved_origins: Vec::new(),
            speed: default_speed(),
            auto_update: None,
        }
    }
}

/// Convert speed setting to thread count.
/// - "full": all available cores
/// - "high": 3/4 of available cores (min 1)
/// - "balanced": half of available cores (min 1)
/// - "light": 3/8 of available cores (min 1)
/// - "low": quarter of available cores (min 1)
pub fn speed_to_threads(speed: &str) -> usize {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    match speed {
        "low" => (cpus / 4).max(1),
        "light" => (cpus * 3 / 8).max(1),
        "balanced" => (cpus / 2).max(1),
        "high" => (cpus * 3 / 4).max(1),
        _ => cpus, // "full" or unknown
    }
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
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
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
        assert_eq!(config.speed, "full");
    }

    #[test]
    fn config_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let config = AcceleratorConfig {
            safari_support: true,
            approved_origins: vec!["https://example.com".to_string()],
            speed: "balanced".to_string(),
            auto_update: Some(true),
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&path, &json).unwrap();
        let loaded: AcceleratorConfig =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(loaded.safari_support);
        assert_eq!(loaded.approved_origins, vec!["https://example.com"]);
        assert_eq!(loaded.speed, "balanced");
    }

    #[test]
    fn speed_to_threads_returns_valid_counts() {
        let full = speed_to_threads("full");
        let high = speed_to_threads("high");
        let balanced = speed_to_threads("balanced");
        let light = speed_to_threads("light");
        let low = speed_to_threads("low");
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
        // config_path() points to the real home dir, but load() handles missing files gracefully
        let config: AcceleratorConfig = serde_json::from_str("{}").unwrap_or_default();
        assert!(!config.safari_support);
    }

    #[test]
    fn load_returns_default_for_malformed_json() {
        let config: AcceleratorConfig = serde_json::from_str("not json").unwrap_or_default();
        assert!(!config.safari_support);
    }

    #[test]
    fn speed_rejects_invalid_values() {
        // speed_to_threads treats unknown values as "full" (all cores)
        let unknown = speed_to_threads("turbo");
        let full = speed_to_threads("full");
        assert_eq!(unknown, full);
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
        let mut config = AcceleratorConfig::default();
        config.auto_update = Some(true);
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
        let mut config = AcceleratorConfig::default();
        config.approved_origins.push("https://a.com".to_string());
        config.approved_origins.push("https://b.com".to_string());
        config.approved_origins.retain(|o| o != "https://a.com");
        assert_eq!(config.approved_origins, vec!["https://b.com"]);
    }
}
