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
//! strictly greater than `max(current_running, floor)` AND not below a LIVE `pending` (candidate
//! `== pending` is allowed, so a failed install can be RETRIED with the exact intent without poisoning
//! it; a lower version stays blocked).
//!
//! ## The floor is CLAMPED to the running build, and corruption self-heals (F-04, audit 2026-07-31)
//!
//! This file used to fail CLOSED without bound: a `Corrupt` load rejected every candidate forever, a
//! `floor` above the running build refused ALL updates, and both mutators refused to overwrite a
//! corrupt file — so a single ~40-byte write (`{"schema":1,"floor":"999.0.0"}`), or a USER
//! deliberately downgrading to an older build, permanently and silently disabled the security-update
//! channel. Nothing repaired it, and the Windows uninstaller does not remove this file, so even a
//! reinstall did not clear it.
//!
//! The bound is now explicit:
//! - **`Corrupt` is recoverable.** It no longer rejects candidates, and the next mutation QUARANTINES
//!   the unreadable file (renamed aside, so the forensic copy survives) and rebuilds from `current`.
//! - **A floor above the running build is REPAIRED, not obeyed forever.** The gate no longer refuses
//!   every candidate just because the floor is higher than the running build (that veto was the
//!   lockout); and the next successful-launch commit RESETS the floor to the version that is actually
//!   running, so a forged or stranded floor survives at most one update-check cycle. Candidates below
//!   a live floor are still blocked — that part is the feature.
//! - **`pending` expires.** Its job is to survive the install→restart window (seconds); a `pending`
//!   older than [`PENDING_TTL_SECS`] is treated as a failed install and dropped.
//!
//! What this trades away: protection against an out-of-band downgrade of the BINARY ITSELF, which
//! requires an attacker who already owns the installed app files. What it keeps — unconditionally, and
//! independent of this file — is [`candidate_allowed`]'s `candidate > current` rule, the load-bearing
//! anti-rollback property. Availability of the patch channel outranks a control that only binds an
//! attacker who has already won.
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

/// How long a `pending` install intent stays authoritative. Its purpose is to bridge the
/// install→restart window (seconds), so anything this old is a failed install, not a live intent.
/// Bounding it is what stops a forged `pending` from wedging the channel the way a forged `floor`
/// used to (F-04).
pub const PENDING_TTL_SECS: u64 = 24 * 60 * 60;

/// How far ahead of `now` a `pending_at` may legitimately sit — an NTP step or a wrong RTC between
/// the write and the read. Anything further is a stamp we could not have written; see
/// [`pending_is_live`].
pub const CLOCK_SKEW_TOLERANCE_SECS: u64 = 15 * 60;

/// Wall-clock seconds since the Unix epoch. Callers pass it in so every decision here stays pure and
/// unit-testable; a pre-epoch clock yields 0, which simply expires any pending.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

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
    /// Unix seconds when `pending` was recorded. Absent on files written before F-04 — treated as
    /// EXPIRED, which only ever permits more updates (the floor still guards regressions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_at: Option<u64>,
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

/// Load the state. IO errors other than "not found" still yield `Corrupt` — a permission or read
/// failure must not be read as "no floor" — but `Corrupt` is now RECOVERABLE rather than terminal
/// (see the module docs). `now` is used to expire a stale `pending`.
pub fn load_state(path: &Path, now: u64) -> LoadedState {
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
    // F-04: expire a stale intent. A missing `pending_at` (pre-F-04 file) counts as expired.
    let pending = pending.filter(|p| match parsed.pending_at {
        Some(at) if pending_is_live(at, now) => true,
        _ => {
            tracing::warn!(
                pending = %p,
                "dropping a stale, undated, or future-dated pending install intent"
            );
            false
        }
    });
    LoadedState::Valid { floor, pending }
}

/// Is a `pending_at` stamp still inside the TTL *window*, in both directions?
///
/// The one-sided `now.saturating_sub(at) <= TTL` this replaces was defeated by a stamp in the
/// FUTURE: `saturating_sub` floors at 0, so `pending_at: 18446744073709551615` read as "0 seconds
/// old" forever and blocked every lower candidate permanently — the exact wedge the TTL exists to
/// prevent, reachable with one field of a user-writable file. (Found by the codex pass over F-04.)
///
/// A small forward tolerance is still allowed: `record_pending` stamps the wall clock, so an NTP
/// correction or a DST-confused RTC between write and read can legitimately put the stamp slightly
/// ahead. Beyond that, a stamp we could not have written is treated as expired.
fn pending_is_live(at: u64, now: u64) -> bool {
    if at > now {
        return at - now <= CLOCK_SKEW_TOLERANCE_SECS;
    }
    now - at <= PENDING_TTL_SECS
}

/// The `pending_at` currently on disk, if the file is readable and well-formed. [`load_state`]
/// deliberately does not surface it (it is an expiry input, not part of the decision), but the
/// mutators need it in order to preserve rather than refresh it.
fn stored_pending_at(path: &Path) -> Option<u64> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice::<StateFile>(&bytes).ok()?.pending_at
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
        // F-04: NOT `false`. Rejecting on corruption disabled the security-update channel permanently
        // and silently — reachable by one local write or a truncated restore, and never repaired. The
        // anti-rollback property that matters (`candidate > current`) is enforced above and does not
        // depend on this file. The next mutation quarantines and rebuilds it.
        LoadedState::Corrupt => true,
        LoadedState::Missing => true,
        LoadedState::Valid { floor, pending } => {
            // The floor is enforced literally here — blocking a candidate BELOW a version that
            // already ran is the whole point. What used to make this unrecoverable was the caller's
            // blanket "running below the floor ⇒ refuse everything" veto (removed) plus the floor
            // never being repairable (now reset by `commit_successful_launch`), NOT this comparison.
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

/// True iff the running build is BELOW the confirmed FLOOR — either a rollback that already happened
/// (an older binary installed out of band), a user's own deliberate downgrade, or a forged floor. Uses
/// the confirmed `floor` ONLY, never `pending`: being below `pending` is the NORMAL state while a
/// higher install has committed but not yet launched.
///
/// F-04: this is a **diagnostic signal, not a veto**. It used to make the gate refuse every update,
/// which is exactly the permanent silent lockout. Callers log it loudly and continue;
/// [`commit_successful_launch`] then resets the floor to the running build.
pub fn running_below_floor(current: &Version, state: &LoadedState) -> bool {
    matches!(state, LoadedState::Valid { floor, .. } if gt(floor, current))
}

/// Move an unreadable state file aside so the next load bootstraps cleanly, keeping the original for
/// forensics. Best-effort: if the rename fails we still proceed (the caller overwrites), because a
/// file we cannot even rename must not be able to hold the update channel shut (F-04).
fn quarantine_corrupt(path: &Path, now: u64) {
    let aside = path.with_extension(format!("corrupt-{now}"));
    match std::fs::rename(path, &aside) {
        Ok(()) => tracing::error!(
            quarantined = %aside.display(),
            "SECURITY: updater state was unreadable; quarantined it and rebuilt from the running version"
        ),
        Err(e) => tracing::error!(
            error = %e,
            "SECURITY: updater state is unreadable and could not be quarantined; overwriting it"
        ),
    }
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
    let now = now_unix();
    let (floor, pending) = match load_state(path, now) {
        // F-04: quarantine + rebuild rather than erroring forever. Refusing here meant a corrupt file
        // could never be replaced, so the lockout was permanent by construction.
        LoadedState::Corrupt => {
            quarantine_corrupt(path, now);
            (current.clone(), Some(candidate.clone()))
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
    write_state(path, &floor, pending.as_ref(), Some(now))
}

/// Record that `current` launched successfully: advance `floor` to `max(floor, current)` and clear any
/// `pending` that has now been superseded (i.e. `pending <= current` by precedence — the pending
/// install has launched, so it graduates into the confirmed floor). Monotonic; never lowers. Refuses a
/// `Corrupt` file. MUST be called under the updater lock.
pub fn commit_successful_launch(path: &Path, current: &Version) -> std::io::Result<()> {
    let now = now_unix();
    let stamp = stored_pending_at(path);
    match load_state(path, now) {
        LoadedState::Corrupt => {
            quarantine_corrupt(path, now);
            write_state(path, current, None, None)
        }
        LoadedState::Missing => write_state(path, current, None, None),
        LoadedState::Valid { floor, pending } => {
            let new_floor = if gt(current, &floor) {
                current.clone()
            } else if gt(&floor, current) {
                // F-04: the stored floor is ABOVE a build that just ran successfully — unsatisfiable,
                // so keeping it would refuse every update forever. Reset to the proven-running version
                // and say so loudly: this is also the only signal a real out-of-band downgrade emits.
                tracing::error!(
                    stored_floor = %floor,
                    running = %current,
                    "SECURITY: version floor is above the running build (out-of-band downgrade, user \
                     downgrade, or a forged state file); resetting it to the running version so the \
                     update channel cannot be permanently disabled"
                );
                current.clone()
            } else {
                floor
            };
            // Drop pending once the running version has caught up to (or passed) it.
            let new_pending = pending.filter(|p| gt(p, current));
            // PRESERVE the original stamp. Re-stamping it with `now` here meant every successful
            // launch pushed the deadline out, so a pending that outlived its install NEVER expired on
            // a machine used daily — the TTL was decorative. Only `record_pending`, which creates the
            // intent, may date it. (Found by the codex pass over F-04.)
            let pending_at = new_pending.as_ref().and(stamp);
            write_state(path, &new_floor, new_pending.as_ref(), pending_at)
        }
    }
}

/// Atomically write the state with owner-only perms: random temp name in the SAME dir (via
/// `tempfile`), explicit `0600`, `fsync` of the file AND the parent dir on Unix, chmod/durability
/// failures are HARD errors (not ignored — L8).
fn write_state(
    path: &Path,
    floor: &Version,
    pending: Option<&Version>,
    pending_at: Option<u64>,
) -> std::io::Result<()> {
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
        pending_at: pending.and(pending_at),
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
    /// A fixed clock for hand-crafted files. Round-trip tests pass this too: `record_pending` stamps
    /// the REAL epoch, and `now.saturating_sub(at)` is 0 when `now` precedes it, so a just-written
    /// intent reads as live under any earlier clock.
    const NOW: u64 = 1_800_000_000;

    fn valid(floor: &str, pending: Option<&str>) -> LoadedState {
        LoadedState::Valid {
            floor: v(floor),
            pending: pending.map(v),
        }
    }

    #[test]
    fn missing_bootstraps() {
        let d = tmp();
        assert_eq!(
            load_state(&d.path().join("s.json"), NOW),
            LoadedState::Missing
        );
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
        assert_eq!(load_state(&p, NOW), valid("1.0.8", None));
        // F-04: running an OLDER build now RESETS the floor to it. Before, the floor was a
        // never-lowering high-water mark, which is exactly what let a forged (or simply higher) floor
        // refuse every update forever. `candidate > current` still blocks regressions unconditionally.
        commit_successful_launch(&p, &v("1.0.5")).unwrap();
        assert_eq!(load_state(&p, NOW), valid("1.0.5", None));
        commit_successful_launch(&p, &v("1.1.0")).unwrap();
        assert_eq!(load_state(&p, NOW), valid("1.1.0", None));
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
        // Written by the code under test, so read it back on the same (real) clock.
        let st = load_state(&p, now_unix());
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
        assert_eq!(load_state(&p, now_unix()), valid("1.0.0", Some("2.0.0")));
        // 2.0.0 launches and proves healthy → floor advances to 2.0.0, pending cleared.
        commit_successful_launch(&p, &v("2.0.0")).unwrap();
        assert_eq!(load_state(&p, NOW), valid("2.0.0", None));
    }

    #[test]
    fn record_pending_is_monotonic() {
        let d = tmp();
        let p = d.path().join("s.json");
        record_pending(&p, &v("1.0.0"), &v("2.0.0")).unwrap();
        record_pending(&p, &v("1.0.0"), &v("1.5.0")).unwrap(); // lower candidate can't lower pending
        assert_eq!(load_state(&p, now_unix()), valid("1.0.0", Some("2.0.0")));
    }

    // ── F-04 regression tests ──────────────────────────────────────────────────────────────────
    // Each of these FAILS against the pre-F-04 code. Together they pin the property that used to be
    // violated: no state this file can hold may permanently disable the update channel.

    /// Pre-F-04 this returned `false`, so one unreadable byte disabled every future security update.
    #[test]
    fn corrupt_state_no_longer_disables_updates() {
        assert!(candidate_allowed(
            &v("9.9.9"),
            &v("1.0.0"),
            &LoadedState::Corrupt
        ));
        // The unconditional rule still holds: never install something at or below what is running.
        assert!(!candidate_allowed(
            &v("1.0.0"),
            &v("1.0.0"),
            &LoadedState::Corrupt
        ));
    }

    /// Pre-F-04 both mutators returned `Err` and left the file in place, so the lockout was permanent
    /// by construction. Now the unreadable file is moved aside (kept for forensics) and rebuilt.
    #[test]
    fn corrupt_state_is_quarantined_and_rebuilt() {
        let d = tmp();
        let p = d.path().join("s.json");
        std::fs::write(&p, b"{ not json").unwrap();
        assert_eq!(load_state(&p, NOW), LoadedState::Corrupt);

        commit_successful_launch(&p, &v("2.0.0")).unwrap();
        assert_eq!(load_state(&p, NOW), valid("2.0.0", None));

        // The original bytes survive beside it for diagnosis.
        let aside: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("s.corrupt-"))
            .collect();
        assert_eq!(
            aside.len(),
            1,
            "expected exactly one quarantined file: {aside:?}"
        );
        assert_eq!(
            std::fs::read_to_string(d.path().join(&aside[0])).unwrap(),
            "{ not json"
        );

        // …and the same holds for the other mutator.
        std::fs::write(&p, b"garbage").unwrap();
        record_pending(&p, &v("1.0.0"), &v("2.0.0")).unwrap();
        assert!(matches!(load_state(&p, NOW), LoadedState::Valid { .. }));
    }

    /// The headline attack: one ~40-byte write (`{"schema":1,"floor":"999.0.0"}`). Pre-F-04 nothing in
    /// the tree could ever lower that floor, so every future update was refused permanently. Now the
    /// first successful launch repairs it. The floor is still honoured against real regressions.
    #[test]
    fn forged_floor_is_repaired_by_the_next_successful_launch() {
        let d = tmp();
        let p = d.path().join("s.json");
        std::fs::write(&p, br#"{"schema":1,"floor":"999.0.0"}"#).unwrap();

        // Before repair the forged floor is visible as an anomaly and does gate candidates…
        let forged = load_state(&p, NOW);
        assert!(running_below_floor(&v("1.0.8"), &forged));
        assert!(!candidate_allowed(&v("1.0.9"), &v("1.0.8"), &forged));

        // …but a launch of the running build resets it, so the channel reopens. Pre-F-04 this call
        // preserved 999.0.0 (monotonic max) and the lockout was permanent.
        commit_successful_launch(&p, &v("1.0.8")).unwrap();
        assert_eq!(load_state(&p, NOW), valid("1.0.8", None));
        assert!(candidate_allowed(
            &v("1.0.9"),
            &v("1.0.8"),
            &load_state(&p, NOW)
        ));
    }

    /// The non-adversarial half: a user downgrades to an older build (the normal response to a bad
    /// release). Pre-F-04 the higher floor blocked every candidate below it, so they were stranded.
    #[test]
    fn user_downgrade_leaves_the_channel_open_and_self_heals() {
        let d = tmp();
        let p = d.path().join("s.json");
        commit_successful_launch(&p, &v("2.0.0")).unwrap();

        // They are now running 1.0.8, below the 2.0.0 floor. Pre-F-04 `layer_b_gate` refused ALL
        // updates in this state (see updater.rs) and nothing ever lowered the floor, so the user was
        // stranded on the build they downgraded to — including past the fix. Now the launch commit
        // repairs the floor and the channel reopens.
        assert!(running_below_floor(&v("1.0.8"), &load_state(&p, NOW)));
        commit_successful_launch(&p, &v("1.0.8")).unwrap();
        assert_eq!(load_state(&p, NOW), valid("1.0.8", None));
        assert!(candidate_allowed(
            &v("1.0.9"),
            &v("1.0.8"),
            &load_state(&p, NOW)
        ));
    }

    /// Round 2 (codex pass over F-04): the TTL was one-sided. `now.saturating_sub(at)` floors at 0,
    /// so a stamp in the FUTURE read as "0 seconds old" — forever. One field of a user-writable file
    /// restored the exact permanent wedge the TTL exists to prevent.
    #[test]
    fn a_future_pending_stamp_does_not_read_as_fresh() {
        let d = tmp();
        let p = d.path().join("s.json");
        std::fs::write(
            &p,
            format!(
                r#"{{"schema":1,"floor":"1.0.0","pending":"999.0.0","pending_at":{}}}"#,
                u64::MAX
            )
            .as_bytes(),
        )
        .unwrap();
        assert_eq!(load_state(&p, NOW), valid("1.0.0", None));
        assert!(candidate_allowed(
            &v("1.0.9"),
            &v("1.0.8"),
            &load_state(&p, NOW)
        ));

        // A modest forward skew is still honoured — `record_pending` stamps the wall clock, and an
        // NTP step between write and read is not an attack.
        let skewed = NOW + CLOCK_SKEW_TOLERANCE_SECS - 1;
        std::fs::write(
            &p,
            format!(r#"{{"schema":1,"floor":"1.0.0","pending":"3.0.0","pending_at":{skewed}}}"#)
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(load_state(&p, NOW), valid("1.0.0", Some("3.0.0")));
    }

    /// Round 2 (codex pass over F-04): `commit_successful_launch` re-stamped a SURVIVING pending with
    /// `now`, so every launch pushed the deadline out and the TTL never elapsed on a machine used
    /// daily. Trigger: an install of 3.0.0 fails while 1.0.0 keeps running — the withdrawn release's
    /// intent blocked its own replacement indefinitely.
    #[test]
    fn a_surviving_pending_keeps_its_original_deadline_across_launches() {
        let d = tmp();
        let p = d.path().join("s.json");
        // The REAL clock: `commit_successful_launch` stamps and expires against `now_unix()`, and the
        // fixed `NOW` above is a later epoch — under the new future-stamp rule a fixture dated from it
        // would read as future-dated and be dropped before this test could observe anything.
        let base = now_unix();
        // An intent dated one hour before the TTL runs out.
        let stamped = base - PENDING_TTL_SECS + 3600;
        std::fs::write(
            &p,
            format!(r#"{{"schema":1,"floor":"1.0.0","pending":"3.0.0","pending_at":{stamped}}}"#)
                .as_bytes(),
        )
        .unwrap();

        // 1.0.0 launches successfully; 3.0.0 never installed, so the intent survives…
        commit_successful_launch(&p, &v("1.0.0")).unwrap();
        assert_eq!(load_state(&p, base), valid("1.0.0", Some("3.0.0")));

        // …but it must NOT have been re-dated. Two hours later it is past the TTL and gone. Pre-fix
        // the commit above rewrote the stamp to `now`, so this still read as live.
        let later = base + 7200;
        assert_eq!(load_state(&p, later), valid("1.0.0", None));
        assert!(candidate_allowed(
            &v("2.0.0"),
            &v("1.0.0"),
            &load_state(&p, later)
        ));
    }

    /// A forged `pending` wedges exactly like a forged floor unless it expires.
    #[test]
    fn stale_pending_expires_but_a_live_one_still_blocks() {
        let d = tmp();
        let p = d.path().join("s.json");

        // Undated (pre-F-04 shape) → treated as expired → cannot block.
        std::fs::write(&p, br#"{"schema":1,"floor":"1.0.0","pending":"999.0.0"}"#).unwrap();
        assert_eq!(load_state(&p, NOW), valid("1.0.0", None));

        // Dated but older than the TTL → expired.
        let stale = NOW - PENDING_TTL_SECS - 1;
        std::fs::write(
            &p,
            format!(r#"{{"schema":1,"floor":"1.0.0","pending":"999.0.0","pending_at":{stale}}}"#)
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(load_state(&p, NOW), valid("1.0.0", None));
        assert!(candidate_allowed(
            &v("1.0.9"),
            &v("1.0.8"),
            &load_state(&p, NOW)
        ));

        // Fresh → the restart-race guarantee still holds (H1): a lower candidate stays blocked.
        std::fs::write(
            &p,
            format!(r#"{{"schema":1,"floor":"1.0.0","pending":"3.0.0","pending_at":{NOW}}}"#)
                .as_bytes(),
        )
        .unwrap();
        let live = load_state(&p, NOW);
        assert_eq!(live, valid("1.0.0", Some("3.0.0")));
        assert!(!candidate_allowed(&v("2.0.0"), &v("1.0.0"), &live));
        assert!(candidate_allowed(&v("3.0.0"), &v("1.0.0"), &live)); // exact retry still allowed
    }

    #[test]
    fn wrong_schema_or_bad_pending_is_corrupt() {
        let d = tmp();
        let p = d.path().join("s.json");
        std::fs::write(&p, br#"{"schema":99,"floor":"1.0.0"}"#).unwrap();
        assert_eq!(load_state(&p, NOW), LoadedState::Corrupt);
        std::fs::write(&p, br#"{"schema":1,"floor":"1.0.0","pending":"notver"}"#).unwrap();
        assert_eq!(load_state(&p, NOW), LoadedState::Corrupt);
        // Unknown field rejected.
        std::fs::write(&p, br#"{"schema":1,"floor":"1.0.0","x":1}"#).unwrap();
        assert_eq!(load_state(&p, NOW), LoadedState::Corrupt);
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
