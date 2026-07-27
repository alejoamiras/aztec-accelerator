//! Thin GUI-side wrapper over `accelerator_core::server`. Re-exports the core server surface (router,
//! start, AppState, HeadlessState, /health, HTTPS_PORT, bind_with_retry, ServerStatus, callbacks, …) so
//! existing `aztec_accelerator::server::*` paths stay stable, and adds the GUI-local HTTPS adapter
//! `start_https` — which uses `tokio_rustls` (a GUI-only dependency, kept out of the headless core).

pub use accelerator_core::server::*;

mod tls;
pub use tls::start_https;

/// Spawn the GUI-side HTTPS server with `tls_config`, logging any error. Shared by the two callers
/// (launch-time `try_start_https` + settings-time `enable_https`) — only the identical
/// spawn+error-log wrapper is unified; each caller keeps its own (intentionally divergent) TLS-load
/// and failure-handling preamble upstream. (F-09)
///
/// Returns `Some(receiver)` resolving to the BIND outcome (`true` = listener live, `false` = port
/// unavailable), or **`None` when another start attempt is already in flight** — the launch gate and
/// the Settings/onboarding enable path can both decide to start HTTPS at the same moment, and a
/// compare-and-swap on `https_starting` makes exactly ONE of them actually spawn. Without it both
/// spawned, one lost the port to `AddrInUse`, and that loser reported failure while HTTPS was in fact
/// live (post-impl codex Medium). A `None` caller should wait on `https_bound` instead of concluding
/// failure. The enable path awaits the receiver before persisting `https_enabled`; the launch path
/// drops it (a dropped receiver just makes `start_https`'s `send` a no-op).
pub fn spawn_https(
    state: AppState,
    tls_config: std::sync::Arc<tokio_rustls::rustls::ServerConfig>,
) -> Option<tokio::sync::oneshot::Receiver<bool>> {
    use std::sync::atomic::Ordering;
    // Claim the single "starting" slot. `Acquire`/`Release` so the winner's subsequent writes (and the
    // `https_bound` flip inside start_https) are visible to a waiter that observes the flag.
    if state
        .https_starting
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        tracing::debug!("HTTPS start already in flight — not spawning a second listener");
        return None;
    }

    let (tx, rx) = tokio::sync::oneshot::channel();
    let starting = state.https_starting.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start_https(state, tls_config, tx).await {
            // start_https only returns Err before it can send `ready`; the dropped `tx` makes the
            // awaiting receiver observe a bind failure (RecvError), which the caller treats as such.
            tracing::error!("HTTPS server error: {e}");
        }
        // Reached ONLY when the listener is not running (bind failure or a fatal error) — the serving
        // loop never returns. Release the slot so a later enable attempt can retry.
        starting.store(false, Ordering::Release);
    });
    Some(rx)
}
