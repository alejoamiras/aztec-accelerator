//! Cross-platform browser-trust-store management for the local CA anchor.
//!
//! `certs.rs` owns cert generation, paths, and the rustls config; this module owns ONLY the OS trust
//! mechanism. It never generates or reads a private key — every entry point takes an already-written
//! CA *cert* path that `certs.rs` derives.
//!
//! Backends:
//! - **macOS** ([`macos`]): the login Keychain via the absolute-path `security` CLI.
//! - **Linux** ([`linux`]): user-level NSS databases (`~/.pki/nssdb` + each Firefox profile) via
//!   `certutil` — NO root, honest per-store reporting when `certutil` is absent.
//! - **Windows** (Phase 4): CurrentUser Root store; currently a [`stub`] that reports not-installed.
//!
//! Security posture (see plan §8): all shell-outs use `Command` + fixed argv (no shell string);
//! external binaries are resolved to safe absolute paths (a planted `certutil` in a writable PATH
//! dir is rejected). Name constraints on the anchor are defense-in-depth; the load-bearing control
//! is the keyless CA (it can sign nothing), so a trusted anchor in any store is harmless.

use std::path::Path;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as imp;

// Any other target (none shipped) falls back to the not-supported stub.
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod stub;
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
use stub as imp;

/// One trust store's install/query result. Surfaced honestly in the UI — a store we could not write
/// (e.g. `certutil` missing, a sandboxed browser we can't reach) is *reported*, not hidden.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StoreStatus {
    /// Human-facing store name (e.g. "macOS Keychain", "System NSS", "Firefox (default)").
    pub store: String,
    /// Whether the anchor is installed / trusted in this store.
    pub installed: bool,
    /// Optional explanation when not installed (e.g. "certutil not found — install libnss3-tools").
    pub detail: Option<String>,
}

impl StoreStatus {
    pub(crate) fn ok(store: impl Into<String>) -> Self {
        Self {
            store: store.into(),
            installed: true,
            detail: None,
        }
    }
    pub(crate) fn fail(store: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            store: store.into(),
            installed: false,
            detail: Some(detail.into()),
        }
    }
}

/// The per-store outcome of an install / removal / status query across all discoverable stores.
#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct TrustReport {
    pub stores: Vec<StoreStatus>,
}

impl TrustReport {
    /// True iff the anchor is installed in at least one store — the "HTTPS is usable in some browser"
    /// signal the wizard keys on (a partially-trusted loopback listener is harmless; see plan R3).
    pub fn any_installed(&self) -> bool {
        self.stores.iter().any(|s| s.installed)
    }
}

/// Opaque reference to a previously-installed anchor, captured BEFORE a rotation swap so the OLD
/// anchor can be removed AFTER the NEW one is trusted (D4). Per-OS payload: macOS SHA-1 / Linux
/// nickname. `None` = nothing to remove.
pub struct AnchorRef(pub(crate) Option<String>);

// ── Public dispatch API (certs.rs + commands.rs) ──

/// Install `ca_cert` as a trusted root in every discoverable browser store. Best-effort per store;
/// the report says which succeeded (Linux/wizard treat ≥1 installed as success).
pub fn install_ca_trust(ca_cert: &Path) -> TrustReport {
    imp::install(ca_cert)
}

/// Remove the anchor from every store (Settings "Remove certificate trust" + the uninstall CLI).
pub fn remove_ca_trust(ca_cert: &Path) -> TrustReport {
    imp::remove(ca_cert)
}

/// Per-store trust status for `ca_cert` (drives the honest Settings/wizard status list).
pub fn trust_status(ca_cert: &Path) -> TrustReport {
    imp::status(ca_cert)
}

/// Whether the anchor is trusted in at least one store (launch gate on macOS/Windows; UI on Linux).
pub fn is_ca_trusted(ca_cert: &Path) -> bool {
    imp::status(ca_cert).any_installed()
}

// ── Rotation hooks (certs::rotate) ──

/// Capture the currently-installed anchor's identity (from `live_ca`) for post-swap removal.
pub fn current_anchor(live_ca: &Path) -> AnchorRef {
    imp::current_anchor(live_ca)
}

/// Trust + verify a freshly-staged anchor BEFORE it replaces the live one. `Err` ⇒ the caller keeps
/// the old set (fail-closed — no outage, never an untrusted cert served).
pub fn trust_new_anchor(staged_ca: &Path) -> Result<(), String> {
    imp::trust_new_anchor(staged_ca)
}

/// Remove a previously-captured old anchor after the swap (best-effort).
pub fn remove_anchor(old: AnchorRef) {
    imp::remove_anchor(old)
}
