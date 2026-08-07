//! In-use leases for cached `bb` versions — the cross-request guard `FINDINGS.md` B2 deferred.
//!
//! ## What this replaces
//!
//! Cleanup used to decide "is anyone using this version?" from the version directory's mtime: an
//! entry touched within a five-minute window was presumed active. That is a heuristic with two
//! failures. It is too WEAK for a long proof — one queued behind the prove permit for longer than the
//! window loses its binary — and it is inherently racy, because cleanup reads the mtime and then
//! unlinks, so a proof starting between those two steps is evicted anyway.
//!
//! F-06 made that sharper rather than milder: the total-size cap can evict a `Mainnet` version, which
//! the per-tier count policy never could, so entries that were previously immortal became eligible
//! while in use.
//!
//! ## Why a predicate is not enough
//!
//! The first version of this module exposed `is_leased()` and had eviction call it before deleting.
//! That is the SAME check-then-act shape as the mtime window, just with a better predicate: the lock
//! is released between the check and the `remove_dir_all`, so a proof that acquires in that gap still
//! loses its binary. (Caught by a codex review of the first attempt.)
//!
//! So eviction does not ask a question — it takes a RESERVATION. [`begin_evict`] and [`acquire`]
//! contend for the same map entry under one mutex, and whichever arrives first excludes the other for
//! as long as its guard lives. A version cannot be acquired while it is being deleted, and cannot be
//! deleted while it is held.
//!
//! ## Why in-process is sufficient
//!
//! Exactly one accelerator instance serves at a time — the port bind in `server::start` decides which,
//! and eviction is only ever started AFTER that bind succeeds (a losing instance exits without ever
//! sweeping; this ordering is load-bearing and noted at the call site). Cleanup is otherwise spawned
//! from this process's own prove path. So every party that could evict and every party that could be
//! executing a binary live in this address space, and a `Mutex` is the whole synchronisation story. A
//! file-based lease would add stale-lock and PID-reuse questions to buy nothing.
//!
//! ## Refcounted, not a flag
//!
//! Concurrent proofs of the SAME version are ordinary — two requests from one dApp — so release has to
//! mean "the last holder finished", not "a holder finished". A bare set would let the first `Drop`
//! unprotect a version another proof is still executing.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// What is currently true of a version, for the one map both sides contend on.
#[derive(Debug)]
enum State {
    /// Held by `n` proofs. Never stored as 0 — the entry is removed at the last release.
    Held(usize),
    /// A cleanup pass has reserved this for deletion. Acquisition is refused until it finishes.
    Evicting,
}

fn registry() -> &'static Mutex<HashMap<String, State>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, State>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A poisoned lock must not stop either side from making progress: a proof that panicked while
/// holding still has to release, and a cleanup that panicked still has to un-reserve. Otherwise one
/// panic pins or bricks a version for the life of the process — the failure this module exists to
/// prevent, wearing a different hat.
fn lock() -> std::sync::MutexGuard<'static, HashMap<String, State>> {
    registry().lock().unwrap_or_else(|e| e.into_inner())
}

/// A held lease. The version it names cannot be evicted while this is alive.
#[derive(Debug)]
pub struct Lease {
    version: String,
}

impl Drop for Lease {
    fn drop(&mut self) {
        let mut map = lock();
        match map.get_mut(&self.version) {
            Some(State::Held(n)) if *n > 1 => *n -= 1,
            // Last holder, or an entry that is somehow not `Held` — either way stop claiming it.
            _ => {
                map.remove(&self.version);
            }
        }
    }
}

/// A reservation to delete. While alive, [`acquire`] refuses this version.
#[derive(Debug)]
pub struct EvictReservation {
    version: String,
}

impl Drop for EvictReservation {
    fn drop(&mut self) {
        let mut map = lock();
        // Only clear OUR reservation. If a `Held` entry is somehow present, a proof owns it now and
        // removing it would silently unprotect that proof.
        if matches!(map.get(&self.version), Some(State::Evicting)) {
            map.remove(&self.version);
        }
    }
}

/// Take a lease on `version`, or `None` if a cleanup pass is deleting it right now.
///
/// `None` means "this binary is going away" — callers treat it as a cache miss and re-download rather
/// than racing the deletion.
///
/// Call this BEFORE resolving the path AND before any wait that precedes execution (the prove permit,
/// a download): the window being closed runs from "cleanup could decide this is evictable" all the way
/// to "we finished executing it".
pub fn acquire(version: &str) -> Option<Lease> {
    let mut map = lock();
    match map.get_mut(version) {
        Some(State::Evicting) => None,
        Some(State::Held(n)) => {
            *n += 1;
            Some(Lease {
                version: version.to_string(),
            })
        }
        None => {
            map.insert(version.to_string(), State::Held(1));
            Some(Lease {
                version: version.to_string(),
            })
        }
    }
}

/// Reserve `version` for deletion, or `None` if a proof holds it (or another pass already reserved).
///
/// The returned guard must be held across the actual `remove_dir_all`. That is the whole point: from
/// an acquirer's perspective the reservation and the deletion are one critical section, so there is no
/// gap for a proof to slip into.
pub fn begin_evict(version: &str) -> Option<EvictReservation> {
    let mut map = lock();
    match map.get(version) {
        // Held by a proof, or already reserved by another pass — either way, not ours to delete.
        Some(_) => None,
        None => {
            map.insert(version.to_string(), State::Evicting);
            Some(EvictReservation {
                version: version.to_string(),
            })
        }
    }
}

/// Is any proof currently holding `version`? Diagnostics only — eviction must use [`begin_evict`],
/// because a bare query cannot be atomic with the deletion that follows it.
#[cfg(test)]
pub fn is_leased(version: &str) -> bool {
    matches!(lock().get(version), Some(State::Held(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unique per test: the registry is process-global, so shared names would make these
    /// order-dependent.
    fn v(tag: &str) -> String {
        format!("5.0.0-lease-{tag}")
    }

    #[test]
    fn a_lease_is_visible_while_held_and_gone_after() {
        let name = v("basic");
        assert!(!is_leased(&name));
        {
            let _l = acquire(&name).expect("nothing is evicting it");
            assert!(is_leased(&name));
        }
        assert!(!is_leased(&name), "dropping the guard must release");
    }

    /// Why this is a refcount and not a flag: two concurrent proofs of one version are ordinary, and
    /// the first to finish must not unprotect the second.
    #[test]
    fn the_last_holder_releases_not_the_first() {
        let name = v("refcount");
        let a = acquire(&name).unwrap();
        let b = acquire(&name).unwrap();
        drop(a);
        assert!(
            is_leased(&name),
            "still held — releasing here would expose a binary in use"
        );
        drop(b);
        assert!(!is_leased(&name));
    }

    /// THE property the predicate version could not provide: a reservation and its deletion are one
    /// critical section, so a proof cannot slip in between "cleanup decided" and "cleanup deleted".
    #[test]
    fn a_reservation_excludes_acquisition_until_it_is_released() {
        let name = v("reserve");
        let res = begin_evict(&name).expect("unheld, so reservable");
        assert!(
            acquire(&name).is_none(),
            "acquiring mid-deletion is exactly the race this closes"
        );
        drop(res);
        assert!(
            acquire(&name).is_some(),
            "once the deletion is done the version is acquirable again"
        );
    }

    /// The other direction: a held version cannot be reserved, so cleanup skips it.
    #[test]
    fn a_held_version_cannot_be_reserved_for_deletion() {
        let name = v("held-vs-evict");
        let _l = acquire(&name).unwrap();
        assert!(begin_evict(&name).is_none());
    }

    /// Two cleanup passes must not both delete the same directory.
    #[test]
    fn two_passes_cannot_both_reserve() {
        let name = v("double-evict");
        let _first = begin_evict(&name).expect("first pass wins");
        assert!(begin_evict(&name).is_none(), "second pass must back off");
    }

    /// A panicking proof must not pin its version for the life of the process.
    #[test]
    fn a_panic_while_holding_still_releases() {
        let name = v("panic");
        let held = name.clone();
        let _ = std::panic::catch_unwind(move || {
            let _l = acquire(&held).unwrap();
            panic!("proof blew up");
        });
        assert!(!is_leased(&name), "unwinding must run Drop and release");
    }

    /// And a panicking CLEANUP must not leave a version permanently unacquirable — the same failure
    /// from the other side.
    #[test]
    fn a_panic_while_reserving_still_un_reserves() {
        let name = v("panic-evict");
        let held = name.clone();
        let _ = std::panic::catch_unwind(move || {
            let _r = begin_evict(&held).unwrap();
            panic!("cleanup blew up");
        });
        assert!(
            acquire(&name).is_some(),
            "a crashed cleanup must not brick the version"
        );
    }

    #[test]
    fn leases_are_per_version() {
        let a = v("iso-a");
        let b = v("iso-b");
        let _l = acquire(&a).unwrap();
        assert!(is_leased(&a));
        assert!(!is_leased(&b));
        assert!(
            begin_evict(&b).is_some(),
            "unrelated versions stay evictable"
        );
    }
}
