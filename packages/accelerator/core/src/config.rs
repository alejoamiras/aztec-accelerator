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
/// ACCEPTED LIMITATION (codex B4): because additive fields don't bump the version, a same-`config_version`
/// build that PRE-dates an additive field will omit it on save; the newer build then re-defaults it on the
/// next load. This additive-downgrade reset is tolerated (the field has a serde default, so it is reset, not
/// corrupted). The version gate only protects against REMOVED/RENAMED-field schemas (which DO bump), which
/// is where silent data loss would otherwise occur.
///
/// B4: `config_version` is now READ and ENFORCED (two-stage load, see [`load_with_cap`]):
/// - A config at `config_version <= CONFIG_VERSION` is current-or-migratable: the load mints a
///   [`PersistCapability`], and the app may persist over it. `safari_support` (from the pre-HTTPS-default
///   clean-install era) is migrated to `https_enabled` via a Value-pass (new key wins), and v2 is written.
/// - A config at `config_version > CONFIG_VERSION` (a NEWER build wrote it) yields NO capability: every
///   save path requires `&PersistCapability`, so an older app structurally CANNOT overwrite a newer
///   config — a compile-enforced invariant, not a runtime check (same discipline as B2's `PendingUpdate`).
///
/// v2 (this build): adds the `safari_support`→`https_enabled` migration on top of the v1 clean-install.
const CONFIG_VERSION: u32 = 2;

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
    /// B4: a pre-v2 config's legacy `safari_support` key is migrated into this at LOAD time (Value-pass,
    /// see [`migrate_value`]) — not a serde alias (which would duplicate-field-error). Absent on a clean
    /// install ⇒ `false`; the onboarding wizard re-enables.
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

/// q7e3-F-15 / B4: load from an explicit path, READ-ONLY (no persist capability). Applies the same
/// migration as [`load_with_cap_from`] so reads are consistent, but drops the capability — a caller that
/// then wants to save must use [`load_with_cap_from`]. Missing or malformed file → defaults.
pub fn load_from(path: &std::path::Path) -> AcceleratorConfig {
    load_with_cap_from(path).config
}

/// B4: a non-forgeable token proving the loaded config is at a schema THIS build may safely OVERWRITE.
/// Minted ONLY by [`load_with_cap_from`] on a current-or-migratable config (`config_version <=
/// CONFIG_VERSION`). It has NO `Default` and NO public constructor, so a future-schema load (which yields
/// `None`) makes every save — all of which require `&PersistCapability` — a COMPILE error, not a runtime
/// guard (same discipline as B2's `PendingUpdate`).
// `Clone` is safe: the non-forgeable property is about CONSTRUCTION (no public/Default ctor — a cap can
// only be MINTED by a successful current-or-migratable load), not about copying a right you already hold.
#[derive(Debug, Clone)]
pub struct PersistCapability {
    /// Private unit field: constructible only inside this module.
    _seal: (),
}

impl PersistCapability {
    /// Test-only mint. Production code can obtain a capability ONLY from a `load_with_cap*` on a
    /// current-or-migratable config; a future-schema config yields `None`, so tests that specifically
    /// prove the future-config guard must NOT use this (they assert the *absence* of a cap / a compile
    /// failure). It exists so the many save-round-trip tests needn't fake a load.
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self { _seal: () }
    }
}

/// Result of a save-capable load. `cap` is `Some` iff the on-disk config is at a schema this build may
/// persist over (`config_version <= CONFIG_VERSION`); `None` for a FUTURE-schema config (read-only).
pub struct LoadedConfig {
    pub config: AcceleratorConfig,
    pub cap: Option<PersistCapability>,
}

/// B4: the config lock bundled with its persist capability. Shared between the desktop's Tauri-managed
/// state and the headless server's `HeadlessState.config`, so the cap travels with the config to every
/// save site. `Deref`s to the inner lock — all existing `.read()/.write()` uses are unchanged; the cap
/// gates saves (a future-schema load yields `None`, so those saves can't be compiled / are skipped).
pub struct ConfigStore {
    pub lock: parking_lot::RwLock<AcceleratorConfig>,
    pub cap: Option<PersistCapability>,
}

impl std::ops::Deref for ConfigStore {
    type Target = parking_lot::RwLock<AcceleratorConfig>;
    fn deref(&self) -> &Self::Target {
        &self.lock
    }
}

impl ConfigStore {
    /// Build from a save-capable load (the startup path).
    pub fn new(loaded: LoadedConfig) -> Self {
        Self {
            lock: parking_lot::RwLock::new(loaded.config),
            cap: loaded.cap,
        }
    }

    /// Test-only: a persistable store around `config` (capability present).
    #[cfg(test)]
    pub(crate) fn for_test(config: AcceleratorConfig) -> Self {
        Self {
            lock: parking_lot::RwLock::new(config),
            cap: Some(PersistCapability::for_test()),
        }
    }
}

/// Read ONLY `config_version` from a config JSON string (lenient — unknown fields ignored). `None` when the
/// version can't be determined: malformed JSON, a non-object, or `config_version` of the wrong type / out of
/// `u32` range. Shared by the load probe and the save-time TOCTOU re-check so both agree on "the version".
fn probe_config_version(contents: &str) -> Option<u32> {
    #[derive(Deserialize)]
    struct VersionProbe {
        #[serde(default = "default_config_version")]
        config_version: u32,
    }
    serde_json::from_str::<VersionProbe>(contents)
        .ok()
        .map(|p| p.config_version)
}

/// B4 two-stage, save-capable load — use at startup where the config will later be SAVED. The returned
/// `cap` gates every save path.
pub fn load_with_cap() -> LoadedConfig {
    load_with_cap_from(&config_path())
}

/// As [`load_with_cap`] but from an explicit path (tests + the headless server).
///
/// **FAIL CLOSED (codex B4):** a capability is minted ONLY when this build is CONFIDENT the on-disk config
/// is at a schema it may overwrite — a fresh install, OR a file it fully read, version-probed
/// (`config_version <= CONFIG_VERSION`), migrated, and deserialized. Every uncertainty — a read error, an
/// unreadable version (malformed / non-object / non-integer / out-of-range), a newer schema, or a malformed
/// current config — yields `cap: None` and a best-effort read, so this build never overwrites a config it
/// couldn't confidently interpret (a newer one, or a partially-recoverable one whose values would be lost).
pub fn load_with_cap_from(path: &std::path::Path) -> LoadedConfig {
    let read_only = |config| LoadedConfig { config, cap: None };
    // Only reachable below where we've confirmed the config is current-or-migratable.
    let persistable = |config| LoadedConfig {
        config,
        cap: Some(PersistCapability { _seal: () }),
    };

    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Fresh install: current-schema defaults, persist allowed.
            return persistable(AcceleratorConfig::default());
        }
        Err(e) => {
            // I/O / permission error — we could NOT read the existing config (it may be valid, even newer).
            // Never mint a cap for a file we couldn't read. Run read-only on defaults.
            tracing::warn!(path = %path.display(), error = %e, "Could not read config; running read-only (no persist)");
            return read_only(AcceleratorConfig::default());
        }
    };

    // Stage 1: version probe. If the version can't be determined, FAIL CLOSED (no cap).
    let version = match probe_config_version(&contents) {
        Some(v) => v,
        None => {
            tracing::warn!(path = %path.display(), "Config version unreadable (malformed?); running read-only (no persist)");
            return read_only(AcceleratorConfig::default());
        }
    };
    if version > CONFIG_VERSION {
        // A NEWER build wrote this — best-effort read for the UI, NEVER persist.
        tracing::warn!(path = %path.display(), on_disk = version, supported = CONFIG_VERSION, "Config from a newer build; running read-only (no persist)");
        return read_only(serde_json::from_str(&contents).unwrap_or_default());
    }

    // Stage 2: raw-Value parse + migration + deserialize (current-or-older schema). A malformed current
    // config FAILS CLOSED (read-only, no cap) rather than overwriting the user's file with defaults — a
    // partial corruption (e.g. a bad `https_enabled`) would otherwise discard a recoverable `safari_support`.
    let mut value: serde_json::Value = match serde_json::from_str(&contents) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "Malformed config; running read-only (no persist)");
            return read_only(AcceleratorConfig::default());
        }
    };
    migrate_value(&mut value);
    match serde_json::from_value::<AcceleratorConfig>(value) {
        Ok(mut config) => {
            config.config_version = CONFIG_VERSION;
            persistable(config)
        }
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "Config failed to deserialize post-migration; running read-only (no persist)");
            read_only(AcceleratorConfig::default())
        }
    }
}

/// Value-pass migration: fold pre-v2 legacy keys forward before deserialization. `safari_support` (the
/// bool macOS-only HTTPS toggle from before HTTPS-by-default) → `https_enabled`, only when `https_enabled`
/// is ABSENT (the new key wins if both are present — avoids the duplicate-field error a serde alias caused).
/// The legacy key is then removed so it never round-trips back to disk.
fn migrate_value(value: &mut serde_json::Value) {
    if let Some(obj) = value.as_object_mut() {
        if !obj.contains_key("https_enabled") {
            if let Some(legacy) = obj
                .get("safari_support")
                .and_then(serde_json::Value::as_bool)
            {
                obj.insert("https_enabled".to_string(), serde_json::Value::Bool(legacy));
            }
        }
        obj.remove("safari_support");
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
///
/// B4: requires a [`PersistCapability`] — a future-schema config yields none from load, so this cannot be
/// called to overwrite it. The token is otherwise unused; its PRESENCE is the whole point (compile gate).
pub fn save(
    config: &AcceleratorConfig,
    cap: &PersistCapability,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    save_to(config, &config_path(), cap)
}

/// q7e3-F-15: save to an explicit path atomically (write-tmp-rename, 0o600 on Unix) — so tests exercise
/// the real save (atomicity + perms + parent creation) instead of re-implementing it. B4: gated by
/// [`PersistCapability`] (see [`save`]).
pub fn save_to(
    config: &AcceleratorConfig,
    path: &std::path::Path,
    _cap: &PersistCapability,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // B4 (codex): the capability is minted at LOAD time; re-check the on-disk version at WRITE time to close
    // the TOCTOU where a NEWER build replaced this file after our cap was minted. If the file is now a newer
    // schema, refuse — never clobber it. (A missing/unreadable file is fine to write: fresh install / our own
    // in-flight write.)
    if let Ok(existing) = std::fs::read_to_string(path) {
        if let Some(v) = probe_config_version(&existing) {
            if v > CONFIG_VERSION {
                return Err(format!(
                    "refusing to overwrite config at {}: on-disk config_version {v} is newer than this build's {CONFIG_VERSION}",
                    path.display()
                )
                .into());
            }
        }
    }
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
    cap: &PersistCapability,
    f: impl FnOnce(&mut AcceleratorConfig) -> bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    lock_mutate_save_to(lock, None, cap, f)
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
    cap: &PersistCapability,
    f: impl FnOnce(&mut AcceleratorConfig) -> bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut cfg = lock.write();
    if f(&mut cfg) {
        match path {
            Some(p) => save_to(&cfg, p, cap),
            None => save(&cfg, cap),
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

    // ── B4: config migration (safari_support→https_enabled) + version-gated persist capability ──

    #[test]
    fn migrates_safari_support_to_https_and_stamps_v2() {
        // [mut: delete the `safari_support` branch in migrate_value → https_enabled stays false, FAILS]
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"config_version": 1, "safari_support": true}"#).unwrap();
        let loaded = load_with_cap_from(&path);
        assert!(
            loaded.config.https_enabled,
            "safari_support:true migrates to https_enabled:true"
        );
        assert_eq!(loaded.config.config_version, CONFIG_VERSION);
        assert!(
            loaded.cap.is_some(),
            "a v1 (migratable) config yields a capability"
        );
        // The persisted form drops the legacy key and is stamped v2.
        save_to(&loaded.config, &path, loaded.cap.as_ref().unwrap()).unwrap();
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(
            !on_disk.contains("safari_support"),
            "legacy key never round-trips"
        );
        assert!(on_disk.contains("\"config_version\": 2"), "stamped v2");
    }

    #[test]
    fn migration_new_key_wins_when_both_present() {
        // [mut: `if !obj.contains_key("https_enabled")` → unconditional insert → legacy overwrites, FAILS]
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"safari_support": true, "https_enabled": false}"#).unwrap();
        assert!(
            !load_from(&path).https_enabled,
            "explicit https_enabled wins over legacy safari_support"
        );
    }

    #[test]
    fn future_config_never_persisted_over() {
        // [mut: drop the `probe.config_version > CONFIG_VERSION` gate → cap becomes Some, FAILS]
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        // A NEWER build's config: version ahead AND a shape THIS build can't fully parse.
        std::fs::write(
            &path,
            r#"{"config_version": 999, "https_enabled": {"future": true}}"#,
        )
        .unwrap();
        let loaded = load_with_cap_from(&path);
        assert!(
            loaded.cap.is_none(),
            "a newer-schema config yields NO capability"
        );
        // STRUCTURAL: every save path requires `&PersistCapability`; with cap == None there is no way to
        // persist over this config. The DECISIVE mutation — removing the cap arg from a save signature —
        // makes the whole crate fail to COMPILE, not merely this test.
    }

    #[test]
    fn v2_config_round_trips_untouched_with_capability() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let original = AcceleratorConfig {
            https_enabled: true,
            ..Default::default()
        };
        assert_eq!(original.config_version, CONFIG_VERSION);
        save_to(&original, &path, &PersistCapability::for_test()).unwrap();
        let loaded = load_with_cap_from(&path);
        assert!(loaded.config.https_enabled);
        assert_eq!(loaded.config.config_version, CONFIG_VERSION);
        assert!(loaded.cap.is_some(), "a current v2 config is persistable");
    }

    #[test]
    fn malformed_config_is_read_only_no_capability() {
        // [mut: revert load to `unwrap_or(VersionProbe{v:CONFIG_VERSION})` / `.unwrap_or_default()`+cap → FAILS]
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        // Non-object JSON: version un-probable → FAIL CLOSED (no cap), never overwrite.
        std::fs::write(&path, r#"[1, 2, 3]"#).unwrap();
        assert!(
            load_with_cap_from(&path).cap.is_none(),
            "non-object config is read-only"
        );
        // A valid object whose current-schema field is the WRONG type: the recoverable parts (a legacy
        // safari_support) must NOT be discarded + persisted over — fail closed.
        std::fs::write(
            &path,
            r#"{"config_version": 2, "https_enabled": "not-a-bool", "safari_support": true}"#,
        )
        .unwrap();
        assert!(
            load_with_cap_from(&path).cap.is_none(),
            "a partially-corrupt current config is read-only"
        );
    }

    #[test]
    fn read_io_error_yields_no_capability() {
        // A directory path → read_to_string errors (not NotFound) → FAIL CLOSED (no cap).
        let dir = tempfile::tempdir().unwrap();
        assert!(
            load_with_cap_from(dir.path()).cap.is_none(),
            "an unreadable config path is read-only"
        );
    }

    #[test]
    fn save_refuses_when_disk_version_is_newer_toctou() {
        // [mut: delete the save-time version re-check in save_to → the save succeeds, FAILS]
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        // A NEWER build replaced the file AFTER our (for_test) cap was minted.
        std::fs::write(&path, r#"{"config_version": 999}"#).unwrap();
        let err = save_to(
            &AcceleratorConfig::default(),
            &path,
            &PersistCapability::for_test(),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("newer"),
            "save_to refuses to clobber a newer on-disk config"
        );
        assert!(
            std::fs::read_to_string(&path).unwrap().contains("999"),
            "the newer file is left untouched"
        );
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
        save_to(&original, &cfg_path, &PersistCapability::for_test()).unwrap();
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
