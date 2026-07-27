//! Thin GUI-side wrapper over `accelerator_core::server`. Re-exports the core server surface (router,
//! start, AppState, HeadlessState, /health, HTTPS_PORT, bind_with_retry, ServerStatus, callbacks, …) so
//! existing `aztec_accelerator::server::*` paths stay stable, and adds the GUI-local HTTPS adapter
//! `start_https` — which uses `tokio_rustls` (a GUI-only dependency, kept out of the headless core).

pub use accelerator_core::server::*;

mod tls;
pub use tls::start_https;

/// Exclusive claim on the HTTPS lifecycle — held across the WHOLE rotate → load-TLS → bind sequence,
/// not just the bind (post-impl codex). Claiming only at spawn time left a window where the launch
/// gate was mid-rotation while the enable path loaded the pre-rotation (or, between `swap_into`'s
/// sequential renames, a MIXED leaf/key) set and won the bind — serving a stale leaf for the whole
/// session, or failing to load and resetting `https_enabled`.
///
/// Releases on `Drop`, so every early return in the claim-holding caller (a failed rotation, a failed
/// TLS load) frees the slot automatically — a missed manual release would wedge HTTPS for the rest of
/// the process. Ownership moves into [`spawn_https`]'s task, which drops it only when the listener
/// exits (a successful serving loop never returns, so a live listener keeps the claim — harmless,
/// since `https_bound` short-circuits every later attempt before it tries to claim).
pub struct HttpsStartClaim {
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for HttpsStartClaim {
    fn drop(&mut self) {
        self.flag.store(false, std::sync::atomic::Ordering::Release);
        tracing::debug!("HTTPS lifecycle claim released");
    }
}

/// Try to take the exclusive HTTPS-lifecycle claim. `None` ⇒ another task is already starting HTTPS;
/// that caller must wait on `https_bound` rather than concluding failure. Call this BEFORE rotating or
/// loading the cert set, so the whole sequence is single-owner.
pub fn try_claim_https_start(state: &AppState) -> Option<HttpsStartClaim> {
    use std::sync::atomic::Ordering;
    // `Acquire`/`Release` so the winner's subsequent writes (and the `https_bound` flip inside
    // start_https) are visible to a waiter that observes the flag.
    if state
        .https_starting
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        tracing::debug!("HTTPS start already in flight — not claiming");
        return None;
    }
    Some(HttpsStartClaim {
        flag: state.https_starting.clone(),
    })
}

/// Spawn the GUI-side HTTPS server with `tls_config`, logging any error. Shared by the two callers
/// (launch-time `try_start_https` + settings-time `enable_https`) — only the identical
/// spawn+error-log wrapper is unified; each caller keeps its own (intentionally divergent) TLS-load
/// and failure-handling preamble upstream. (F-09)
///
/// Requires the caller's [`HttpsStartClaim`] (proof it owns the lifecycle) and moves it into the
/// spawned task. Returns a receiver resolving to the BIND outcome (`true` = listener live, `false` =
/// port unavailable). The enable path awaits it before persisting `https_enabled`; the launch path
/// drops it (a dropped receiver just makes `start_https`'s `send` a no-op — it does not cancel).
pub fn spawn_https(
    state: AppState,
    tls_config: std::sync::Arc<tokio_rustls::rustls::ServerConfig>,
    claim: HttpsStartClaim,
) -> tokio::sync::oneshot::Receiver<bool> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start_https(state, tls_config, tx).await {
            // start_https only returns Err before it can send `ready`; the dropped `tx` makes the
            // awaiting receiver observe a bind failure (RecvError), which the caller treats as such.
            tracing::error!("HTTPS server error: {e}");
        }
        // Reached ONLY when the listener is not running (bind failure or a fatal error) — the serving
        // loop never returns. Dropping the claim here frees the slot for a later retry.
        drop(claim);
    });
    rx
}
