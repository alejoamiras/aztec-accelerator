Post-implementation ADVERSARIAL review of PR-2 (F-04 + F-03, both BEHAVIOR-PRESERVING pure moves). cwd = repo root.

Diff: `git diff main...quality/pr2-split-giants` (commits 956f55d F-04, 947dbde F-03). These move code without changing behavior — your job is to verify that claim + catch any regression. Release-critical: `versions.rs` is the bb download/cache path.

**F-04** — extracted the bb download pipeline from `core/src/versions.rs` into `core/src/versions/downloader.rs` (file→`versions/mod.rs` + `downloader.rs`):
- Moved: `download_bb`, `download_tarball`, `verify_digest`, `install_version_dir`, `extract_bb_from_tarball`; the macOS xattr+codesign tail extracted into `finalize_downloaded_binary` (a `#[cfg(not(macos))]` no-op sibling).
- Verify: is `download_bb`'s pipeline byte-identical (cache-check → download → verify BEFORE install → unix chmod → macOS finalize)? Does `finalize_downloaded_binary` preserve the exact xattr(warn-only)/codesign(error→`remove_dir_all(version_dir)`→Err) behavior? Are visibilities right (`download_bb` pub re-export; `http_client`/`sha256_hex`/`fetch_github_asset_digest` `pub(crate)`; `install_version_dir`/`extract_bb_from_tarball` `pub(crate)` + `#[cfg(test)]` re-export)? Do consumers (`bb.rs`, `server/prove.rs`, `src-tauri/tray.rs`) still resolve `versions::*`?

**F-03** — extracted `spawn_http_server` (incl. the `AddrInUse`/redundant-instance classification + Windows `exit(0)` bow-out) + `spawn_update_poller` from `src-tauri/main.rs`'s `.setup` closure:
- Verify: same `AddrInUse` structural classification (`downcast_ref::<io::Error>` + `ErrorKind::AddrInUse`), same `cfg!(windows) && healthy_aztec_on_port()` bow-out with `exit(0)`, same tray/status error messages? `spawn_update_poller` — same 5s warm-up + 12h loop? Captured vars threaded correctly (the original MOVED `status_for_diagnostics`/`tray_for_diagnostics`; the call now passes them — any double-use/borrow issue)? The `webdriver` cfg-gating intact (poller gated; `spawn_http_server` always compiled, uses `tauri::AppHandle` full path)?

Any behavior change in either finding? Any release-critical risk? Lead with a one-line verdict (`clean` / `issues: …`), then findings by severity with file:line. ~400–700 words.
