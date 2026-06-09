//! HTTPS listener for optional Safari support.
//!
//! Runs independently from the HTTP server — errors are logged but never crash the app (HTTPS is
//! optional). Extracted from server.rs (Q2).

use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;

use accelerator_core::server::{bind_with_retry, router_for_port, AppState, HTTPS_PORT};

/// Start an HTTPS listener using the provided TLS config.
/// Runs independently from HTTP — errors are logged but never crash the app.
pub async fn start_https(
    state: AppState,
    tls_config: Arc<tokio_rustls::rustls::ServerConfig>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Capture the shared bind-state flag before `router_for_port` consumes `state`.
    let https_bound = state.https_bound.clone();
    // Pass HTTPS_PORT so the loopback-Host guard accepts `127.0.0.1:59834` (Safari) and rejects a
    // `:59833` authority replayed onto the HTTPS listener.
    let app = router_for_port(state, HTTPS_PORT);
    let addr = SocketAddr::from(([127, 0, 0, 1], HTTPS_PORT));
    // Same restart race as the HTTP listener: an in-place update relaunches while
    // the old process still holds 59834. Retry first; only fall back to HTTP-only
    // if the port is genuinely unavailable past the budget (HTTPS must never
    // crash the app — it's optional Safari support).
    let listener = match bind_with_retry(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("HTTPS port {HTTPS_PORT} unavailable: {e} — continuing HTTP-only");
            return Ok(());
        }
    };
    // The listener bound — mark HTTPS live so /health advertises https_port (Q7).
    https_bound.store(true, Ordering::Relaxed);

    let acceptor = TlsAcceptor::from(tls_config);
    tracing::info!("HTTPS server listening on {addr}");

    loop {
        let (stream, _peer) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                tracing::debug!("TCP accept error: {e}");
                continue;
            }
        };

        let acceptor = acceptor.clone();
        let app = app.clone();

        tokio::spawn(async move {
            let tls_stream = match acceptor.accept(stream).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!("TLS handshake failed: {e}");
                    return;
                }
            };
            let io = hyper_util::rt::TokioIo::new(tls_stream);
            let hyper_service = hyper_util::service::TowerToHyperService::new(app);
            if let Err(e) =
                hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection(io, hyper_service)
                    .await
            {
                tracing::debug!("HTTPS connection error: {e}");
            }
        });
    }
}
