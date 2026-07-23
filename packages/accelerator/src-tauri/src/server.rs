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
pub fn spawn_https(
    state: AppState,
    tls_config: std::sync::Arc<tokio_rustls::rustls::ServerConfig>,
) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = start_https(state, tls_config).await {
            tracing::error!("HTTPS server error: {e}");
        }
    });
}
