//! Thin GUI-side wrapper over `accelerator_core::server`. Re-exports the core server surface (router,
//! start, AppState, HeadlessState, /health, HTTPS_PORT, bind_with_retry, ServerStatus, callbacks, …) so
//! existing `aztec_accelerator::server::*` paths stay stable, and adds the GUI-local HTTPS adapter
//! `start_https` — which uses `tokio_rustls` (a GUI-only dependency, kept out of the headless core).

pub use accelerator_core::server::*;

mod tls;
pub use tls::start_https;

/// Exclusive ownership of the HTTPS bring-up, held across the WHOLE sequence: cert
/// inspect/generate/trust → rotate → load TLS → bind. Anything narrower has raced (post-impl codex):
/// claiming only at spawn time let the other path load a mid-`swap_into` MIXED leaf/key set, and
/// claiming only after the cert checks let both paths WRITE cert sets concurrently and corrupt the
/// live one.
///
/// It is a `tokio::sync::Mutex` guard, not an atomic flag, because a waiter must block for exactly as
/// long as the owner takes — the owner may raise an OS trust dialog (unbounded: the user is typing a
/// password) or shell out to `certutil` per NSS store, so any fixed polling budget would either give
/// up early (reporting a spurious failure while the owner is still working) or stall needlessly.
///
/// Released as soon as the bind RESOLVES — the serving loop then runs unlocked, so a later enable can
/// always take the lock. Dropping it on any early return is automatic.
pub type HttpsLifecycleGuard = tokio::sync::OwnedMutexGuard<()>;

/// Take the HTTPS-lifecycle lock, waiting as long as the current owner needs. Async — the
/// Settings/onboarding path uses this. Call it BEFORE touching the cert set.
pub async fn claim_https_lifecycle(state: &AppState) -> HttpsLifecycleGuard {
    state.https_lifecycle.clone().lock_owned().await
}

/// Non-blocking variant for the synchronous launch gate (it runs on a blocking task and must never
/// stall). `None` ⇒ the enable path already owns the bring-up, so launch simply stands down: that
/// path will finish the job, including the bind.
pub fn try_claim_https_lifecycle(state: &AppState) -> Option<HttpsLifecycleGuard> {
    state.https_lifecycle.clone().try_lock_owned().ok()
}

/// Spawn the GUI-side HTTPS server with `tls_config`, logging any error. Shared by the two callers
/// (launch-time `try_start_https` + settings-time `enable_https`) — only the identical
/// spawn+error-log wrapper is unified; each caller keeps its own (intentionally divergent) TLS-load
/// and failure-handling preamble upstream. (F-09)
///
/// The CALLER must hold a [`HttpsLifecycleGuard`] and keep holding it until it has awaited the
/// returned receiver AND committed any resulting state (`https_enabled = true`). Releasing at the bind
/// instead would let the Settings "Remove certificate trust" action slip in between the bind and that
/// config write — removing the anchor and saving `false`, only to be overwritten by the enable's
/// `true`, leaving "enabled" HTTPS with an untrusted CA (post-impl codex Medium).
///
/// Returns a receiver resolving to the bind outcome (`true` = listener live, `false` = port
/// unavailable). A dropped receiver just makes the `send` a no-op — it does not cancel the listener.
pub fn spawn_https(
    state: AppState,
    tls_config: std::sync::Arc<tokio_rustls::rustls::ServerConfig>,
) -> tokio::sync::oneshot::Receiver<bool> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start_https(state, tls_config, tx).await {
            // start_https only returns Err before it can send `ready`; the dropped `tx` makes the
            // awaiting receiver observe a bind failure (RecvError), which the caller treats as such.
            tracing::error!("HTTPS server error: {e}");
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// THE invariant eight rounds of race fixes rest on: the HTTPS bring-up is single-owner. Every
    /// path that touches cert state (launch gate, Settings/onboarding enable, renewal, trust removal)
    /// takes this lock, so if it ever stopped excluding, all of those could interleave — concurrently
    /// writing cert sets, loading a half-renamed one, or committing config over each other.
    #[tokio::test]
    async fn concurrent_claims_are_mutually_exclusive() {
        let state = AppState::default();
        let inside = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let state = state.clone();
            let inside = inside.clone();
            let max_concurrent = max_concurrent.clone();
            handles.push(tokio::spawn(async move {
                let _guard = claim_https_lifecycle(&state).await;
                let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                max_concurrent.fetch_max(now, Ordering::SeqCst);
                // Hold it long enough that a broken lock would overlap observably.
                tokio::time::sleep(Duration::from_millis(5)).await;
                inside.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for h in handles {
            h.await.expect("task");
        }

        assert_eq!(
            max_concurrent.load(Ordering::SeqCst),
            1,
            "at most ONE path may own the HTTPS bring-up at a time"
        );
        assert_eq!(inside.load(Ordering::SeqCst), 0, "all guards released");
    }

    /// The launch gate uses the non-blocking variant and must NOT barge in while another path owns the
    /// bring-up — and must be able to claim once that path is done.
    #[tokio::test]
    async fn try_claim_fails_while_held_and_succeeds_after_release() {
        let state = AppState::default();

        let guard = claim_https_lifecycle(&state).await;
        assert!(
            try_claim_https_lifecycle(&state).is_none(),
            "try_claim must decline while another path holds the bring-up"
        );

        drop(guard);
        assert!(
            try_claim_https_lifecycle(&state).is_some(),
            "try_claim must succeed once the holder released"
        );
    }

    /// Guards release on EVERY exit path — including the `?`/early-return cases scattered through
    /// `enable_https_inner` and `prepare_launch_https`. A leaked guard would wedge HTTPS for the rest
    /// of the process (no further enable could ever claim), so this pins the RAII behavior.
    #[tokio::test]
    async fn guard_releases_on_early_return_and_on_error() {
        let state = AppState::default();

        async fn early_return(state: &AppState) -> Result<(), String> {
            let _guard = claim_https_lifecycle(state).await;
            Err("simulates a failed TLS load / trust install".to_string())?;
            unreachable!()
        }

        assert!(early_return(&state).await.is_err());
        assert!(
            try_claim_https_lifecycle(&state).is_some(),
            "an early return must not leak the bring-up lock"
        );
    }

    /// A panic while holding the guard must still release it (unwind runs `Drop`), otherwise one
    /// unexpected panic in the cert path would permanently disable HTTPS enable/renewal/removal.
    #[tokio::test]
    async fn guard_releases_on_panic() {
        let state = AppState::default();

        let panicking = {
            let state = state.clone();
            tokio::spawn(async move {
                let _guard = claim_https_lifecycle(&state).await;
                panic!("boom");
            })
        };
        assert!(panicking.await.is_err(), "task should have panicked");

        assert!(
            try_claim_https_lifecycle(&state).is_some(),
            "a panic while holding must not leak the bring-up lock"
        );
    }
}
