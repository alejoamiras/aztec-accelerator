# PR-3 — F-07 CertPaths + F-09 spawn_https + F-08 status ownership

Branch: `quality/pr3-local-refactors` off `main@c3569d9` (PR-2 merged). Order: F-07 → F-09 → F-08.

## Log
- **F-07 ✓** (d729ee5): `CertPaths { ca_cert, leaf_cert, leaf_key }` + `live()`/`staged(dir)`/`exists()`/
  `swap_into(live)`/`remove()`. `write_new_cert_set(&CertPaths)`; `rotate()` → `staged.swap_into(&CertPaths::live())`
  (renames stay ca→leaf→key). REMOVED the 3 served-path accessors (`ca_cert_path`/`leaf_cert_path`/`leaf_key_path`);
  `ca_key_path` kept standalone (legacy-migration target). Updated ALL callers incl. the macOS trust fns
  (`install_ca_trust`/`is_ca_trusted` → `CertPaths::live().ca_cert`) + `load_rustls_config` + `leaf_cert_days_remaining`
  + the `generation_writes_no_ca_key` test. src-tauri 19 + clippy green.
- **F-09 ✓** (d50f9df): `server::spawn_https(state, tls_config)` wrapper in `src-tauri/src/server.rs`; both
  `try_start_https` (launch) + `enable_safari_support` (settings) call it. The divergent TLS-load/failure
  preamble stays upstream (intentional — not unified, per plan). default + webdriver clippy green.
- **F-08 ✓** (6c0bb5c): `resolve_version` → **pure sync** `ResolvedVersion { version, to_download }` (no status,
  no download). `prove()` owns the whole `Proving→(Downloading→Proving)→Idle` machine; the **redundant leading
  Proving is preserved** so the download arm stays `[Proving, Downloading, Proving, Idle]` (opus H2). New tests:
  `resolve_version_flags_uncached_for_download` + `resolve_version_no_download_for_bundled`. The full 4-element
  download-arm sequence is NOT unit-testable (`download_bb` needs the network) → covered by the no-download
  char test (`prove_success_path_and_status_sequence`, still `[Proving, Idle]`) + the structural reorder +
  the `to_download` flag test. `ResolvedVersion` needs `#[derive(Debug)]` (test `unwrap_err`) + `pub(crate)`
  (a `pub(crate)` fn can't return a private type). core 120 + clippy green.

## Infra note
- **GPG signing via 1Password failed mid-run** ("failed to fill whole buffer" — agent hiccup). Per the standing
  AFK rule, committed F-07/F-08/F-09 **unsigned** via `git -c commit.gpgsign=false` (never touched git config).
  Signatures are backfillable later (`git rebase --exec 'git commit --amend --no-edit -S' <base>`).

## Next
- push PR-3 → codex post-impl → CI green → merge → PR-4 (F-05 + F-06, SDK). PR-3 does NOT move the Windows
  AddrInUse arm (that was F-03/PR-2) → normal `accelerator.yml` gate, no Windows-E2E precondition.
