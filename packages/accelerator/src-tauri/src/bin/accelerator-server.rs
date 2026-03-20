//! Headless accelerator server — no Tauri, no GUI.
//!
//! Runs the same Axum HTTP server as the Tauri app but without any display
//! context. Used in CI for e2e testing against the native `bb` binary.

use aztec_accelerator::server::{start, AppState};
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stdout))
        .init();

    tracing::info!("Starting headless accelerator server");

    let state = AppState::default();

    if let Err(e) = start(state).await {
        tracing::error!("Accelerator server error: {e}");
        std::process::exit(1);
    }
}
