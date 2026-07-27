//! F-004 Layer B: a monotonic version floor — the rollback ratchet.
//!
//! Even if Layer A ([`crate::update_manifest`]) were somehow bypassed, the app must never move
//! backwards. We persist, in a dedicated owner-only atomically-written state file (NOT `config.json`,
//! whose load is deliberately fail-OPEN and would silently erase the floor on any parse glitch), two
//! monotonic high-water marks:
//!
//! - **`floor`** — the highest version that has ever SUCCESSFULLY RUN. Advanced only after a new build
//!   proves it runs (the caller commits post-launch, [`commit_successful_launch`]), so a crashing bad
//!   update can never ratchet it.
//! - **`pending`** — the version an install has COMMITTED to (the install INTENT) but that has not yet
//!   proven healthy ([`record_pending`], written under the updater lock right BEFORE `install()` — see
//!   that fn for why after-install is wrong on Windows). This closes the restart race: instance A
//!   commits `3.0.0` and installs/restarts (releasing the lock) before `3.0.0` can commit its floor;
//!   without `pending`, a racing instance B would still see `floor = 1.0.0` and could install a LOWER
//!   `2.0.0`, regressing the just-installed higher version. With `pending`, B sees the floor at `3.0.0`.
//!
//! An update candidate is accepted only if — by SemVer PRECEDENCE (build metadata ignored) — it is
//! strictly greater than `max(current_running, floor)` AND not below `pending` (candidate `== pending`
//! is allowed, so a failed install can be RETRIED with the exact intent without poisoning it; a lower
//! version stays blocked). A corrupt/unreadable state fails CLOSED (updates disabled) and is never
//! overwritten, preserving forensic evidence.
//!
//! This module is the pure, GUI-agnostic core logic (unit-testable without the Tauri toolchain). The
//! cross-process `flock` "updater transaction" that serialises check→install→record→commit across
//! concurrent instances is applied by the desktop caller around these operations; every mutating call
//! here ([`record_pending`], [`commit_successful_launch`]) MUST run while the caller holds that lock.

use std::io::Write as _;
use std::path::Path;

use semver::Version;
use serde::{Deserialize, Serialize};

const SCHEMA: u32 = 1;

/// The persisted state. `deny_unknown_fields` so a tampered file with extra keys is rejected
/// (→ [`LoadedState::Corrupt`]) rather than silently accepted. `pending` is optional (older files and
/// the common "nothing installing" case omit it).
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateFile {
    schema: u32,
    floor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending: Option<String>,
}

/// The distinguishable load outcomes. `Corrupt` and `Missing` are NOT the same: `Missing` bootstraps
/// (first run / pre-floor upgrade), `Corrupt` fails closed and is never overwritten.
#[derive(Debug, PartialEq, Eq)]
pub enum LoadedState {
    /// No state file yet — first run. Effective floor is `current_running` alone.
    Missing,
    /// File exists but is unreadable / not our schema / not canonical SemVer. Fail closed.
    Corrupt,
    Valid {
        floor: Version,
        /// An install committed to but not yet proven healthy — part of the effective floor.
        pending: Option<Version>,
    },
}

/// Parse a stored version string, requiring canonical round-trip so a non-canonical value can't slip
/// the comparator.
fn parse_canonical(s: &str) -> Option<Version> {
    match Version::parse(s) {
        Ok(v) if v.to_string() == s => Some(v),
        _ => None,
    }
}

/// Load the state. IO errors other than "not found" are treated as `Corrupt` (fail closed) — a
/// permission or read failure on security state must not be interpreted as "no floor".
pub fn load_state(path: &Path) -> LoadedState {
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return LoadedState::Missing,
        Err(_) => return LoadedState::Corrupt,
    };
    let parsed: StateFile = match serde_json::from_str(&contents) {
        Ok(p) => p,
        Err(_) => return LoadedState::Corrupt,
    };
    if parsed.schema != SCHEMA {
        return LoadedState::Corrupt;
    }
    let Some(floor) = parse_canonical(&parsed.floor) else {
        return LoadedState::Corrupt;
    };
    // A present-but-unparseable `pending` is corruption, not "no pending" — fail closed.
    let pending = match parsed.pending {
        None => None,
        Some(ref s) => match parse_canonical(s) {
            Some(v) => Some(v),
            None => return LoadedState::Corrupt,
        },
    };
    LoadedState::Valid { floor, pending }
}

fn gt(a: &Version, b: &Version) -> bool {
    a.cmp_precedence(b) == std::cmp::Ordering::Greater
}

/// Is `candidate` acceptable given the running version and the loaded state? Uses SemVer PRECEDENCE
/// (build metadata ignored, prerelease ordered correctly). A `Corrupt` state rejects everything
/// (fail closed).
///
/// Rules (codex audit #5 — retryable intent semantics):
/// - strictly above the running version (`current`);
/// - strictly above the confirmed `floor` (no rollback below a version that ran healthy);
/// - `>= pending` — i.e. NOT below the pending install intent (a lower still-signed version would
///   regress the committed install), but candidate `== pending` IS allowed so a failed/interrupted
///   install can be RETRIED with the exact same version without permanently poisoning it. Recording
///   the intent BEFORE install (see [`record_pending`]) is what makes this the anti-downgrade floor
///   even on platforms where `install()` never returns to record it afterwards (Windows exits mid-install).
pub fn candidate_allowed(candidate: &Version, current: &Version, state: &LoadedState) -> bool {
    if !gt(candidate, current) {
        return false;
    }
    match state {
        LoadedState::Corrupt => false,
        LoadedState::Missing => true,
        LoadedState::Valid { floor, pending } => {
            if !gt(candidate, floor) {
                return false;
            }
            match pending {
                Some(p) => !gt(p, candidate), // candidate >= pending (equal = retry the exact intent)
                None => true,
            }
        }
    }
}

/// True iff the running build is BELOW the confirmed FLOOR — a rollback that already happened (someone
/// installed an older binary out of band). Uses the confirmed `floor` ONLY, never `pending`: being
/// below `pending` is the NORMAL state while a higher install has committed but not yet launched.
pub fn running_below_floor(current: &Version, state: &LoadedState) -> bool {
    matches!(state, LoadedState::Valid { floor, .. } if gt(floor, current))
}

/// Record that `current` (the running, therefore proven, installer) is COMMITTING to install
/// `candidate`, which becomes the PENDING intent until a build at that version proves healthy.
/// Advances `floor` to include `current` and `pending` to include `candidate` (both monotonic by
/// precedence). MUST be called under the updater lock, and — codex #5 — right BEFORE `install()`
/// (not after): on Windows `tauri-plugin-updater`'s `install()` dispatches the external installer and
/// `std::process::exit(0)`s, so anything after it (including this record) never runs. Recording the
/// intent first is what raises the anti-downgrade floor for a racing instance regardless of platform,
/// and [`candidate_allowed`] permits re-attempting the exact `candidate` so a failed install does not
/// poison that version. Refuses a `Corrupt` file.
pub fn record_pending(path: &Path, current: &Version, candidate: &Version) -> std::io::Result<()> {
    let (floor, pending) = match load_state(path) {
        LoadedState::Corrupt => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "refusing to overwrite a corrupt version-floor state",
            ));
        }
        LoadedState::Missing => (current.clone(), Some(candidate.clone())),
        LoadedState::Valid { floor, pending } => {
            let new_floor = if gt(current, &floor) {
                current.clone()
            } else {
                floor
            };
            let new_pending = match pending {
                Some(p) if gt(&p, candidate) => Some(p),
                _ => Some(candidate.clone()),
            };
            (new_floor, new_pending)
        }
    };
    write_state(path, &floor, pending.as_ref())
}

/// Record that `current` launched successfully: advance `floor` to `max(floor, current)` and clear any
/// `pending` that has now been superseded (i.e. `pending <= current` by precedence — the pending
/// install has launched, so it graduates into the confirmed floor). Monotonic; never lowers. Refuses a
/// `Corrupt` file. MUST be called under the updater lock.
pub fn commit_successful_launch(path: &Path, current: &Version) -> std::io::Result<()> {
    match load_state(path) {
        LoadedState::Corrupt => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "refusing to overwrite a corrupt version-floor state",
        )),
        LoadedState::Missing => write_state(path, current, None),
        LoadedState::Valid { floor, pending } => {
            let new_floor = if gt(current, &floor) {
                current.clone()
            } else {
                floor
            };
            // Drop pending once the running version has caught up to (or passed) it.
            let new_pending = pending.filter(|p| gt(p, current));
            write_state(path, &new_floor, new_pending.as_ref())
        }
    }
}

/// Atomically write the state with owner-only perms: random temp name in the SAME dir (via
/// `tempfile`), explicit `0600`, `fsync` of the file AND the parent dir on Unix, chmod/durability
/// failures are HARD errors (not ignored — L8).
fn write_state(path: &Path, floor: &Version, pending: Option<&Version>) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "state path has no parent")
    })?;
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }

    let body = serde_json::to_vec(&StateFile {
        schema: SCHEMA,
        floor: floor.to_string(),
        pending: pending.map(ToString::to_string),
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

    // fsync the directory so the rename is durable. Propagate failures (L8): a swallowed dir-fsync
    // failure could let a crash restore an older/missing dir entry and lower or erase the floor.
    #[cfg(unix)]
    {
        let dir = std::fs::File::open(parent)?;
        dir.sync_all()?;
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
    fn valid(floor: &str, pending: Option<&str>) -> LoadedState {
        LoadedState::Valid {
            floor: v(floor),
            pending: pending.map(v),
        }
    }

    #[test]
    fn missing_bootstraps() {
        let d = tmp();
        assert_eq!(load_state(&d.path().join("s.json")), LoadedState::Missing);
        assert!(candidate_allowed(
            &v("1.1.0"),
            &v("1.0.0"),
            &LoadedState::Missing
        ));
        assert!(!candidate_allowed(
            &v("1.0.0"),
            &v("1.0.0"),
            &LoadedState::Missing
        ));
    }

    #[test]
    fn commit_is_monotonic_and_roundtrips() {
        let d = tmp();
        let p = d.path().join("s.json");
        commit_successful_launch(&p, &v("1.0.8")).unwrap();
        assert_eq!(load_state(&p), valid("1.0.8", None));
        commit_successful_launch(&p, &v("1.0.5")).unwrap(); // lower never lowers
        assert_eq!(load_state(&p), valid("1.0.8", None));
        commit_successful_launch(&p, &v("1.1.0")).unwrap();
        assert_eq!(load_state(&p), valid("1.1.0", None));
    }

    #[test]
    fn floor_blocks_rollback_candidate() {
        let f = valid("1.0.8", None);
        assert!(!candidate_allowed(&v("1.0.7"), &v("1.0.6"), &f));
        assert!(!candidate_allowed(&v("1.0.8"), &v("1.0.6"), &f)); // equal to floor
        assert!(candidate_allowed(&v("1.0.9"), &v("1.0.6"), &f));
    }

    #[test]
    fn pending_blocks_lower_candidate_before_commit() {
        // H1 restart race: A installed 3.0.0 (recorded pending), floor still 1.0.0. B (current 1.0.0)
        // must NOT be allowed to install 2.0.0 — the effective floor is now 3.0.0.
        let d = tmp();
        let p = d.path().join("s.json");
        commit_successful_launch(&p, &v("1.0.0")).unwrap();
        record_pending(&p, &v("1.0.0"), &v("3.0.0")).unwrap();
        let st = load_state(&p);
        assert_eq!(st, valid("1.0.0", Some("3.0.0")));
        assert!(!candidate_allowed(&v("2.0.0"), &v("1.0.0"), &st)); // regression blocked
        assert!(candidate_allowed(&v("3.0.1"), &v("1.0.0"), &st)); // strictly-higher allowed
                                                                   // codex #5: retrying the EXACT pending intent is allowed (a failed/interrupted install must not
                                                                   // poison that version); anything strictly below it stays blocked.
        assert!(candidate_allowed(&v("3.0.0"), &v("1.0.0"), &st)); // retry the exact intent
        assert!(!candidate_allowed(&v("2.9.9"), &v("1.0.0"), &st)); // still below intent → blocked
                                                                    // running below the confirmed FLOOR is false (1.0.0 == floor); pending doesn't count as floor.
        assert!(!running_below_floor(&v("1.0.0"), &st));
    }

    #[test]
    fn commit_promotes_pending_into_floor() {
        let d = tmp();
        let p = d.path().join("s.json");
        commit_successful_launch(&p, &v("1.0.0")).unwrap();
        record_pending(&p, &v("1.0.0"), &v("2.0.0")).unwrap();
        assert_eq!(load_state(&p), valid("1.0.0", Some("2.0.0")));
        // 2.0.0 launches and proves healthy → floor advances to 2.0.0, pending cleared.
        commit_successful_launch(&p, &v("2.0.0")).unwrap();
        assert_eq!(load_state(&p), valid("2.0.0", None));
    }

    #[test]
    fn record_pending_is_monotonic() {
        let d = tmp();
        let p = d.path().join("s.json");
        record_pending(&p, &v("1.0.0"), &v("2.0.0")).unwrap();
        record_pending(&p, &v("1.0.0"), &v("1.5.0")).unwrap(); // lower candidate can't lower pending
        assert_eq!(load_state(&p), valid("1.0.0", Some("2.0.0")));
    }

    #[test]
    fn corrupt_fails_closed_and_is_preserved() {
        let d = tmp();
        let p = d.path().join("s.json");
        std::fs::write(&p, b"{ not json").unwrap();
        assert_eq!(load_state(&p), LoadedState::Corrupt);
        assert!(!candidate_allowed(
            &v("9.9.9"),
            &v("1.0.0"),
            &LoadedState::Corrupt
        ));
        assert!(commit_successful_launch(&p, &v("2.0.0")).is_err());
        assert!(record_pending(&p, &v("1.0.0"), &v("2.0.0")).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{ not json"); // untouched
    }

    #[test]
    fn wrong_schema_or_bad_pending_is_corrupt() {
        let d = tmp();
        let p = d.path().join("s.json");
        std::fs::write(&p, br#"{"schema":99,"floor":"1.0.0"}"#).unwrap();
        assert_eq!(load_state(&p), LoadedState::Corrupt);
        std::fs::write(&p, br#"{"schema":1,"floor":"1.0.0","pending":"notver"}"#).unwrap();
        assert_eq!(load_state(&p), LoadedState::Corrupt);
        // Unknown field rejected.
        std::fs::write(&p, br#"{"schema":1,"floor":"1.0.0","x":1}"#).unwrap();
        assert_eq!(load_state(&p), LoadedState::Corrupt);
    }

    #[test]
    fn prerelease_and_build_metadata_precedence() {
        let f = valid("1.0.8-rc.2", None);
        assert!(candidate_allowed(&v("1.0.8-rc.10"), &v("1.0.7"), &f)); // rc.10 > rc.2 numerically
        assert!(candidate_allowed(&v("1.0.8"), &v("1.0.7"), &f)); // stable > its rc
        let f2 = valid("1.0.8", None);
        assert!(!candidate_allowed(&v("1.0.8+build.5"), &v("1.0.0"), &f2)); // build metadata ignored
    }

    #[test]
    fn running_below_floor_detects_rollback() {
        assert!(running_below_floor(&v("1.0.5"), &valid("1.0.8", None)));
        assert!(!running_below_floor(&v("1.0.8"), &valid("1.0.8", None)));
        assert!(!running_below_floor(&v("1.0.9"), &valid("1.0.8", None)));
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
