# Research — version-model (versions.rs + bb.rs) · Q3, Q11 + minors

## Public surface / call sites (AztecVersion changes these)
- `pub` fns: `is_valid_version`, `version_bb_path`, `download_bb`, `NetworkTier::from_version`, `versions_to_evict`, `list_cached_versions`, `cleanup_old_versions` (versions.rs); `find_bb` (bb.rs).
- External call sites (only ~5): server.rs:418 `is_valid_version`, :434 `version_bb_path`, :440 `download_bb`, :446 `cleanup_old_versions`, :242 `find_bb`; main.rs:385 `find_bb`; bb.rs:29 internal `version_bb_path`.

## Invariants (must preserve)
- `is_valid_version` rejects traversal (empty/`..`/lead+trail dot/>128/non-ASCII) — hardened #99; AztecVersion ctor MUST enforce identically BEFORE any path/net.
- Tier retention: nightly 2 / devnet 3 / testnet 5 / mainnet ∞; **bundled never evicted**.
- download_bb order: cache→GET(32→64MB cap)→**digest verify→extract**→atomic rename→chmod→macOS codesign. Fail-closed digest. Lowercase hex (GitHub API compare).

## Tests pinning behavior (versions.rs 24 + bb.rs 8)
Strong: is_valid_version accept/reject, download_bb sink-guard, tier_classification, retention_limits, evict_excess_nightlies, bundled_version_never_evicted, mainnet_never_evicted, rc_versions_sort_numerically, mixed_tiers, extract_* (synthetic/nested/symlink/corrupt/empty/cleanup), sha256_hex, find_bb env/priority. download_and_verify_bb (gated ACCELERATOR_DOWNLOAD_TEST).
**GAPS to add first:** download_bb atomic-rename-cleanup (tmp not stranded on extract failure); versions_to_evict empty-list / all-bundled; find_bb search-chain order (env→cache→sidecar→bbup→PATH stop-at-first); from_version edge cases.

## Safe seams
- **Q11 download_bb → 4 helpers**: `download_tarball(version)->Vec<u8>` (L284-321), `verify_digest(version,&bytes)` (L328-352), `install_version_dir(&bytes,version)->PathBuf` (L354-370, folds the 2 remove_dir_all arms), `postprocess_unix`/`postprocess_macos` (L373-417). Orchestrator keeps guard+cache fast-path. Ordering preserved.
- **Q3 AztecVersion**: `{ raw:String, tier:NetworkTier, sort_key:(String,u64) }` computed once in `new()` (validation = the gate). `Deref<Target=str>` + `AsRef<str>` for call-site ergonomics. `&str` boundary stays at HTTP ingress (server.rs resolve_version constructs it). Subsumes versions_to_evict re-parse (compute bundled_tier once) + `bb_asset_name()` helper (versions.rs:121,331 dup).

## Behavior-change risks
- AztecVersion ctor validates earlier (SAFER; test ctor rejects == old is_valid_version inputs).
- versions_to_evict re-parse removal = identical output (perf only).
- dirs_next/home_dir_fallback wrappers (bb.rs) — inline is fine, low value; note inconsistency w/ versions.rs direct dirs::home_dir.
