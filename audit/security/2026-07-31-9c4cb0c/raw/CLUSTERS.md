# Phase 2 clusters (security route: by entrypoint + sink family)

Derived from the Phase 1 maps. 8 clusters × 2 agents (1 Opus + 1 codex xhigh, blind to each other —
medium tier has no cross-rebuttal). Stable names so future runs are comparable.

| # | Cluster | Files | Why this boundary |
|---|---|---|---|
| C1 | `listener-ingress` | `core/src/server.rs`, `server/host.rs`, `server/auth.rs`, `server/bind.rs`, `server/probe.rs`, `core/src/authorization.rs` | ENTRYPOINT: the only network-reachable surface. Host guard (anti-rebinding), Origin authz, CORS, `/health` gating. Owner concern #1. |
| C2 | `prove-pipeline` | `core/src/server/prove.rs`, `core/src/bb.rs`, `core/src/win_acl.rs` | SINK: request body → private temp file → **child process exec**. Witness confidentiality, caps/semaphores, workspace ACLs. |
| C3 | `version-supply-chain` | `core/src/versions/{downloader,version_policy,cache_layout,release_metadata}.rs` | SINK: an untrusted request header can cause **network fetch → disk install → execute**. Contains the documented SEC-02 circular-trust gap and the empty revocation list. |
| C4 | `updater-trust` | `core/src/update_manifest.rs`, `core/src/updater_state.rs`, `src-tauri/src/updater.rs` | SINK: signed-update verification (F-004 A+B), size caps, install handoff. Remote-code path if verification is bypassable. |
| C5 | `certs-and-trust-stores` | `src-tauri/src/certs.rs`, `trust/{mod,macos,linux,windows}.rs`, `src-tauri/src/server.rs`, `server/tls.rs` | SINK: writes into **OS trust stores** + private key handling + TLS config. Owner concern #2. |
| C6 | `persistence-autostart-recovery` | `src-tauri/src/autostart.rs`, `crash_recovery.rs`, `update_marker.rs`, `src-tauri/nsis/hooks.nsi` | SINK: **OS persistence** (Run key, LaunchAgent, systemd unit, Task Scheduler) + the update-window transaction. Owner concern #3; heavily changed recently. |
| C7 | `ipc-and-webview` | `src-tauri/src/commands.rs`, `windows.rs`, `tray.rs`, `main.rs`, `verified_sites.rs`, `core/src/config.rs`, `frontend-src/**`, `capabilities/*.json` | ENTRYPOINT: webview → Rust IPC, window/nav guards, tray actions, argv/env, config fail-open. |
| C8 | `sdk` | `packages/sdk/src/**` | ENTRYPOINT/SINK: published third-party library; witness egress, `/health` + `/prove` response trust, publish surface. |

Excluded from all clusters (per SCOPE.md): `packages/accelerator/server`, playground, landing,
`.github/workflows`, `infra/tofu`, generated/vendored paths listed in the maps.
