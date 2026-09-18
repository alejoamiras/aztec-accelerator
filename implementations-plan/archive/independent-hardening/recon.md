# Recon — Entry-Point Inventory (independent-hardening)

Fresh inventory from source at main @ `9eff8dc`. No prior audit artifacts consulted.

## HTTP surface (core server, loopback)

- Router (`core/src/server.rs:342-370`): `GET /health`, `POST /prove`; outermost Host guard
  (`server/host.rs::guard` → `host_is_trusted(authority, expected_port)`), then CORS, then routes.
- Origin authz: `server/auth.rs::authorize_origin` (single fn — trace its callers + failure mode).
- `/health` origin-tiered body (probe.rs).
- Bind: `server/bind.rs` — verify exact addresses bound (v4/v6/dual-stack) per OS.
- Headless (`server/src/main.rs`): `--allow-all` / `ACCEL_ALLOW_ALL=1` ⇒ auth_manager None;
  `ALLOWED_ORIGINS=a,b` pre-approves; both together = fail-loud. TLS-free by design.

## Tauri IPC surface (19 commands, commands.rs)

set_speed, set_autostart, set_auto_update, respond_update_prompt, respond_auth,
repair_autostart, renew_cert, remove_https_trust, remove_approved_origin,
record_renewal_prompt, get_verified_info, get_system_info, get_pending_auth,
get_onboarding_state, get_config, get_autostart_enabled, enable_https, disable_https,
complete_onboarding.
→ Each takes webview-controlled args; check validation + state-machine gating
(e.g. can `enable_https` be called before consent? can `respond_auth` approve arbitrary origins?).

## Process spawns

- `core/src/bb.rs` (bb binary exec), `core/src/versions/downloader.rs` (+ signing/codesign paths),
  `core/src/server/owner.rs` (!), `src-tauri/src/{main,crash_recovery}.rs`,
  `src-tauri/src/trust/{linux,windows,macos}.rs` (certutil / security / reg / PowerShell?),
  tests excluded.
- owner.rs spawning processes is unexpected → priority read.

## FS sinks (write/create counts)

update_marker.rs 25 · cache_layout.rs 23 · config.rs 18 · bb.rs 15 · win_acl.rs 14 ·
updater_state.rs 13 · certs.rs 12 · downloader.rs 10 · updater.rs 6 · crash_recovery.rs 6 ·
autostart.rs 6 · trust/linux.rs 3.
→ Path construction sources: version strings, origin strings, config fields, env vars.

## OS integration writes

- trust/linux.rs: certutil -d NSS DB (profile discovery!), no root claimed.
- trust/macos.rs: `security` CLI into user Keychain; login items via autostart.
- trust/windows.rs: CurrentUser Root store (certutil/PowerShell?), registry ops in windows.rs/autostart.rs.
- autostart.rs (2838 LOC): launchd plist / Run key / .desktop generation + healing.

## SDK client surface (packages/sdk)

- Transport builds URLs by interpolation AFTER parsing host literal (`accelerator-transport.ts:111,134,476-478,523,541`)
  — comments describe past URL-authority confusion fixes; re-derive current guarantees independently:
  probe→use TOCTOU, HTTPS-preference logic, httpsOnly knob, redirect behavior of fetch on loopback URLs,
  cert trust source for `https://127.0.0.1` (custom CA? Node agent? Bun fetch specifics).

## Updater chain

updater.rs (906) + update_marker.rs (1290) + updater_state.rs (744) + update_manifest.rs (445):
feed fetch → parse → verify → download → size cap → install; marker file coordinates mid-NSIS state.

## Open questions to resolve during Phase 1

1. Does authorize_origin run for BOTH http and https listeners? Null-origin / missing-Origin handling?
2. What exactly does owner.rs spawn and why?
3. Downloader integrity: digest fetched from where (same host as artifact?) — pinning or self-referential?
4. CA private key location + perms per OS; is issuance name-constrained at generation time?
5. SDK HTTPS trust: how does a browser/Node/Bun client validate the local CA chain?
6. Cache layout: which strings become path components without validation?
7. Config: bounds on speed control; migration paths that reinterpret old files.
8. Crash recovery: what happens on adversarially corrupted state files?
