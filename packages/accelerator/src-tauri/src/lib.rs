pub mod authorization;
pub mod bb;
pub mod certs;
pub mod commands;
pub mod config;
pub mod crash_recovery;
pub mod server;
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
