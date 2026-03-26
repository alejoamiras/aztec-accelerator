//! Headless accelerator server — no Tauri, no GUI.
//!
//! Runs the same Axum HTTP server as the Tauri app but without any display
//! context. Used in CI for e2e testing against the native `bb` binary.
//!
//! Set `ALLOWED_ORIGINS=origin1,origin2` to restrict which origins can call `/prove`.
//! When unset, all origins are auto-approved (no auth_manager).

use aztec_accelerator::authorization::AuthorizationManager;
use aztec_accelerator::config::AcceleratorConfig;
use aztec_accelerator::server::{start, AppState};
use parking_lot::RwLock;
use std::sync::Arc;
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

    // If ALLOWED_ORIGINS is set, enforce origin gating with those origins pre-approved.
    // Without it, auth_manager is None and all origins are auto-approved.
    let (auth_manager, config) = if let Ok(origins_str) = std::env::var("ALLOWED_ORIGINS") {
        let origins: Vec<String> = origins_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        tracing::info!(origins = ?origins, "Restricting to allowed origins");
        let cfg = AcceleratorConfig {
            approved_origins: origins,
            ..Default::default()
        };
        (
            Some(Arc::new(AuthorizationManager::new())),
            Some(Arc::new(RwLock::new(cfg))),
        )
    } else {
        (None, None)
    };

    let state = AppState {
        auth_manager,
        config,
        prove_semaphore: Some(Arc::new(tokio::sync::Semaphore::new(1))),
        ..Default::default()
    };

    if let Err(e) = start(state).await {
        tracing::error!("Accelerator server error: {e}");
        std::process::exit(1);
    }
}
