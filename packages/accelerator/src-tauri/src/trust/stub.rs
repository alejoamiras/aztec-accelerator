//! Fallback trust backend for platforms without a real implementation yet (Windows lands in Phase 4).
//! Reports "not installed / unsupported" and refuses installs, preserving the pre-refactor behavior
//! where non-macOS trust was a stub error.

use super::{AnchorRef, StoreStatus, TrustReport};
use std::path::Path;

const STORE: &str = "system trust store";
const MSG: &str = "Encrypted connection is not yet available on this platform";

pub fn install(_ca_cert: &Path) -> TrustReport {
    TrustReport {
        stores: vec![StoreStatus::fail(STORE, MSG)],
    }
}

pub fn status(_ca_cert: &Path) -> TrustReport {
    TrustReport {
        stores: vec![StoreStatus {
            store: STORE.into(),
            installed: false,
            detail: None,
        }],
    }
}

pub fn remove(_ca_cert: &Path) -> TrustReport {
    TrustReport {
        stores: vec![StoreStatus {
            store: STORE.into(),
            installed: false,
            detail: None,
        }],
    }
}

pub fn current_anchor(_live_ca: &Path) -> AnchorRef {
    AnchorRef(None)
}

pub fn trust_new_anchor(_staged_ca: &Path) -> Result<(), String> {
    Err(MSG.into())
}

pub fn remove_anchor(_old: AnchorRef) {}
