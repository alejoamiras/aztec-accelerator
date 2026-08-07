//! Aztec bb version management: validation/policy, cache layout, release metadata, and the
//! download/install pipeline.
//!
//! q7e3-F-07: previously a single ~1000-LOC module; now focused submodules. This root re-exports the
//! public surface unchanged, so `versions::X` paths outside the module are untouched (the F-12
//! lesson: same module-tree position, zero external churn).

mod cache_layout;
mod downloader;
mod leases;
mod release_metadata;
mod version_policy;

pub use cache_layout::{
    bb_binary_name, list_cached_versions, verify_cached_bb, version_bb_path, versions_base_dir,
};
pub use downloader::download_bb;
pub use leases::{acquire as acquire_lease, Lease};
pub use release_metadata::{current_platform, download_url};
pub use version_policy::{
    check_version_selectable, cleanup_old_versions, is_valid_version, sweep_cache_on_start,
    versions_to_evict, versions_to_evict_for_size, AztecVersion, NetworkTier, VersionRejection,
    CACHE_MAX_TOTAL_BYTES,
};
