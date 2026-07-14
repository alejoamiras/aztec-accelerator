//! GUI-agnostic core of the Aztec Accelerator: the HTTP proving server, bb binary cache, Aztec
//! version resolution, and origin authorization — everything the headless `accelerator-server`
//! needs, with zero Tauri / GUI / TLS-serving coupling. Both the desktop crate (`aztec-accelerator`)
//! and the headless `accelerator-server` depend on this.
//!
//! Intentionally `build.rs`-free: the `AZTEC_VERSION` read, `verified-sites.json` check, and
//! `tauri_build::build()` all stay in src-tauri/build.rs; the bb-version is injected via
//! `server::HeadlessState.bundled_version`. See implementations-plan/core-extraction-2026-06-07.

pub mod authorization;
pub mod bb;
pub mod config;
pub mod server;
pub mod update_manifest;
pub mod updater_state;
pub mod versions;

use std::path::PathBuf;

/// Returns the log directory.
///
/// - macOS: `~/Library/Application Support/aztec-accelerator/logs/`
/// - Linux: `~/.local/share/aztec-accelerator/logs/`
pub fn log_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("aztec-accelerator")
        .join("logs")
}
