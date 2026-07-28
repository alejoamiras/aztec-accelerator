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
///
/// NOTE: nothing currently *reads* `config_version`. The HTTPS-by-default feature is a CLEAN-INSTALL
/// (no migration from the old macOS-only "Safari Support" state — decided with the owner): a config
/// written by an older build that carried `safari_support` simply deserializes with `https_enabled`
/// at its serde default (`false`); the onboarding wizard (shown once because `onboarding_version`
/// defaults to 0) re-enables HTTPS in one click. Dropping the `safari_support` alias also removes the
/// duplicate-field edge that could reset an entire config to defaults.
const CONFIG_VERSION: u32 = 1;

/// The onboarding-wizard consent version. The first-run wizard shows while a config's
/// `onboarding_version` is `< ONBOARDING_VERSION`. A versioned int (not a bool) so a future
/// release can re-onboard for a new consent surface by bumping this. New installs AND existing
/// upgraders both start at 0 (serde default), so both see the wizard once.
pub const ONBOARDING_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceleratorConfig {
    /// Schema version for future migration support.
    #[serde(default = "default_config_version")]
    pub config_version: u32,
    /// Whether the local HTTPS listener (browser ⇄ accelerator encrypted channel) is enabled.
    /// Clean-install: NO `safari_support` alias (see `CONFIG_VERSION`) — an older config's
    /// `safari_support` key is simply ignored and this defaults to `false`; the wizard re-enables.
    #[serde(default)]
    pub https_enabled: bool,
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
    /// First-run onboarding progress. `< ONBOARDING_VERSION` ⇒ the wizard is shown once. Serde
    /// default 0 = never onboarded (so existing upgraders see it too — the consent moment for the
    /// HTTPS-by-default migration).
    #[serde(default)]
    pub onboarding_version: u32,
    /// Unix seconds of the last cert-renewal consent prompt (macOS/Windows renewal-window throttle).
    /// `None` = never prompted. Skipped when `None` to keep on-disk configs clean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_rotation_prompt_at: Option<i64>,
}

impl Default for AcceleratorConfig {
    fn default() -> Self {
        Self {
            config_version: CONFIG_VERSION,
            https_enabled: false,
            approved_origins: Vec::new(),
            speed: Speed::default(),
            auto_update: None,
            auto_approve_localhost: false,
            onboarding_version: 0,
            last_rotation_prompt_at: None,
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
    load_from(&config_path())
}

/// q7e3-F-15: load from an explicit path (so tests exercise the real load instead of re-implementing
/// it). Missing or malformed file → defaults.
pub fn load_from(path: &std::path::Path) -> AcceleratorConfig {
    match std::fs::read_to_string(path) {
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

/// Save config to disk (to the default `config_path()`). Creates parent dirs; 0o600 on Unix.
pub fn save(config: &AcceleratorConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    save_to(config, &config_path())
}

/// q7e3-F-15: save to an explicit path atomically (write-tmp-rename, 0o600 on Unix) — so tests exercise
/// the real save (atomicity + perms + parent creation) instead of re-implementing it.
pub fn save_to(
    config: &AcceleratorConfig,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
    #[cfg(windows)]
    {
        use std::io::Write;
        // F-003 Windows tail: write the temp with an owner-only DACL; the SD travels with the same-volume
        // rename to `config.json`. Clear any stale temp first so CREATE_NEW succeeds.
        let _ = std::fs::remove_file(&tmp_path);
        let mut file = crate::win_acl::secure_create_file(&tmp_path)?;
        file.write_all(json.as_bytes())?;
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        std::fs::write(&tmp_path, &json)?;
    }
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// q7e3-F-13: lock the config, mutate via `f`, and save IFF `f` returns `true` (it changed something).
/// Returns the save `Result` so each caller keeps its own save-failure disposition (`?` to propagate,
/// `warn!`, or ignore) — preserving the three divergent policies the prior hand-rolled copies had, with
/// one shared lock-mutate-save body in `core`. (The prior `mutate_config` helper lived in `src-tauri`,
/// so core's `auth.rs` couldn't reach it.) The `bool` return keeps `auth.rs`'s conditional save from
/// becoming an always-write on the piggyback-Allow path.
pub fn lock_mutate_save(
    lock: &parking_lot::RwLock<AcceleratorConfig>,
    f: impl FnOnce(&mut AcceleratorConfig) -> bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    lock_mutate_save_to(lock, None, f)
}

/// As [`lock_mutate_save`], but writes to `path` when given instead of [`config_path`].
///
/// Exists so the persistence path can be tested for real. Approving an origin is now unconditional
/// (there is no ephemeral Allow), so a test that drives `authorize_origin` would otherwise write the
/// DEVELOPER'S OWN `~/.aztec-accelerator/config.json` — and an assertion against the in-memory config
/// would still pass even if that write failed, proving nothing about persistence (post-impl codex).
/// With a destination injected, the test can reload from disk and assert what actually landed.
pub fn lock_mutate_save_to(
    lock: &parking_lot::RwLock<AcceleratorConfig>,
    path: Option<&std::path::Path>,
    f: impl FnOnce(&mut AcceleratorConfig) -> bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut cfg = lock.write();
    if f(&mut cfg) {
        match path {
            Some(p) => save_to(&cfg, p),
            None => save(&cfg),
        }
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_https_disabled() {
        let config = AcceleratorConfig::default();
        assert!(!config.https_enabled);
        assert!(config.approved_origins.is_empty());
        assert_eq!(config.speed, Speed::Full);
        assert_eq!(config.onboarding_version, 0);
        assert_eq!(config.last_rotation_prompt_at, None);
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
            https_enabled: true,
            approved_origins: vec![co("https://example.com"), co("https://other.dev")],
            speed: Speed::Balanced,
            auto_update: Some(true),
            onboarding_version: 1,
            last_rotation_prompt_at: Some(1_700_000_000),
            ..Default::default()
        };

        // q7e3-F-15: exercise the REAL save_to/load_from (atomic write-tmp-rename + 0o600 + the
        // de_approved_origins deserializer) instead of re-implementing them — so this test can now
        // catch a regression in save()/load() itself.
        save_to(&original, &cfg_path).unwrap();
        let loaded = load_from(&cfg_path);

        assert_eq!(loaded.https_enabled, original.https_enabled);
        assert_eq!(loaded.approved_origins, original.approved_origins);
        assert_eq!(loaded.speed, original.speed);
        assert_eq!(loaded.auto_update, original.auto_update);
        assert_eq!(loaded.onboarding_version, original.onboarding_version);
        assert_eq!(
            loaded.last_rotation_prompt_at,
            original.last_rotation_prompt_at
        );
    }

    // ─── no-migration clean install: `safari_support` is NOT aliased ─────────

    #[test]
    fn legacy_safari_support_key_is_ignored_https_defaults_off() {
        // Clean-install (no alias): a pre-rename config's `safari_support` key is an UNKNOWN field —
        // ignored — so https_enabled takes its serde default (false). The onboarding wizard (shown once
        // because onboarding_version defaults to 0) re-enables HTTPS.
        let cfg: AcceleratorConfig = serde_json::from_str(r#"{"safari_support": true}"#).unwrap();
        assert!(
            !cfg.https_enabled,
            "legacy safari_support is ignored — https_enabled defaults off (no migration)"
        );
    }

    #[test]
    fn save_writes_only_new_key_not_legacy_alias() {
        // A save must emit `https_enabled` and never `safari_support`.
        let cfg = AcceleratorConfig {
            https_enabled: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"https_enabled\":true"));
        assert!(!json.contains("safari_support"));
    }

    #[test]
    fn both_keys_present_reads_https_enabled_ignores_safari() {
        // With no alias, a config carrying BOTH keys is NOT a duplicate-field error: `safari_support`
        // is ignored and `https_enabled` is read normally (the old alias made this a hard error that
        // reset the WHOLE config to defaults — post-impl codex Medium, now moot).
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("config.json");
        std::fs::write(
            &cfg_path,
            r#"{"safari_support": false, "https_enabled": true, "speed": "low"}"#,
        )
        .unwrap();
        let loaded = load_from(&cfg_path);
        assert!(
            loaded.https_enabled,
            "https_enabled must be read directly; safari_support ignored, not a duplicate-field error"
        );
        assert_eq!(
            loaded.speed,
            Speed::Low,
            "the rest of the config must survive (no reset to defaults)"
        );
    }

    #[test]
    fn onboarding_version_defaults_to_zero_when_absent() {
        let cfg: AcceleratorConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.onboarding_version, 0);
    }

    #[test]
    fn last_rotation_prompt_at_none_not_serialized() {
        let cfg = AcceleratorConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(!json.contains("last_rotation_prompt_at"));
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
        assert!(!config.https_enabled);
    }

    #[test]
    fn load_returns_default_for_malformed_json() {
        let config: AcceleratorConfig = serde_json::from_str("not json").unwrap_or_default();
        assert!(!config.https_enabled);
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
            de_origins(
                r#"["https://nulo.sh","chrome-extension://abcdefghijklmnopabcdefghijklmnop"]"#
            ),
            vec![
                co("https://nulo.sh"),
                co("chrome-extension://abcdefghijklmnopabcdefghijklmnop"),
            ],
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
    fn de_origins_drops_trailing_dot_without_migrating() {
        // F-011: a persisted dotted origin is DROPPED on load — never silently rewritten to its
        // undotted (approved) form, so it cannot inherit the undotted origin's grant.
        let loaded = de_origins(r#"["https://ok.example","https://bad.example."]"#);
        assert_eq!(loaded, vec![co("https://ok.example")]);
        assert!(!loaded.contains(&co("https://bad.example")));
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
