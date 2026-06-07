//! Desktop (Tauri) crate. The GUI-agnostic proving core lives in `accelerator-core`; this crate adds
//! the Tauri layer (tray, updater, windows, commands) + the Safari HTTPS / cert surface.
//!
//! `authorization`/`bb`/`config`/`versions`/`log_dir` are re-exported from core so existing
//! `aztec_accelerator::…` imports stay stable; `server` is a thin wrapper that re-exports
//! `accelerator_core::server` and adds the GUI-local `start_https`.

pub use accelerator_core::{authorization, bb, config, log_dir, versions};

pub mod certs;
pub mod commands;
pub mod crash_recovery;
pub mod server;
pub mod updater;
pub mod verified_sites;
