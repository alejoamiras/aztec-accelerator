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
