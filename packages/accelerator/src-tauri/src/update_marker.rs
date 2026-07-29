//! The Windows update-window marker (plan D18/D21/D22; piece-2 plan §2-§4).
//!
//! Windows `install()` hands off to NSIS and calls `std::process::exit(0)`, so the updater's file
//! lock dies exactly when the danger starts: for a few seconds the installed exe is deleted and
//! not yet rewritten, and any instance that runs the autostart heal in that window "repairs" the
//! entry to a transient copy. The marker is the cross-process do-not-disturb sign for that window:
//! while one is live, no process heals, no process rearms crash recovery, no process starts a new
//! update, and explicit autostart-ON is rejected (OFF always works).
//!
//! Removal (next launch of the installed candidate) requires ALL FOUR: matching transaction nonce
//! (via the NSIS-written completion token), matching candidate version, matching canonical install
//! path (with the settled rename-tolerance clause), and crash recovery reconciled to CURRENT
//! intent. The in-payload deadline is a LIVENESS heuristic only — it recovers orphaned windows, it
//! is never trusted as a safety bound (an installer hung past it reopens the race; nothing here
//! pretends otherwise). File mtime is never consulted: it is forgeable by the same user who can
//! write the file.
//!
//! Everything that mutates recovery is expressed over INJECTED closures (the house
//! `enable_transaction` pattern) so the transaction tables test on every OS with counters — no
//! test ever touches the real scheduled task (piece-1 rule). Lock discipline is the CALLER's
//! (autostart.lock, held across load → decide → act; see plan §4) — this module is deliberately
//! lock-free so the tables stay pure.

use std::path::{Path, PathBuf};

/// Marker lifetime from creation. NSIS on this app installs in seconds; 15 minutes absorbs AV
/// interference and slow disks (plan §5.3).
pub const DEADLINE_SECS: i64 = 15 * 60;
/// A deadline further out than this is not a window, it is a bug or a forgery — classified
/// `Corrupt` at load so it cannot suppress for years (plan §5.2/§5.3).
pub const MAX_FUTURE_SECS: i64 = 24 * 60 * 60;
/// §9 bounded reads — these files are tens of bytes.
const MAX_READ_BYTES: u64 = 64 * 1024;

const SCHEMA: u32 = 1;

/// The three sibling files of one update transaction. Grouped so tests inject a temp dir and no
/// path is ever derived twice (piece-1 lesson: one derivation or the platforms drift).
#[derive(Debug, Clone)]
pub struct MarkerPaths {
    /// `update-in-progress.json` — the marker itself.
    pub marker: PathBuf,
    /// `update-txn` — the plain-text nonce handoff the updater writes for NSIS.
    pub handoff: PathBuf,
    /// `update-txn-done` — the completion token NSIS renames the handoff into.
    pub token: PathBuf,
}

impl MarkerPaths {
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            marker: dir.join("update-in-progress.json"),
            handoff: dir.join("update-txn"),
            token: dir.join("update-txn-done"),
        }
    }

    /// Production location: `~/.aztec-accelerator/` (same dir as `updater-state.json` and the
    /// locks). `None` if the home directory is unresolvable.
    pub fn default_paths() -> Option<Self> {
        Some(Self::in_dir(&dirs::home_dir()?.join(".aztec-accelerator")))
    }
}

/// The persisted marker. `deny_unknown_fields`: a tampered/extended file is `Corrupt`, never
/// silently accepted (mirrors `updater_state.rs`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkerPayload {
    pub schema: u32,
    /// Transaction nonce — the completion token must hand this exact value back.
    pub txn: String,
    /// Candidate version, canonical SemVer. Removal rule #1 ONLY — floor/gate semantics belong to
    /// `updater_state.rs` and are never derived from this copy (recon §C).
    pub candidate: String,
    /// Where the installed exe will live, canonicalized at creation. Removal rule #2.
    pub expected_install_path: String,
    /// Intent at disarm time (plan D18). Diagnostic; reconciliation always reads CURRENT intent.
    pub intent_at_disarm: bool,
    /// Absolute expiry, unix seconds, in-payload (mtime is never trusted).
    pub deadline_unix: i64,
}

impl MarkerPayload {
    pub fn new(
        candidate: &semver::Version,
        expected_install_path: &Path,
        intent_at_disarm: bool,
        now_unix: i64,
    ) -> Self {
        Self {
            schema: SCHEMA,
            txn: uuid::Uuid::new_v4().to_string(),
            candidate: candidate.to_string(),
            expected_install_path: expected_install_path.to_string_lossy().into_owned(),
            intent_at_disarm,
            deadline_unix: now_unix + DEADLINE_SECS,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum LoadedMarker {
    /// No marker — the common case. Never anything to do.
    Missing,
    /// Unreadable, wrong schema, non-canonical version, or future-dated beyond the clamp. The exit
    /// is delete + ONE-launch suppression (plan §5.2) — a corrupt payload has no readable deadline,
    /// so the expire path cannot apply, and fail-closed-forever was withdrawn upstream (r4).
    Corrupt,
    Valid(MarkerPayload),
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Bounded read helper (§9): refuses implausibly large files rather than loading them.
fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>, String> {
    use std::io::Read as _;
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot open {}: {e}", path.display())),
    };
    let mut buf = Vec::new();
    file.take(MAX_READ_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if buf.len() as u64 > MAX_READ_BYTES {
        return Err(format!("{} is implausibly large", path.display()));
    }
    Ok(Some(buf))
}

/// Load + classify. `now_unix` is a parameter (not sampled inside) so the truth tables are pure.
pub fn load(paths: &MarkerPaths, now_unix: i64) -> LoadedMarker {
    let bytes = match read_bounded(&paths.marker) {
        Ok(None) => return LoadedMarker::Missing,
        // A real I/O failure on the marker is indistinguishable from tampering for our purposes:
        // classify Corrupt (delete + one-launch suppression) rather than inventing a fourth state.
        Ok(Some(b)) => b,
        Err(_) => return LoadedMarker::Corrupt,
    };
    let parsed: MarkerPayload = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(_) => return LoadedMarker::Corrupt,
    };
    if parsed.schema != SCHEMA {
        return LoadedMarker::Corrupt;
    }
    // Canonical round-trip, as updater_state does — a non-canonical version string cannot slip the
    // removal comparator.
    match semver::Version::parse(&parsed.candidate) {
        Ok(v) if v.to_string() == parsed.candidate => {}
        _ => return LoadedMarker::Corrupt,
    }
    // Future-date clamp (plan §5.2): a deadline further than the clamp is Corrupt, NOT a longer
    // window — one semantics, everywhere.
    if parsed.deadline_unix > now_unix + MAX_FUTURE_SECS {
        return LoadedMarker::Corrupt;
    }
    LoadedMarker::Valid(parsed)
}

/// Is there a live (valid, unexpired) window right now?
pub fn is_live(loaded: &LoadedMarker, now_unix: i64) -> bool {
    matches!(loaded, LoadedMarker::Valid(p) if now_unix < p.deadline_unix)
}

/// Convenience for the locked re-checks (heal step, startup_rearm, set_enabled ON branch).
pub fn live_marker_exists(paths: &MarkerPaths, now_unix: i64) -> bool {
    is_live(&load(paths, now_unix), now_unix)
}

#[derive(Debug, PartialEq)]
pub enum CreateErr {
    /// A live foreign window exists — NEVER deleted, never replaced (D22).
    Live,
    Io(String),
}

/// Compare-and-create (D22). Caller MUST hold `autostart.lock` — that hold is what makes the
/// locked re-checks at the mutation sites race-free. An existing marker is reloaded under that
/// same hold: live ⇒ `CreateErr::Live`; expired/corrupt ⇒ deleted, then ONE create retry.
pub fn create_new(
    paths: &MarkerPaths,
    payload: &MarkerPayload,
    now_unix: i64,
) -> Result<(), CreateErr> {
    let bytes = serde_json::to_vec_pretty(payload)
        .map_err(|e| CreateErr::Io(format!("cannot serialize marker: {e}")))?;
    if let Some(parent) = paths.marker.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CreateErr::Io(format!("cannot create state dir: {e}")))?;
    }
    for attempt in 0..2 {
        match try_create_exclusive(&paths.marker, &bytes) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt == 0 => {
                match load(paths, now_unix) {
                    // Live foreign window: hard stop, never delete (D22 / final pass #2).
                    l if is_live(&l, now_unix) => return Err(CreateErr::Live),
                    // Expired or corrupt leftover: clear it (and its siblings) and retry once.
                    _ => {
                        let _ = remove_all(paths);
                    }
                }
            }
            Err(e) => return Err(CreateErr::Io(format!("cannot create marker: {e}"))),
        }
    }
    Err(CreateErr::Io(
        "marker reappeared during creation".to_string(),
    ))
}

/// Exclusive creation: `CREATE_NEW` semantics on every platform; owner-private + reparse-safe on
/// Windows via the existing `win_acl` primitive.
fn try_create_exclusive(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    #[cfg(windows)]
    let mut f = accelerator_core::win_acl::secure_create_file(path)?;
    #[cfg(not(windows))]
    let mut f = {
        use std::os::unix::fs::OpenOptionsExt as _;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?
    };
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

/// The nonce handoff for NSIS (plan §3): a single plain-text line the POSTINSTALL hook renames
/// into the completion token. Owner-writes with plain create (truncate) — the transaction that
/// writes it holds the lock and just created the marker.
pub fn write_handoff(paths: &MarkerPaths, txn: &str) -> Result<(), String> {
    std::fs::write(&paths.handoff, txn.as_bytes())
        .map_err(|e| format!("cannot write update handoff: {e}"))
}

/// Read the completion token's nonce, if present. Trailing whitespace tolerated (NSIS Rename
/// preserves bytes, but be liberal in what a one-line file may carry).
pub fn read_token_nonce(paths: &MarkerPaths) -> Option<String> {
    let bytes = read_bounded(&paths.token).ok().flatten()?;
    let s = String::from_utf8(bytes).ok()?;
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Remove the whole transaction: marker + token + handoff. Missing files are success; the FIRST
/// real error is returned (removal success gates rearms — plan §4).
pub fn remove_all(paths: &MarkerPaths) -> Result<(), String> {
    let mut first_err = None;
    for p in [&paths.marker, &paths.handoff, &paths.token] {
        match std::fs::remove_file(p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                first_err.get_or_insert(format!("cannot remove {}: {e}", p.display()));
            }
        }
    }
    match first_err {
        None => Ok(()),
        Some(e) => Err(e),
    }
}

#[derive(Debug, PartialEq)]
pub enum Decision {
    /// All four removal conditions can be satisfied — the caller reconciles then removes.
    Remove,
    /// The window is live and this process cannot close it. No heal, no rearm.
    Suppress(&'static str),
    /// Deadline passed — remove with ZERO reconciliation (the T3 discriminator).
    Expire(&'static str),
}

/// The pure removal rule (plan §2). `expected_canon` is the canonicalized
/// `expected_install_path` — `None` means the path is PROVEN absent (`try_exists() == Ok(false)`,
/// never a canonicalize/permission failure — the caller suppresses those), which triggers the
/// settled rename-tolerance clause (a renamed build runs from a path ≠ expected; if the version
/// and token match, the install demonstrably completed).
pub fn removal_decision(
    m: &MarkerPayload,
    token_nonce: Option<&str>,
    running: &semver::Version,
    exe_canon: &Path,
    expected_canon: Option<&Path>,
    now_unix: i64,
) -> Decision {
    if now_unix >= m.deadline_unix {
        return Decision::Expire("deadline passed");
    }
    match token_nonce {
        None => return Decision::Suppress("no completion token"),
        Some(t) if t != m.txn => return Decision::Suppress("token nonce mismatch"),
        Some(_) => {}
    }
    if running.to_string() != m.candidate {
        return Decision::Suppress("running version is not the candidate");
    }
    match expected_canon {
        Some(expected) if expected == exe_canon => Decision::Remove,
        // Rename-tolerance: expected path gone, but the matching-version candidate is running —
        // valid completion (upstream plan.md:184-188).
        None => Decision::Remove,
        Some(_) => Decision::Suppress("running from a different path than expected"),
    }
}

#[derive(Debug, PartialEq)]
pub enum ReconcileOutcome {
    /// Normal life may resume: heal, then rearm.
    Proceed,
    /// A window is (or must be treated as) live — skip heal AND rearm this launch.
    Suppressed(&'static str),
}

/// The startup removal transaction (plan §4). CALLER holds `autostart.lock` for the whole call —
/// load, classify, decide, and act all happen under one hold (final pass #2: a pre-lock load can
/// delete a marker it did not classify). Recovery actions are injected:
/// - `intent_read`: current intent (D13), NOT the stored snapshot (r5).
/// - `arm`: idempotent `enable_crash_recovery`.
/// - `disarm_confirmed`: `disable_crash_recovery`, true only when confirmed gone.
///
/// This transaction is the ONLY rearm path allowed while a marker exists (the r5 exemption).
pub fn reconcile_under_lock(
    paths: &MarkerPaths,
    now_unix: i64,
    running: &semver::Version,
    exe_canon: &Path,
    intent_read: &dyn Fn() -> Result<bool, String>,
    arm: &dyn Fn() -> Result<(), String>,
    disarm_confirmed: &dyn Fn() -> bool,
) -> ReconcileOutcome {
    let m = match load(paths, now_unix) {
        LoadedMarker::Missing => return ReconcileOutcome::Proceed,
        LoadedMarker::Corrupt => {
            // Plan §5.2: delete, then ONE launch of conservative suppression. Deleting restores
            // liveness; the single-launch skip keeps the window conservative without fail-closed-
            // forever. Deletion failure changes nothing — still suppressed, retried next launch.
            tracing::warn!("corrupt update marker found; removing (one-launch suppression)");
            let _ = remove_all(paths);
            return ReconcileOutcome::Suppressed("corrupt marker removed");
        }
        LoadedMarker::Valid(m) => m,
    };

    // Expiry DOMINATES every verification failure below: an expected path that stays
    // unverifiable (e.g. permanently broken permissions) must not suppress past the deadline —
    // that would be exactly the TTL-limbo r5 forbids. Zero reconciliation on expiry, as ever.
    if now_unix >= m.deadline_unix {
        tracing::warn!("update marker expired without completion; clearing");
        let _ = remove_all(paths);
        return ReconcileOutcome::Proceed;
    }

    // Rename-tolerance demands PROVEN absence: conflating a permission or transient I/O error
    // with "the path is gone" would widen the settled clause and could remove a marker while
    // running from the wrong path (post-impl audit #3). Unverifiable ⇒ Suppress and retry.
    let expected = PathBuf::from(&m.expected_install_path);
    let expected_canon = match expected.try_exists() {
        Ok(false) => None, // proven absent — the rename-tolerance arm may apply
        Ok(true) => match expected.canonicalize() {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::warn!("expected install path exists but cannot be canonicalized ({e})");
                return ReconcileOutcome::Suppressed("expected path unverifiable");
            }
        },
        Err(e) => {
            tracing::warn!("cannot stat the expected install path ({e})");
            return ReconcileOutcome::Suppressed("expected path unverifiable");
        }
    };
    let token = read_token_nonce(paths);

    match removal_decision(
        &m,
        token.as_deref(),
        running,
        exe_canon,
        expected_canon.as_deref(),
        now_unix,
    ) {
        Decision::Suppress(reason) => ReconcileOutcome::Suppressed(reason),
        Decision::Expire(_) => {
            // Defense in depth: expiry is normally handled by the dominant check above, before
            // path verification. Same semantics if reached: zero reconciliation, clear, proceed.
            tracing::warn!("update marker expired without completion; clearing");
            let _ = remove_all(paths);
            ReconcileOutcome::Proceed
        }
        Decision::Remove => {
            // Reconcile FIRST, remove SECOND — with intent-sensitive requirements (final pass #2):
            // a rearm/disarm that cannot be confirmed leaves the marker in place (Suppressed,
            // retried next launch — r5: "transient schtasks failure at N's first launch: retry,
            // not TTL limbo").
            let intent = match intent_read() {
                Ok(i) => i,
                Err(_) => return ReconcileOutcome::Suppressed("intent unreadable"),
            };
            if intent {
                if arm().is_err() {
                    return ReconcileOutcome::Suppressed("rearm failed; will retry");
                }
            } else if !disarm_confirmed() {
                return ReconcileOutcome::Suppressed("disarm unconfirmed; will retry");
            }
            match remove_all(paths) {
                Ok(()) => ReconcileOutcome::Proceed,
                // Recovery is reconciled; only the flag failed to clear. Suppress and retry —
                // the next launch re-reconciles idempotently.
                Err(_) => ReconcileOutcome::Suppressed("marker removal failed; will retry"),
            }
        }
    }
}

/// Cleanup after a post-create failure inside `perform_update` (handoff-write failure, or the
/// `install()` Err return). CALLER holds `autostart.lock`. Intent-sensitive ordering
/// (final pass #2):
/// - intent ON: remove first (CHECKED), then arm — a failed removal leaves recovery disarmed and
///   the window suppressed until expiry, the safe direction;
/// - intent OFF: confirm the disarm FIRST, then remove — removing first could leave
///   intent-OFF/task-armed permanently if confirmation fails.
///
/// Returns whether the caller's `CrashRecoveryGuard` must be DEFUSED (always true: after this
/// function, any further Drop-rearm would act on stale state).
pub fn post_create_failure_cleanup(
    paths: &MarkerPaths,
    intent_read: &dyn Fn() -> Result<bool, String>,
    arm: &dyn Fn() -> Result<(), String>,
    disarm_confirmed: &dyn Fn() -> bool,
) -> bool {
    // Unreadable intent is NOT assumed ON here (unlike the pre-disarm capture): on this path the
    // marker may already be published and intent may have changed while the lock was released —
    // assuming ON could remove the marker and arm recovery permanently against an OFF intent.
    // The sanctioned exit is suppression: keep the marker, stay disarmed, defuse; the next
    // launch's reconcile retries with a readable intent (post-impl audit #2).
    let intent = match intent_read() {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!("intent unreadable during cleanup ({e}); leaving the window suppressed");
            return true;
        }
    };
    if intent {
        match remove_all(paths) {
            Ok(()) => {
                if let Err(e) = arm() {
                    tracing::warn!("post-cleanup rearm failed: {e}");
                }
            }
            Err(e) => {
                // Marker stays live ⇒ suppression until expiry. Do NOT rearm into a live window.
                tracing::warn!(
                    "marker removal failed after aborted install ({e}); leaving window suppressed"
                );
            }
        }
    } else {
        if disarm_confirmed() {
            if let Err(e) = remove_all(paths) {
                tracing::warn!("marker removal failed after aborted install ({e}); will expire");
            }
        } else {
            tracing::warn!("disarm unconfirmed after aborted install; leaving marker for retry");
        }
    }
    true
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests — T1 (state table), T3 (reconcile transaction), T7 (cleanup transaction).
// All pure/tempdir + counters; nothing here touches locks, registries, or schedulers.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const NOW: i64 = 1_800_000_000;

    fn temp_paths() -> (tempfile::TempDir, MarkerPaths) {
        let d = tempfile::tempdir().unwrap();
        let p = MarkerPaths::in_dir(d.path());
        (d, p)
    }

    fn payload(txn: &str, candidate: &str, expected: &str, deadline: i64) -> MarkerPayload {
        MarkerPayload {
            schema: SCHEMA,
            txn: txn.into(),
            candidate: candidate.into(),
            expected_install_path: expected.into(),
            intent_at_disarm: true,
            deadline_unix: deadline,
        }
    }

    fn write_marker(paths: &MarkerPaths, p: &MarkerPayload) {
        std::fs::write(&paths.marker, serde_json::to_vec(p).unwrap()).unwrap();
    }

    fn ver(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    // ── T1: load/classify ──

    #[test]
    fn t1_load_classes() {
        let (_d, paths) = temp_paths();
        assert_eq!(load(&paths, NOW), LoadedMarker::Missing);

        let good = payload("abc", "1.0.9", "C:\\app.exe", NOW + 600);
        write_marker(&paths, &good);
        assert_eq!(load(&paths, NOW), LoadedMarker::Valid(good.clone()));
        assert!(is_live(&load(&paths, NOW), NOW));
        assert!(
            !is_live(&load(&paths, NOW), NOW + 601),
            "expired is not live"
        );

        // Corrupt classes: garbage, unknown fields, wrong schema, non-canonical version.
        std::fs::write(&paths.marker, b"not json").unwrap();
        assert_eq!(load(&paths, NOW), LoadedMarker::Corrupt);
        std::fs::write(
            &paths.marker,
            serde_json::to_vec(&serde_json::json!({
                "schema": 1, "txn": "a", "candidate": "1.0.9",
                "expected_install_path": "x", "intent_at_disarm": true,
                "deadline_unix": NOW + 1, "extra": 1
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            load(&paths, NOW),
            LoadedMarker::Corrupt,
            "unknown fields fail closed"
        );
        write_marker(&paths, &payload("a", "1.09", "x", NOW + 1)); // non-canonical semver
        assert_eq!(load(&paths, NOW), LoadedMarker::Corrupt);
        let mut wrong = payload("a", "1.0.9", "x", NOW + 1);
        wrong.schema = 2;
        write_marker(&paths, &wrong);
        assert_eq!(load(&paths, NOW), LoadedMarker::Corrupt);
    }

    #[test]
    fn t1_future_dated_is_corrupt_not_expire() {
        // Plan §5.2: one semantics everywhere — beyond the clamp is Corrupt (delete + one-launch
        // suppression), never a longer window and never Expire.
        let (_d, paths) = temp_paths();
        write_marker(
            &paths,
            &payload("a", "1.0.9", "x", NOW + MAX_FUTURE_SECS + 1),
        );
        assert_eq!(load(&paths, NOW), LoadedMarker::Corrupt);
        // At the clamp boundary: still Valid.
        write_marker(&paths, &payload("a", "1.0.9", "x", NOW + MAX_FUTURE_SECS));
        assert!(matches!(load(&paths, NOW), LoadedMarker::Valid(_)));
    }

    // ── T1: removal_decision truth table ──

    #[test]
    fn t1_removal_decision_table() {
        let m = payload("nonce-1", "1.0.9", "/install/app.exe", NOW + 600);
        let running = ver("1.0.9");
        let exe = Path::new("/install/app.exe");
        let expected = Some(Path::new("/install/app.exe"));

        assert_eq!(
            removal_decision(&m, Some("nonce-1"), &running, exe, expected, NOW),
            Decision::Remove
        );
        assert_eq!(
            removal_decision(&m, None, &running, exe, expected, NOW),
            Decision::Suppress("no completion token")
        );
        assert_eq!(
            removal_decision(&m, Some("other"), &running, exe, expected, NOW),
            Decision::Suppress("token nonce mismatch")
        );
        assert_eq!(
            removal_decision(&m, Some("nonce-1"), &ver("1.0.8"), exe, expected, NOW),
            Decision::Suppress("running version is not the candidate")
        );
        assert_eq!(
            removal_decision(
                &m,
                Some("nonce-1"),
                &running,
                Path::new("/elsewhere/app.exe"),
                expected,
                NOW
            ),
            Decision::Suppress("running from a different path than expected")
        );
        // Rename-tolerance: expected path no longer exists (None), version+token match ⇒ Remove.
        assert_eq!(
            removal_decision(
                &m,
                Some("nonce-1"),
                &running,
                Path::new("/renamed/app.exe"),
                None,
                NOW
            ),
            Decision::Remove
        );
        // Expiry dominates everything — even a fully-matching set.
        assert_eq!(
            removal_decision(&m, Some("nonce-1"), &running, exe, expected, NOW + 600),
            Decision::Expire("deadline passed")
        );
    }

    // ── T1: compare-and-create ──

    #[test]
    fn t1_create_new_live_loses_expired_replaced() {
        let (_d, paths) = temp_paths();
        let live = payload("live", "1.0.9", "x", NOW + 600);
        write_marker(&paths, &live);
        let fresh = MarkerPayload::new(&ver("1.0.10"), Path::new("/y"), true, NOW);
        // Live foreign marker: never deleted, never replaced.
        assert_eq!(create_new(&paths, &fresh, NOW), Err(CreateErr::Live));
        assert_eq!(load(&paths, NOW), LoadedMarker::Valid(live));

        // Expired: replaced (and stale siblings cleared).
        write_marker(&paths, &payload("old", "1.0.8", "x", NOW - 1));
        std::fs::write(&paths.token, b"old").unwrap();
        assert_eq!(create_new(&paths, &fresh, NOW), Ok(()));
        match load(&paths, NOW) {
            LoadedMarker::Valid(p) => assert_eq!(p.txn, fresh.txn),
            other => panic!("expected fresh marker, got {other:?}"),
        }
        assert!(
            read_token_nonce(&paths).is_none(),
            "stale token cleared with the old window"
        );

        // Corrupt leftover: also replaced.
        remove_all(&paths).unwrap();
        std::fs::write(&paths.marker, b"garbage").unwrap();
        assert_eq!(create_new(&paths, &fresh, NOW), Ok(()));
    }

    #[test]
    fn t1_create_new_race_one_winner() {
        // CREATE_NEW semantics: with no pre-existing file, two same-instant writers cannot both
        // succeed. Simulate the loser's view: the winner's file appears between its check and
        // create — create_new must yield Live, not clobber.
        let (_d, paths) = temp_paths();
        let a = MarkerPayload::new(&ver("1.0.9"), Path::new("/a"), true, NOW);
        let b = MarkerPayload::new(&ver("1.0.9"), Path::new("/b"), true, NOW);
        assert_eq!(create_new(&paths, &a, NOW), Ok(()));
        assert_eq!(create_new(&paths, &b, NOW), Err(CreateErr::Live));
        match load(&paths, NOW) {
            LoadedMarker::Valid(p) => assert_eq!(p.expected_install_path, "/a"),
            other => panic!("winner clobbered: {other:?}"),
        }
    }

    // ── T3: reconcile transaction (counters; zero real recovery calls) ──

    struct Counters {
        intent: Result<bool, String>,
        arm_ok: bool,
        disarm_ok: bool,
        arms: Cell<u32>,
        disarms: Cell<u32>,
    }

    impl Counters {
        fn new(intent: bool) -> Self {
            Self {
                intent: Ok(intent),
                arm_ok: true,
                disarm_ok: true,
                arms: Cell::new(0),
                disarms: Cell::new(0),
            }
        }
        fn run(&self, paths: &MarkerPaths, running: &str, exe: &str) -> ReconcileOutcome {
            reconcile_under_lock(
                paths,
                NOW,
                &ver(running),
                Path::new(exe),
                &|| self.intent.clone(),
                &|| {
                    self.arms.set(self.arms.get() + 1);
                    if self.arm_ok {
                        Ok(())
                    } else {
                        Err("schtasks failed".into())
                    }
                },
                &|| {
                    self.disarms.set(self.disarms.get() + 1);
                    self.disarm_ok
                },
            )
        }
    }

    /// A marker whose expected path REALLY exists (so canonicalize succeeds and matches).
    fn matching_setup(paths: &MarkerPaths, dir: &Path) -> (PathBuf, MarkerPayload) {
        let exe = dir.join("app-exe");
        std::fs::write(&exe, b"x").unwrap();
        let exe_canon = exe.canonicalize().unwrap();
        let m = payload("nonce-1", "1.0.9", &exe_canon.to_string_lossy(), NOW + 600);
        write_marker(paths, &m);
        std::fs::write(&paths.token, b"nonce-1").unwrap();
        (exe_canon, m)
    }

    #[test]
    fn t3_missing_proceeds_with_zero_calls() {
        let (_d, paths) = temp_paths();
        let c = Counters::new(true);
        assert_eq!(c.run(&paths, "1.0.9", "/x"), ReconcileOutcome::Proceed);
        assert_eq!((c.arms.get(), c.disarms.get()), (0, 0));
    }

    #[test]
    fn t3_remove_arms_once_when_intent_on() {
        let (d, paths) = temp_paths();
        let (exe, _m) = matching_setup(&paths, d.path());
        let c = Counters::new(true);
        assert_eq!(
            c.run(&paths, "1.0.9", &exe.to_string_lossy()),
            ReconcileOutcome::Proceed
        );
        assert_eq!((c.arms.get(), c.disarms.get()), (1, 0));
        assert_eq!(load(&paths, NOW), LoadedMarker::Missing, "marker removed");
        assert!(read_token_nonce(&paths).is_none(), "token removed");
    }

    #[test]
    fn t3_remove_confirm_disarms_once_when_intent_off() {
        let (d, paths) = temp_paths();
        let (exe, _m) = matching_setup(&paths, d.path());
        let c = Counters::new(false);
        assert_eq!(
            c.run(&paths, "1.0.9", &exe.to_string_lossy()),
            ReconcileOutcome::Proceed
        );
        assert_eq!((c.arms.get(), c.disarms.get()), (0, 1));
        assert_eq!(load(&paths, NOW), LoadedMarker::Missing);
    }

    #[test]
    fn t3_failed_rearm_keeps_marker_for_retry() {
        // r5: transient schtasks failure at first launch ⇒ retry, not TTL limbo.
        let (d, paths) = temp_paths();
        let (exe, _m) = matching_setup(&paths, d.path());
        let mut c = Counters::new(true);
        c.arm_ok = false;
        assert_eq!(
            c.run(&paths, "1.0.9", &exe.to_string_lossy()),
            ReconcileOutcome::Suppressed("rearm failed; will retry")
        );
        assert!(
            matches!(load(&paths, NOW), LoadedMarker::Valid(_)),
            "marker kept"
        );
    }

    #[test]
    fn t3_expire_preexisting_removed_proceed_zero_reconciliation() {
        // Final pass #4: assert the expired marker PRE-EXISTED, is removed, outcome is Proceed,
        // AND zero reconciliation calls — the Remove/Expire discriminator.
        let (_d, paths) = temp_paths();
        write_marker(&paths, &payload("n", "1.0.9", "/gone", NOW - 5));
        assert!(
            matches!(load(&paths, NOW), LoadedMarker::Valid(_)),
            "pre-exists (expired but well-formed)"
        );
        let c = Counters::new(true);
        assert_eq!(c.run(&paths, "1.0.9", "/x"), ReconcileOutcome::Proceed);
        assert_eq!(
            (c.arms.get(), c.disarms.get()),
            (0, 0),
            "expiry must not reconcile"
        );
        assert_eq!(
            load(&paths, NOW),
            LoadedMarker::Missing,
            "expired marker removed"
        );
    }

    #[test]
    fn t3_suppress_paths_zero_calls() {
        let (d, paths) = temp_paths();
        let (exe, _m) = matching_setup(&paths, d.path());
        // Token nonce mismatch.
        std::fs::write(&paths.token, b"wrong").unwrap();
        let c = Counters::new(true);
        assert_eq!(
            c.run(&paths, "1.0.9", &exe.to_string_lossy()),
            ReconcileOutcome::Suppressed("token nonce mismatch")
        );
        assert_eq!((c.arms.get(), c.disarms.get()), (0, 0));
        // Intent unreadable.
        std::fs::write(&paths.token, b"nonce-1").unwrap();
        let mut c2 = Counters::new(true);
        c2.intent = Err("io".into());
        assert_eq!(
            c2.run(&paths, "1.0.9", &exe.to_string_lossy()),
            ReconcileOutcome::Suppressed("intent unreadable")
        );
        assert_eq!((c2.arms.get(), c2.disarms.get()), (0, 0));
    }

    #[test]
    fn t3_corrupt_deleted_and_one_launch_suppression() {
        let (_d, paths) = temp_paths();
        std::fs::write(&paths.marker, b"garbage").unwrap();
        let c = Counters::new(true);
        assert_eq!(
            c.run(&paths, "1.0.9", "/x"),
            ReconcileOutcome::Suppressed("corrupt marker removed")
        );
        assert_eq!((c.arms.get(), c.disarms.get()), (0, 0));
        assert_eq!(
            load(&paths, NOW),
            LoadedMarker::Missing,
            "corrupt marker deleted"
        );
        // Next launch: clean.
        assert_eq!(c.run(&paths, "1.0.9", "/x"), ReconcileOutcome::Proceed);
    }

    // ── T7: post-create cleanup transaction ──

    #[test]
    fn t7_intent_on_remove_checked_then_arm() {
        let (_d, paths) = temp_paths();
        write_marker(&paths, &payload("n", "1.0.9", "/x", NOW + 600));
        let c = Counters::new(true);
        let defuse = post_create_failure_cleanup(
            &paths,
            &|| c.intent.clone(),
            &|| {
                c.arms.set(c.arms.get() + 1);
                Ok(())
            },
            &|| {
                c.disarms.set(c.disarms.get() + 1);
                true
            },
        );
        assert!(defuse, "guard must always be defused after cleanup");
        assert_eq!(load(&paths, NOW), LoadedMarker::Missing, "removed first");
        assert_eq!((c.arms.get(), c.disarms.get()), (1, 0), "then armed");
    }

    #[test]
    fn t7_intent_on_removal_failure_means_no_rearm() {
        // Removal failure ⇒ live window stays ⇒ NEVER rearm into it (safe direction).
        let (_d, paths) = temp_paths();
        write_marker(&paths, &payload("n", "1.0.9", "/x", NOW + 600));
        // Make removal fail: replace the marker with a same-named DIRECTORY.
        std::fs::remove_file(&paths.marker).unwrap();
        std::fs::create_dir(&paths.marker).unwrap();
        let c = Counters::new(true);
        let defuse = post_create_failure_cleanup(
            &paths,
            &|| c.intent.clone(),
            &|| {
                c.arms.set(c.arms.get() + 1);
                Ok(())
            },
            &|| {
                c.disarms.set(c.disarms.get() + 1);
                true
            },
        );
        assert!(defuse);
        assert_eq!(
            c.arms.get(),
            0,
            "no rearm into a window that could not be cleared"
        );
    }

    #[test]
    fn t7_unreadable_intent_keeps_marker_disarmed_and_defuses() {
        // Post-impl audit #2: on the cleanup path, intent may have changed while the lock was
        // released, so an unreadable read must NOT assume ON — it keeps the marker (suppression),
        // performs zero reconciliation, and still defuses the guard.
        let (_d, paths) = temp_paths();
        write_marker(&paths, &payload("n", "1.0.9", "/x", NOW + 600));
        let mut c = Counters::new(true);
        c.intent = Err("io".into());
        let defuse = post_create_failure_cleanup(
            &paths,
            &|| c.intent.clone(),
            &|| {
                c.arms.set(c.arms.get() + 1);
                Ok(())
            },
            &|| {
                c.disarms.set(c.disarms.get() + 1);
                true
            },
        );
        assert!(defuse);
        assert_eq!((c.arms.get(), c.disarms.get()), (0, 0));
        assert!(
            matches!(load(&paths, NOW), LoadedMarker::Valid(_)),
            "unreadable intent must leave the window suppressed, not resolved"
        );
    }

    #[cfg(unix)]
    #[test]
    fn t3_expired_marker_with_unverifiable_path_still_expires() {
        // The dominance rule: expiry beats path verification, or a permanently unreadable path
        // suppresses forever — the TTL-limbo r5 forbids.
        let (d, paths) = temp_paths();
        let locked_dir = d.path().join("locked2");
        std::fs::create_dir(&locked_dir).unwrap();
        let target = locked_dir.join("app.exe");
        std::fs::write(&target, b"x").unwrap();
        write_marker(
            &paths,
            &payload("n", "1.0.9", &target.to_string_lossy(), NOW - 5),
        );
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o000)).unwrap();
        let c = Counters::new(true);
        let out = c.run(&paths, "1.0.9", "/x");
        std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            out,
            ReconcileOutcome::Proceed,
            "expiry must dominate unverifiability"
        );
        assert_eq!((c.arms.get(), c.disarms.get()), (0, 0));
        assert_eq!(load(&paths, NOW), LoadedMarker::Missing);
    }

    #[cfg(unix)]
    #[test]
    fn t3_unverifiable_expected_path_suppresses_not_removes() {
        // Post-impl audit #3: a permission failure on the expected path must NOT read as
        // "absent" — that would widen the rename-tolerance clause into wrong removals.
        let (d, paths) = temp_paths();
        let locked_dir = d.path().join("locked");
        std::fs::create_dir(&locked_dir).unwrap();
        let target = locked_dir.join("app.exe");
        std::fs::write(&target, b"x").unwrap();
        let m = payload("nonce-1", "1.0.9", &target.to_string_lossy(), NOW + 600);
        write_marker(&paths, &m);
        std::fs::write(&paths.token, b"nonce-1").unwrap();
        // Make the parent unsearchable: try_exists on the target now errors.
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o000)).unwrap();

        let c = Counters::new(true);
        let out = c.run(&paths, "1.0.9", "/somewhere/else");
        // Restore perms BEFORE asserting so a failure cannot leak an undeletable tempdir.
        std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            out,
            ReconcileOutcome::Suppressed("expected path unverifiable")
        );
        assert_eq!((c.arms.get(), c.disarms.get()), (0, 0));
        assert!(
            matches!(load(&paths, NOW), LoadedMarker::Valid(_)),
            "marker kept for retry"
        );
    }

    #[test]
    fn t7_intent_off_confirm_disarm_before_remove() {
        let (_d, paths) = temp_paths();
        write_marker(&paths, &payload("n", "1.0.9", "/x", NOW + 600));
        // Disarm unconfirmed ⇒ marker must SURVIVE (removing first could leave
        // intent-OFF/task-armed permanently).
        let c = Counters::new(false);
        let defuse = post_create_failure_cleanup(
            &paths,
            &|| c.intent.clone(),
            &|| {
                c.arms.set(c.arms.get() + 1);
                Ok(())
            },
            &|| {
                c.disarms.set(c.disarms.get() + 1);
                false
            },
        );
        assert!(defuse);
        assert!(
            matches!(load(&paths, NOW), LoadedMarker::Valid(_)),
            "marker kept on unconfirmed disarm"
        );
        // Confirmed disarm ⇒ removed.
        let c2 = Counters::new(false);
        post_create_failure_cleanup(&paths, &|| c2.intent.clone(), &|| Ok(()), &|| true);
        assert_eq!(load(&paths, NOW), LoadedMarker::Missing);
    }

    // ── T4: barrier race (both audits' highest-value ask) ──
    //
    // The shape both audits demanded: an operation observes Missing, the window is PUBLISHED
    // before the operation's authoritative check, and the operation must come back Suppressed
    // with zero mutation callbacks. This drives the PRODUCTION reconcile core across the real
    // decision boundary (the lock itself is the caller's; what the lock protects is exactly the
    // load-before-act sequence pinned here).

    #[test]
    fn t4_barrier_marker_published_after_first_observation() {
        let (_d, paths) = temp_paths();
        // Precondition, asserted (piece-1 rule): no marker exists at first observation.
        assert_eq!(load(&paths, NOW), LoadedMarker::Missing);
        assert!(!live_marker_exists(&paths, NOW));

        // The updater publishes a live window between that observation and the gated mutation's
        // authoritative re-check (in production: between heal step 4b and the under-lock re-check).
        write_marker(&paths, &payload("raced", "1.0.9", "/x", NOW + 600));

        // The authoritative check must now see it…
        assert!(
            live_marker_exists(&paths, NOW),
            "re-check must observe the published window"
        );

        // …and the reconcile transaction must suppress with ZERO reconciliation callbacks — the
        // token belongs to no completed install, so nothing may arm, disarm, or remove.
        let c = Counters::new(true);
        assert_eq!(
            c.run(&paths, "1.0.8", "/elsewhere"),
            ReconcileOutcome::Suppressed("no completion token")
        );
        assert_eq!((c.arms.get(), c.disarms.get()), (0, 0));
        assert!(
            matches!(load(&paths, NOW), LoadedMarker::Valid(_)),
            "the raced window must survive untouched"
        );
    }

    // ── handoff/token plumbing ──

    #[test]
    fn token_roundtrip_and_bounds() {
        let (_d, paths) = temp_paths();
        write_handoff(&paths, "abc-123").unwrap();
        // Simulate the NSIS rename.
        std::fs::rename(&paths.handoff, &paths.token).unwrap();
        assert_eq!(read_token_nonce(&paths).as_deref(), Some("abc-123"));
        // Whitespace tolerated; empty rejected.
        std::fs::write(&paths.token, b"  abc-123\r\n").unwrap();
        assert_eq!(read_token_nonce(&paths).as_deref(), Some("abc-123"));
        std::fs::write(&paths.token, b"   ").unwrap();
        assert_eq!(read_token_nonce(&paths), None);
    }
}
