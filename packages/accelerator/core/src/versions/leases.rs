//! In-use leases for cached `bb` versions — the cross-request guard `FINDINGS.md` B2 deferred.
//!
//! ## What this replaces
//!
//! Cleanup used to decide "is anyone using this version?" from the version directory's mtime: an
//! entry touched within a five-minute window was presumed active. That is a heuristic with two
//! failures. It is too WEAK for a long proof — a proof queued behind the semaphore for longer than
//! the window loses its binary — and it is inherently racy, because cleanup reads the mtime and then
//! unlinks, so a proof that starts between those two steps is evicted anyway.
//!
//! F-06 made that sharper rather than milder: the total-size cap can evict a `Mainnet` version,
//! which the per-tier count policy never could, so entries that were previously immortal became
//! eligible while in use.
//!
//! A lease answers the question directly instead of inferring it. A proof takes one before it
//! resolves its binary and holds it until it is finished; cleanup skips anything held.
//!
//! ## Why in-process is sufficient
//!
//! Exactly one accelerator instance runs at a time — the port guard in `server::bind` enforces it,
//! and a second instance fails to bind rather than racing. Cleanup is likewise always spawned from
//! this process's own prove path. So every party that could evict and every party that could be
//! using a binary live in this address space, and a `Mutex` is the whole synchronisation story. A
//! file-based lease would add crash-recovery questions (stale lock files, PID reuse) to buy nothing.
//!
//! ## Refcounted, not a flag
//!
//! Concurrent proofs of the SAME version are ordinary — two requests from the same dApp — so release
//! has to be "when the last holder finishes", not "when any holder finishes". A bare `HashSet` would
//! let the first `Drop` unprotect a version another proof is still executing.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Version string → number of live leases. Absent means unleased; a count never reaches 0 while
/// present, because [`Lease::drop`] removes the entry at zero.
fn registry() -> &'static Mutex<HashMap<String, usize>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A held lease. The version it names cannot be evicted while this is alive; dropping it releases.
///
/// Deliberately has no public constructor other than [`acquire`], so a lease cannot be forged or
/// released early by anything except going out of scope.
#[derive(Debug)]
pub struct Lease {
    version: String,
}

impl Drop for Lease {
    fn drop(&mut self) {
        // A poisoned lock still has to release, or one panicking proof would pin a version for the
        // life of the process — the failure this whole module exists to avoid, in a subtler form.
        let mut map = registry().lock().unwrap_or_else(|e| e.into_inner());
        match map.get_mut(&self.version) {
            Some(n) if *n > 1 => *n -= 1,
            _ => {
                map.remove(&self.version);
            }
        }
    }
}

/// Take a lease on `version` for as long as the returned guard lives.
///
/// Call this BEFORE resolving the binary's path, not after: the gap between "cleanup decided this
/// was evictable" and "we opened the file" is precisely the window being closed.
pub fn acquire(version: &str) -> Lease {
    let mut map = registry().lock().unwrap_or_else(|e| e.into_inner());
    *map.entry(version.to_string()).or_insert(0) += 1;
    Lease {
        version: version.to_string(),
    }
}

/// Is any proof currently holding `version`? Consulted by eviction.
pub fn is_leased(version: &str) -> bool {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unique per test so the process-global registry cannot make these order-dependent.
    fn v(tag: &str) -> String {
        format!("5.0.0-lease-{tag}")
    }

    #[test]
    fn a_lease_is_visible_while_held_and_gone_after() {
        let name = v("basic");
        assert!(!is_leased(&name));
        {
            let _l = acquire(&name);
            assert!(is_leased(&name), "a held lease must be visible to eviction");
        }
        assert!(!is_leased(&name), "dropping the guard must release");
    }

    /// The reason this is a refcount and not a flag: two concurrent proofs of the same version are
    /// ordinary, and the first one finishing must not unprotect the second.
    #[test]
    fn the_last_holder_releases_not_the_first() {
        let name = v("refcount");
        let a = acquire(&name);
        let b = acquire(&name);
        drop(a);
        assert!(
            is_leased(&name),
            "still held by the second proof — releasing here would evict a binary in use"
        );
        drop(b);
        assert!(!is_leased(&name));
    }

    /// A panicking proof must not pin its version forever — that would be the same class of bug
    /// (something un-evictable for the life of the process) that this module removes.
    #[test]
    fn a_panic_while_holding_still_releases() {
        let name = v("panic");
        let held = name.clone();
        let _ = std::panic::catch_unwind(move || {
            let _l = acquire(&held);
            panic!("proof blew up");
        });
        assert!(!is_leased(&name), "unwinding must run Drop and release");
    }

    #[test]
    fn leases_are_per_version() {
        let a = v("iso-a");
        let b = v("iso-b");
        let _l = acquire(&a);
        assert!(is_leased(&a));
        assert!(
            !is_leased(&b),
            "a lease must not protect an unrelated version"
        );
    }
}
