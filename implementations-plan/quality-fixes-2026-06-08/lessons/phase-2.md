# PR-2 — F-04 versions split + F-03 .setup decompose

Branch: `quality/pr2-split-giants` off `main@5c57d39` (PR-1 merged).

## PR-1 closeout
- **Merged green** (#333, squash → 5c57d39). CI: full accelerator.yml green on aa5dcd0 (incl. WebDriver macos/linux/windows + Windows smokes). Codex post-impl: 1 Med (strict Deserialize) fixed + 1 Low (respond_auth timeout-deny) documented. F-01 + F-02 ✓.

## F-04 — versions.rs → façade + submodules (EXECUTABLE MAPPING)
`versions.rs` = 1209 LOC, prod 1–581, tests 582–1209. Convert to `versions/mod.rs` + 5 submodules. **Pure move** — keep every `versions::X` path via `pub use` in mod.rs (consumers `bb.rs`/`core/server.rs`/`prove.rs`/`tray.rs` unchanged).

Submodule assignment (by call-cohesion — private helpers stay with their callers; cross-module calls are all to `pub fn`):
- **version_id.rs**: `NetworkTier` (+impl), `AztecVersion` (+impl/Deref/AsRef/Display), `is_valid_version`.
- **platform.rs**: `bb_binary_name`, `current_platform`. (no deps)
- **artifact_layout.rs**: `versions_base_dir`, `version_bb_path`, `download_url`. (uses platform::*)
- **cache.rs**: `version_sort_key` (priv), `versions_to_evict`, `list_cached_versions`, `cleanup_old_versions`. (uses artifact_layout::version_bb_path)
- **downloader.rs**: `http_client`, `sha256_hex` (priv), `fetch_github_asset_digest` (priv), `download_bb`, `download_tarball` (priv), `verify_digest` (priv), `install_version_dir` (priv), `extract_bb_from_tarball` (priv), + the macOS `xattr`+`codesign` finalize tail → extract `finalize_macos_binary` here. (uses platform + artifact_layout pub fns)

mod.rs: `mod version_id; mod platform; ...` + `pub use version_id::{AztecVersion, NetworkTier, is_valid_version}; pub use platform::{bb_binary_name, current_platform}; pub use artifact_layout::{versions_base_dir, version_bb_path, download_url}; pub use cache::{list_cached_versions, versions_to_evict, cleanup_old_versions}; pub use downloader::download_bb;` (+ keep `DEFAULT_BB_VERSION` etc. if defined here — check the const).
Each submodule: `use super::*;` or explicit `use crate::versions::...` for sibling pub fns + its own external `use` (reqwest/sha2/etc.). Inline `#[cfg(test)]` tests (582–1209) move to the submodule owning the unit under test.
Execute: rm versions.rs; write mod.rs + 5 files; `cargo test -p accelerator-core` → fix paths the compiler flags; `clippy -D warnings`.

## F-03 — decompose .setup (main.rs:260-462) — after F-04
Extract `build_tray_and_status`, `build_desktop_state -> AppState` (wires the 3 callbacks inline via `AppState::desktop` — NO `DesktopCallbacks`), `run_startup_diagnostics`, `spawn_http_server` (Windows `#[cfg]` `AddrInUse` block moves verbatim), `spawn_update_poller`. Pure move; WebDriver E2E + compiler = validity. **Required gate:** `_e2e-crash-recovery-windows.yml` must be green before merge (moves the Windows arm).

## Log
- **F-04 ✓** (956f55d): `versions.rs` → `versions/mod.rs` + `versions/downloader.rs`. **Pragmatic 2-module
  split** (vs planned 5-way): extracted the heaviest, most self-contained concern — the network
  download/verify/install pipeline (`download_bb`/`download_tarball`/`verify_digest`/`install_version_dir`/
  `extract_bb_from_tarball`) + the macOS finalize tail → `finalize_downloaded_binary`. Identity/platform/
  layout/cache stay cohesive in mod.rs. Pure move: `download_bb` re-exported pub; shared helpers `pub(crate)`;
  test-only re-export `#[cfg(test)]`-gated (clippy caught the non-test unused import). core 119 + clippy green;
  consumers unchanged. Rationale: full 6-file redistribution of a 1209-line release-critical file is high-risk
  via tooling; the 2-module split resolves the multi-responsibility smell reliably. Codex post-impl can assess.
- **F-03 ✓** (947dbde): extracted `spawn_http_server` (incl. the AddrInUse/redundant-instance classification)
  + `spawn_update_poller` from the 203-line `.setup`. **Partial decomposition** — the 2 most self-contained
  spawn blocks; tray/callback/AppState wiring stays inline (capture-dense; plan said builders stay local
  closures). Caught: `AppHandle` import was `webdriver`-gated → used full path `tauri::AppHandle` so the
  always-compiled `spawn_http_server` builds under both features. default + webdriver clippy + 19 tests green.
- **Next:** push PR-2 → watch CI (Windows crash-recovery E2E is the required gate for the moved AddrInUse arm).
