//! F-004 Layer B: a monotonic version floor — the rollback ratchet.
//!
//! Even if Layer A ([`crate::update_manifest`]) were somehow bypassed, the app must never move
//! backwards. We persist the highest version that has ever SUCCESSFULLY RUN in a dedicated,
//! owner-only, atomically-written state file (NOT `config.json`, whose load is deliberately
//! fail-OPEN and would silently erase the floor on any parse glitch). An update candidate is
//! accepted only if it is strictly greater — by SemVer PRECEDENCE (build metadata ignored) — than
//! `max(current_running, floor)`. The floor advances only after a new build proves it runs (the
//! caller commits post-launch), so a crashing bad update can never ratchet it. A corrupt/unreadable
//! floor fails CLOSED (updates disabled) and is never overwritten, preserving forensic evidence.
//!
//! This module is the pure, GUI-agnostic core logic (unit-testable without the Tauri toolchain).
//! The cross-process `flock` "updater transaction" that serialises check→install→commit across
//! concurrent instances is applied by the desktop caller around these operations.

use std::io::Write as _;
use std::path::Path;

use semver::Version;
use serde::{Deserialize, Serialize};

const SCHEMA: u32 = 1;

/// The persisted floor. `deny_unknown_fields` so a tampered file with extra keys is rejected
/// (→ [`FloorState::Corrupt`]) rather than silently accepted.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FloorFile {
    schema: u32,
    floor: String,
}

/// The three distinguishable load outcomes. `Corrupt` and `Missing` are NOT the same: `Missing`
/// bootstraps (first run / pre-floor upgrade), `Corrupt` fails closed and is never overwritten.
#[derive(Debug, PartialEq, Eq)]
pub enum FloorState {
    /// No floor file yet — first run. Effective floor is `current_running` alone.
    Missing,
    Valid(Version),
    /// File exists but is unreadable / not our schema / not canonical SemVer. Fail closed.
    Corrupt,
}

/// Load the floor. IO errors other than "not found" are treated as `Corrupt` (fail closed) — a
/// permission or read failure on security state must not be interpreted as "no floor".
pub fn load_floor(path: &Path) -> FloorState {
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return FloorState::Missing,
        Err(_) => return FloorState::Corrupt,
    };
    let parsed: FloorFile = match serde_json::from_str(&contents) {
        Ok(p) => p,
        Err(_) => return FloorState::Corrupt,
    };
    if parsed.schema != SCHEMA {
        return FloorState::Corrupt;
    }
    match Version::parse(&parsed.floor) {
        // Require canonical round-trip so a non-canonical stored value can't slip the comparator.
        Ok(v) if v.to_string() == parsed.floor => FloorState::Valid(v),
        _ => FloorState::Corrupt,
    }
}

/// Is `candidate` acceptable given the running version and the loaded floor? Uses SemVer
/// PRECEDENCE (build metadata ignored, prerelease ordered correctly) and requires STRICTLY greater
/// than both. A `Corrupt` floor rejects everything (fail closed).
pub fn candidate_allowed(candidate: &Version, current: &Version, floor: &FloorState) -> bool {
    let floor_v = match floor {
        FloorState::Corrupt => return false,
        FloorState::Missing => None,
        FloorState::Valid(v) => Some(v),
    };
    let gt = |a: &Version, b: &Version| a.cmp_precedence(b) == std::cmp::Ordering::Greater;
    if !gt(candidate, current) {
        return false;
    }
    match floor_v {
        Some(f) => gt(candidate, f),
        None => true,
    }
}

/// True iff the running build is BELOW the floor — a possible rollback that already happened
/// (someone installed an older binary out of band). The caller disables updates + logs `SECURITY:`.
pub fn running_below_floor(current: &Version, floor: &FloorState) -> bool {
    matches!(floor, FloorState::Valid(f) if f.cmp_precedence(current) == std::cmp::Ordering::Greater)
}

/// Record that `current` launched successfully: advance the floor to `max(floor, current)` by
/// precedence. Monotonic by construction — never lowers. Refuses to touch a `Corrupt` floor
/// (returns an error so the caller keeps updates disabled and preserves the file for inspection).
/// Atomic + owner-only write.
pub fn commit_successful_launch(path: &Path, current: &Version) -> std::io::Result<()> {
    match load_floor(path) {
        FloorState::Corrupt => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "refusing to overwrite a corrupt version floor",
        )),
        FloorState::Valid(f) if f.cmp_precedence(current) != std::cmp::Ordering::Less => Ok(()), // already >= current
        _ => write_floor(path, current), // Missing, or floor < current
    }
}

/// Atomically write the floor with owner-only perms. Strengthened over the config.rs pattern:
/// random temp name in the SAME dir (via `tempfile`), explicit `0600`, `fsync` of the file AND the
/// parent dir on Unix, and a chmod failure is a HARD error (not ignored).
fn write_floor(path: &Path, v: &Version) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "floor path has no parent")
    })?;
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }

    let body = serde_json::to_vec(&FloorFile {
        schema: SCHEMA,
        floor: v.to_string(),
    })?;

    // Random same-dir temp (owner-only from creation), write, fsync, then atomic rename.
    let mut tmp = tempfile::Builder::new()
        .prefix(".updater-state-")
        .tempfile_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    tmp.write_all(&body)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)
        .map_err(|e| std::io::Error::other(e.error))?;

    // fsync the directory so the rename is durable.
    #[cfg(unix)]
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }
    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn missing_bootstraps() {
        let d = tmp();
        assert_eq!(load_floor(&d.path().join("s.json")), FloorState::Missing);
        // With no floor, only `> current` matters.
        assert!(candidate_allowed(
            &v("1.1.0"),
            &v("1.0.0"),
            &FloorState::Missing
        ));
        assert!(!candidate_allowed(
            &v("1.0.0"),
            &v("1.0.0"),
            &FloorState::Missing
        ));
    }

    #[test]
    fn commit_is_monotonic_and_roundtrips() {
        let d = tmp();
        let p = d.path().join("s.json");
        commit_successful_launch(&p, &v("1.0.8")).unwrap();
        assert_eq!(load_floor(&p), FloorState::Valid(v("1.0.8")));
        // A lower "current" never lowers the floor.
        commit_successful_launch(&p, &v("1.0.5")).unwrap();
        assert_eq!(load_floor(&p), FloorState::Valid(v("1.0.8")));
        // A higher one advances it.
        commit_successful_launch(&p, &v("1.1.0")).unwrap();
        assert_eq!(load_floor(&p), FloorState::Valid(v("1.1.0")));
    }

    #[test]
    fn floor_blocks_rollback_candidate() {
        let f = FloorState::Valid(v("1.0.8"));
        // candidate ≤ floor rejected even if > current (the ratchet).
        assert!(!candidate_allowed(&v("1.0.7"), &v("1.0.6"), &f));
        assert!(!candidate_allowed(&v("1.0.8"), &v("1.0.6"), &f)); // equal to floor
        assert!(candidate_allowed(&v("1.0.9"), &v("1.0.6"), &f));
    }

    #[test]
    fn corrupt_fails_closed_and_is_preserved() {
        let d = tmp();
        let p = d.path().join("s.json");
        std::fs::write(&p, b"{ not json").unwrap();
        assert_eq!(load_floor(&p), FloorState::Corrupt);
        assert!(!candidate_allowed(
            &v("9.9.9"),
            &v("1.0.0"),
            &FloorState::Corrupt
        ));
        // commit must REFUSE and leave the corrupt file untouched.
        assert!(commit_successful_launch(&p, &v("2.0.0")).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{ not json");
    }

    #[test]
    fn wrong_schema_is_corrupt() {
        let d = tmp();
        let p = d.path().join("s.json");
        std::fs::write(&p, br#"{"schema":99,"floor":"1.0.0"}"#).unwrap();
        assert_eq!(load_floor(&p), FloorState::Corrupt);
    }

    #[test]
    fn prerelease_and_build_metadata_precedence() {
        let f = FloorState::Valid(v("1.0.8-rc.2"));
        assert!(candidate_allowed(&v("1.0.8-rc.10"), &v("1.0.7"), &f)); // rc.10 > rc.2 (numeric)
        assert!(candidate_allowed(&v("1.0.8"), &v("1.0.7"), &f)); // stable > its rc
                                                                  // Build metadata is ignored by precedence — same precedence ⇒ NOT strictly greater ⇒ rejected.
        let f2 = FloorState::Valid(v("1.0.8"));
        assert!(!candidate_allowed(&v("1.0.8+build.5"), &v("1.0.0"), &f2));
    }

    #[test]
    fn running_below_floor_detects_rollback() {
        assert!(running_below_floor(
            &v("1.0.5"),
            &FloorState::Valid(v("1.0.8"))
        ));
        assert!(!running_below_floor(
            &v("1.0.8"),
            &FloorState::Valid(v("1.0.8"))
        ));
        assert!(!running_below_floor(
            &v("1.0.9"),
            &FloorState::Valid(v("1.0.8"))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn written_file_is_owner_only() {
        use std::os::unix::fs::MetadataExt;
        let d = tmp();
        let p = d.path().join("s.json");
        commit_successful_launch(&p, &v("1.0.0")).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(d.path()).unwrap().mode() & 0o777, 0o700);
    }
}
