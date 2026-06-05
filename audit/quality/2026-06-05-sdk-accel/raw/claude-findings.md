# Phase 2 raw findings — Claude (Sonnet) finders, 6 clusters

## Cluster: sdk (accelerator-prover.ts)
1. **Long Method / Divergent Change** — `checkAcceleratorStatus()` L202-319 (118 LOC: cache + dual-protocol probe+retry + multi-version vs legacy parse). Extract `#probeHealth` + `#parseHealthResponse`. *structural*
2. **Temporal Coupling / boilerplate state-machine** — ~17 ad-hoc `#onPhase?.()` emissions across `createChonkProof` (L331-404) + `#proveLocally` (L417,422); legal orderings enforced nowhere. *architectural*
3. **Duplicate Code** — WASM-fallback sequence twice: not-available L336-339 vs 403-denial L384-390. Extract `#fallbackToWasm(steps, preamblePhase?)`. *structural*
4. **Primitive Obsession** — `AcceleratorStatus` (L46-59) flat iface w/ 6 optionals = implicit discriminated union; consumers re-derive. Replace w/ discriminated union. *structural*
5. **Feature Envy** — port/host resolution in constructor L145-167; extract `resolveAcceleratorConfig()`. *local*

## Cluster: server.rs
1. **Primitive Obsession** — `ProveError = (StatusCode, String)`; 7/10 error sites bypass `json_error` w/ inline `serde_json::to_string(&json!{}).unwrap()` (L315-319,338-343,351,368-376,398-404,504-509,516-519,565-567,421-424,456-460). Typed `ProveErrorBody: Serialize+IntoResponse`. *structural*
2. **Duplicate Code** — 60s auth-timeout split: server.rs:361 + windows.rs:72. Shared const. *structural* [CONVERGES w/ app-shell #3]
3. **Duplicate Code** — `MAX_BODY_SIZE` literal `50*1024*1024` inline at L213 vs const at L485. *local*
4. **Large Class / Divergent Change** — server.rs 578 prod-LOC, 6 unrelated clusters (bind-retry / TLS loop / CORS / auth-glue / version-resolve / prove). Extract submodules. *architectural*
5. **Long Method + Temporal Coupling** — `authorize_origin` L288-406 (118 LOC, 6 ordered phases). Extract phase fns. *structural*
6. **Data Clump** — `AppState` L33-43 merges GUI callbacks + headless fields → Option-guard everywhere (4 guards in authorize_origin). Split GuiCallbacks. *structural*
7. (cosmetic) misleading test names + tautology assert L1259,1291,1314. *local*

## Cluster: versions-bb
1. **Primitive Obsession** — version strings raw through API: `from_version` L38-52 + `version_sort_key` L133-140 + signatures (versions.rs:84,148,272; bb.rs:18,79). `ParsedVersion`/`BbVersion` value object w/ tier+suffix computed once. *architectural*
2. **Temporal Coupling (doc)** — `is_valid_version` "single source of truth" prose attached to `download_bb`'s rustdoc block, not the fn. *structural (weak)*
3. **Long Method** — `download_bb` L272-421 (149 LOC: guard / stream / digest / extract+macOS-codesign). Extract `install_bb_binary`. *structural*
4. **Data Clump** — `barretenberg-{}.tar.gz` format in download_url L121-127 + download_bb L331. Extract `bb_asset_name()`. *local*
5. **Duplicate Code** — `versions_to_evict` re-invokes `from_version` L168-170 on already-classified strings; compute `bundled_tier` once. *local* [consequence of #1]
6. **Feature Envy** — `dirs_next`/`home_dir_fallback` wrappers bb.rs:66-72 (trivial delegators; inconsistent w/ versions.rs:68 direct call). *cosmetic/local* [CONVERGES w/ certs-recovery #2]

## Cluster: certs-recovery
1. **Parallel Platform Impls / Shotgun Surgery** — `enable/disable_crash_recovery` 6 disjoint `#[cfg]` blocks (crash_recovery.rs:23-51,76-80,93-158,163-177,216-275,281-308); **divergent signatures** (Win disable→bool, mac/linux→()). Extract `trait CrashRecovery`. *architectural*
2. **Duplicate Code** — home-dir boilerplate inconsistent fallback: certs.rs:14-15 `"."` vs crash_recovery.rs:84-85 `"~"` vs certs.rs:303-305 `"."`. Shared `home_dir()`. *structural* [CONVERGES w/ versions-bb #6]
3. **Long Method** — `enable_crash_recovery` Linux L93-158 + Windows L216-275 mix orchestration+I/O+subprocess. Extract install helpers. *structural*
4. **Temporal Coupling** — `write_pem_file` sets 0o600 twice (open L177-186 + post-rename set_permissions L193-197); 2nd redundant. *local*
5. **Duplicate Code (test)** — 2 tests rebuild CA+leaf verbatim (certs.rs:414-445,448-490) w/ stale consts (3650 not CA_VALIDITY_DAYS; 825 not 824). *local*

## Cluster: app-shell (main/tray/updater/windows/commands)
1. **Stringly-Typed / Primitive Obsession** — animation trigger `text.contains("Proving")||text.contains("Downloading")` main.rs:356, coupled to display strings server.rs:437,465,531. Typed `ServerStatus` enum in StatusCallback. *structural*
2. **Duplicate Code** — HTTPS-spawn pattern main.rs:83-89 (`try_start_https`) vs commands.rs:153-162 (enable_safari_support); one has recovery path, other doesn't. Extract `spawn_https_server`. *structural*
3. **Duplicate Code / Config Sprawl** — 60s timeout windows.rs:72 + server.rs:361. *local* [CONVERGES w/ server #2 — SAME finding]
4. **Duplicate Code** — 3 window-open helpers windows.rs:20-35,41-81,88-107 (builder chain copy-paste). Extract `open_or_focus_window(WindowConfig)`. *local*
5. **Duplicate Code** — lock-write-save boilerplate 6 sites commands.rs:49-52,60-63,147-149,171-173,195-197,217-219; respond_update_prompt SWALLOWS save error (divergence). Extract `mutate_config(f)`. *local*
6. **Temporal Coupling** — main.rs AppState clone stutter: https_port patched on 2 clones L347-366,376-378,396-397. *local*
   - REJECTED by finder: safari_support `#[cfg]` stubs (genuinely different impls, correct pattern).

## Cluster: config-auth-ui
1. **Duplicate Code / Shotgun Surgery** — `is_auto_approved` (authorization.rs:126-147) bespoke strip_prefix/split parser duplicates `canonicalize_origin` (L21-57 url::Url). Reuse canonical host. *structural*
2. **Data Clump / Speculative Generality** — `VerifiedSite` (verified_sites.rs:24-34) 3 `#[allow(dead_code)]` pub fields + parallel `VerifiedSitesEntry` (43-51) + `VerifiedSiteDto` (commands.rs:84-87) = 3-place edits. Collapse. *structural*
3. **Duplicate Code** — `fetchWindowsBb` twin size guards copy-bb.ts:99-104,106-110. *local*
4. **Long Method asymmetry** — copy-bb.ts main(): Windows extracted (L159-160) vs Unix inlined (L161-178). Extract `copyUnixBb`. *local*
5. **Config Sprawl** — `default_config_version` free fn config.rs:44,69-71 (serde-default indirection). *cosmetic/local*
6. **Duplicate Code** — popup scaffolding authorize.html:11-30 ≈ update-prompt.html:11-30. *local*
   - REJECTED: tauri-bridge.js helpers NOT duplicated (all pages use them); style.css well-factored; AuthorizationManager methods clean.
