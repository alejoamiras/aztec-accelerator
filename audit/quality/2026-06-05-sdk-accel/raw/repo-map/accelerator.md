# Repo map — accelerator (quality audit)

**Package:** Tauri-2 desktop app. Scope: `packages/accelerator/src-tauri/src` (Rust) + frontend (`src-tauri/frontend`) + `scripts/`.

## Rust inventory (src-tauri/src, 5768 LOC)
| File | LOC | Purpose |
|---|---|---|
| `server.rs` | 1335 | axum HTTP/HTTPS router, `/health`+`/prove`, auth+version resolve, bb invoke, bind-with-retry. **God module; long `prove()` handler ~L487-577.** |
| `versions.rs` | 964 | bb version cache, tier retention (nightly/devnet/testnet/mainnet), download/extract, digest verify. NetworkTier::from_version prefix-parsing. |
| `certs.rs` | 561 | TLS cert gen (keyless CA, 824d leaf), macOS trust install, renewal, pem load. Platform `#[cfg]` blocks. |
| `main.rs` | 495 | Tauri entry, tray, HTTPS startup, 12h update loop, crash recovery, window lifecycle. Status-string parsing for animation (~L356). |
| `crash_recovery.rs` | 452 | macOS LaunchAgent plist / Linux systemd / Windows Task Scheduler — parallel platform impls. |
| `config.rs` | 394 | config schema (Speed enum, safari_support, approved_origins, auto_update), JSON load/save, migration. |
| `authorization.rs` | 383 | origin canonicalization (RFC 6454), pending-request dedup, 60s timeout, decision broadcast. |
| `bb.rs` | 280 | bb binary search chain (env→cache→sidecar→bbup→PATH), prove invoke w/ timeout+threads. |
| `commands.rs` | 261 | 14 Tauri commands. `enable/disable_safari_support` duplicated macOS vs !macOS stubs (~L132-190). |
| `verified_sites.rs` | 211 | embedded JSON registry (recognition badge, NOT security). |
| `tray.rs` | 172 | tray icon/menu, 24-frame proving animation (static `[&[u8];24]`). |
| `updater.rs` | 130 | check/perform update, Windows crash-recovery re-arm. |
| `windows.rs` | 107 | Settings/Auth/Update window lifecycle. 60s timeout const (dup w/ server.rs). |
| `lib.rs` | 23 | re-exports. |

Headless: `server/src/main.rs` (74) — same router, CLI/env-gated.

## Frontend (src-tauri/frontend)
`settings.html` (179), `authorize.html` (65), `update-prompt.html` (47), `tauri-bridge.js` (86, shared IPC utils), `style.css` (204).

## Scripts
`copy-bb.ts` (190, sidecar extract + Windows fetch/SHA-pin), `updater-feed-server.ts` (73).

## Dep graph (use crate::)
lib→all; main→{auth,commands,server,certs,config,verified_sites,tray,windows,updater}; commands→{auth,config,verified_sites,certs,server}; server→{auth,config,bb,versions}; bb→versions; updater→commands; verified_sites→authorization. No cycles.

## Exclude
`target/`, `gen/`, `node_modules/`, `dist/`, `icons/`, `binaries/`, `test-results/`, `*.spec.ts` e2e (unless prod-wired).

## First-glance smells (locations only — for finder agents to verify)
1. **Large Class / Long Method:** server.rs (1335) — `prove()` ~L487-577 mixes auth + version-resolve + bb-invoke + error-serialize.
2. **Shotgun Surgery / Duplicated platform arms:** safari `#[cfg]` in commands.rs ~L132-190; platform blocks in certs.rs + crash_recovery.rs (plist/systemd/scheduler).
3. **Duplicated constant:** 60s auth timeout in windows.rs ~L72 + server.rs ~L361 (not a shared const).
4. **Primitive Obsession:** version strings parsed by prefix (NetworkTier::from_version, versions.rs ~L38-62); raw msgpack `/prove` req + magic header strings ("x-prove-duration-ms") — no DTOs.
5. **Stringly-typed:** status animation via `text.contains("Proving"|"Downloading")` (main.rs ~L356); hand-assembled `json!({})` errors (server.rs).
6. Magic numbers: 5min prove timeout, 824d cert, 60s — scattered.

## Clusters (Phase 2)
1. **sdk** (separate map) 2. **server** = server.rs 3. **versions-bb** = versions.rs+bb.rs 4. **certs-recovery** = certs.rs+crash_recovery.rs 5. **app-shell** = main.rs+tray.rs+updater.rs+windows.rs+commands.rs 6. **config-auth-ui** = config.rs+authorization.rs+verified_sites.rs+frontend+copy-bb.ts
