# Phase 1 Map — `packages/accelerator` (desktop + in-app core)

Mapper: Opus Explore agent, 2026-07-31. `packages/accelerator/server` (separate headless crate) was
confirmed out of scope and not read. `src-tauri/frontend/assets/**` is gitignored build output and
absent in this worktree.

## 1. Module inventory

### core (`packages/accelerator/core/src`, GUI-agnostic, shared with the headless crate)

| Module | Purpose | LOC |
|---|---|---|
| `server.rs` | Axum router (`/health`, `/prove`), `AppState`/`HeadlessState`, CORS + body-limit + host-guard stack, `ProveError` taxonomy, `start()` on 127.0.0.1:59833 | 491 |
| `server/prove.rs` | `/prove`: authorize → inflight gate → Content-Length precheck → body buffer → version resolve/download → prove permit → `bb::prove` → base64 proof | 465 |
| `server/auth.rs` | Origin authorization for `/prove`: approved list, auto-approve-localhost, popup + queue backstop, persists Allow | 123 |
| `server/host.rs` | SEC-01a loopback `Host`/`:authority` allowlist middleware (DNS-rebinding keystone) | 135 |
| `server/bind.rs` | `bind_with_retry`, 5 s AddrInUse budget | 124 |
| `server/probe.rs` | `/health` self-probe for redundant-instance bow-out + F-004 launch floor | 100 |
| `authorization.rs` | RFC-6454 origin canonicalization, extension-ID grammar, `AuthorizationManager` (single-active-popup arbiter, piggyback, caps) | 919 |
| `config.rs` | `AcceleratorConfig`; `~/.aztec-accelerator/config.json` load/atomic-save (0600), `lock_mutate_save*` | 559 |
| `bb.rs` | `find_bb` search chain + `prove()` — spawns `bb` over a private 0700 workspace, 5 min timeout | 440 |
| `versions/downloader.rs` | bb tarball download (64 MB cap) → GitHub digest verify → staged extract (512 MB decompress cap) → atomic publish + marker | 813 |
| `versions/version_policy.rs` | `AztecVersion`, `is_valid_version` traversal guard, revocation denylist, tiering/eviction | 669 |
| `versions/cache_layout.rs` | versions dir layout, `sha256_file`, bb integrity marker | 427 |
| `versions/release_metadata.rs` | Platform naming, GitHub URL, shared reqwest client, asset-digest fetch (SEC-02 caveat) | 195 |
| `update_manifest.rs` | **F-004 Layer A**: minisign verification of the signed envelope + outer-feed binding | 445 |
| `updater_state.rs` | **F-004 Layer B**: monotonic floor + pending intent, atomic owner-only state | 396 |
| `win_acl.rs` | Windows owner-only PROTECTED DACLs (prove workspace, witness, leaf key, config) | 402 |

### src-tauri (`packages/accelerator/src-tauri/src`, desktop)

| Module | Purpose | LOC |
|---|---|---|
| `main.rs` | Entry: `--remove-ca-trust`, logging, Tauri builder, `invoke_handler`, `.setup()` bootstrap, exit handling | 820 |
| `commands.rs` | 19 `#[tauri::command]` handlers + `require_label` window-binding guard + popup arbiter helpers | 816 |
| `windows.rs` | Window creation (settings/onboarding/renewal/auth-popup/update-prompt); `is_local_asset_url` nav guard | 341 |
| `tray.rs` | Tray menu, icon, animation loop | 172 |
| `server.rs` + `server/tls.rs` | Core re-export + HTTPS lifecycle mutex + `spawn_https`; HTTPS listener on 127.0.0.1:59834 | 169 + 90 |
| `certs.rs` | Keyless local CA + leaf (rcgen), atomic 0600 PEM writes, rotation/staging/swap, `migrate_legacy_ca_key`, rustls load | 761 |
| `trust/{mod,macos,linux,windows,stub}.rs` | Trust-store dispatch; Keychain via `/usr/bin/security`; user NSS via safe-path `certutil`; CurrentUser Root via System32 `certutil.exe` | 147+174+687+198+45 |
| `autostart.rs` | Owned Start-on-Login: pure parsers + `#[cfg]` backends, heal, startup reconcile/rearm, `autostart.lock` | 2355 |
| `crash_recovery.rs` | launchd `KeepAlive` / systemd user unit / Task Scheduler XML + `enable_transaction` | 884 |
| `update_marker.rs` | Windows update-window marker/handoff/token transaction + removal decision tables | 1228 |
| `updater.rs` | Update check, `VerifiedUpdate` token, both F-004 layers, size cap, download+install, `updater.lock`, crash-recovery guard | 771 |
| `verified_sites.rs` | Embedded recognition registry (explicitly *not* a security boundary) | 211 |

### frontend-src (production-wired; `.html`/`.css` committed under `src-tauri/frontend/`)

`bridge.js` (invoke wrapper, 700 ms click-steal guard) 126 · `settings.js` 259 · `authorize.js` 108 ·
`onboarding.js` 100 · `update-prompt.js` 20 · `renewal.js` 16 · `style.css` 588.

## 2. Entrypoints

### HTTP(S) routes — router built once (`server.rs:261`), mounted on BOTH listeners

| Method | Path | Handler | Controls |
|---|---|---|---|
| GET | `/health` | `server.rs:319` | Loopback Host guard only, **no origin auth**. SEC-05 gating via `health_is_detailed` (`server.rs:294`): absent or approved Origin ⇒ full body (version, cached versions, `bb_available`, `https_port`); else minimal `{status, api_version}` |
| POST | `/prove` | `prove.rs:224` | Host guard → `authorize_origin` (`auth.rs:15`) → inflight semaphore (8) → Content-Length precheck → 50 MB cap + 30 s read deadline → prove permit (1) |
| OPTIONS | any | `CorsLayer` (`server.rs:262`) | `allow_origin(Any)`, GET/POST, headers `content-type` + `x-aztec-version`, exposes `x-prove-duration-ms` |

Layer order (`server.rs:271-285`): host guard → `cross-origin-resource-policy: cross-origin` →
CORS → `DefaultBodyLimit::max(50 MB)` → routes.

Request-controlled inputs on `/prove`: `Origin` (`auth.rs:24`; **absent ⇒ auto-approve**, `auth.rs:33`);
`x-aztec-version` (`prove.rs:249` → `resolve_version` → `AztecVersion::parse` →
`check_version_selectable`) which can trigger a **network download + install + execute** of a remote
bb binary (`downloader.rs:20`); `Host`/`:authority` (`host.rs:50`); `Content-Length` incl. comma-lists
(`prove.rs:144`); body → msgpack IVC inputs written to a private temp file for `bb` (`bb.rs:158,190+`).

### Tauri IPC — 19 commands (`main.rs:569-589`), declared in `build.rs` (flips app commands to per-window default-DENY), each also calling `require_label` (`commands.rs:304`)

settings-bound: `get_config` (:37), `get_system_info` (:116), `get_autostart_enabled` (:46),
`set_autostart` (:77), `repair_autostart` (:63), `set_speed` (:87), `set_auto_update` (:696),
`remove_approved_origin` (:97), `enable_https` (:344), `disable_https` (:469), `remove_https_trust` (:487).
auth-popup-bound: `get_verified_info` (:134, any `auth-<32 hex>` via `require_auth_window` :326),
`get_pending_auth` (:203, exact `auth-{sha256_16(request_id)}`), `respond_auth` (:147, exact label +
server-side `resolve_active` arbiter). Others: `get_onboarding_state` (:535), `complete_onboarding`
(:566), `renew_cert` (:625), `record_renewal_prompt` (:671), `respond_update_prompt` (:711).
Per-window ACLs in `capabilities/*.json`; no `core:default`, no `core:window`, no updater/process/
autostart plugin grants — windows are closed from Rust.

### Windows / tray / protocols

Tray events (`main.rs:382-405`): quit (Windows: `autostart::quit_disarm()` then exit), show_logs,
open_github, settings. `open_in_browser` spawns `open`/`xdg-open`/`explorer` (`main.rs:34-46`).
Tray animation 50 ms (`tray.rs:140`). `ExitRequested` with `code == None` prevented (`main.rs:263,746`).
Navigation guard `.on_navigation(is_local_asset_url)` + `.on_new_window(Deny)` (`windows.rs:87-88`,
predicate `:19`). Auth-popup close listener (`commands.rs:258`). Popup URL params
`authorize.html?origin=…&requestId=…` (`windows.rs:184-188`), update prompt (`:247`).
**No deep links / custom protocols** (verified by grep) — only Tauri's built-in asset protocol.
`tauri-plugin-webdriver` (port 4445, `main.rs:556-560`) is behind the non-default `webdriver` feature.

### CLI / env

argv: only `--remove-ca-trust` (`main.rs:486`), runs before GUI/server init, exits 1 if incomplete.
Env in production paths: `RUST_LOG` (`main.rs:532`), `AZTEC_ACCEL_NO_UPDATE` (`:216`),
`AZTEC_ACCEL_FORCE_UPDATE_CHECK` (debug only, `:220`), **`BB_BINARY_PATH` — unversioned trusted
override of the bb executable** (`bb.rs:28`), `HOME` fallback (`bb.rs:74`), `APPIMAGE`/`APPDIR`
(provenance-checked by `appimage_self`, `autostart.rs:1204-1205`), `SystemRoot`/`windir`
(`crash_recovery.rs:389`, `trust/windows.rs:41`), `LOCALAPPDATA` (`updater.rs:626`).

### Startup filesystem inputs (all under `~/.aztec-accelerator/` unless noted)

config.json (**fail-open**: malformed ⇒ defaults, `config.rs:127`) · legacy `certs/ca.key`
(**fail-closed**, `main.rs:654`) · cert set (invalid ⇒ reset `https_enabled`) · `updater-state.json`
(**fail-closed**) · `updater.lock` · `autostart.lock` (10 s bounded) · `update-in-progress.json` /
`update-txn` / `update-txn-done` (Windows) · autostart artifact (plist / .desktop / Run key, bounded
read `autostart.rs:31`) · `versions/<ver>/{bb,marker}` · compile-time embedded `verified-sites.json`,
updater pubkey, tray PNGs.

### Background tasks

HTTP server (`main.rs:725`) · HTTPS bring-up (`main.rs:663`) · **update poller** 5 s warm-up then 12 h
(`main.rs:736`) · version-floor tracker every 3 s ≤40 iters (`main.rs:739`) · tray animation 50 ms ·
auth-popup 60 s auto-deny (`commands.rs:238`) · detached cache cleanup after each download
(`prove.rs:287`) · macOS/Windows renewal prompt once, throttled 20 h (`main.rs:700-718`).

## 3. Trust boundaries

**B1 — browser origins → local listener (largest surface).** Host guard (`host.rs:50`, exact loopback
literal + exact port; rejects userinfo smuggling, alternate numeric forms, Host/`:authority`
disagreement) → `authorize_origin` (`auth.rs:15`; **absent Origin ⇒ allow**, `:33`) → approving writes
persistently to config (`auth.rs:101`) granting unattended proving → `x-aztec-version` can cause
**network fetch + on-disk install + child-process execution** (`prove.rs:268` → `downloader.rs:20` →
`bb.rs:26`), integrity resting on a GitHub asset digest fetched from the same control plane
(**documented circular-trust gap SEC-02**, `release_metadata.rs:73-81`) → `/health` fingerprint
reduction for unapproved origins (`server.rs:294`).

**B2 — updater fetch + signature.** Feed `https://aztec-accelerator.dev/releases/latest.json`; pinned
minisign pubkey in `tauri.conf.json`, read back at `updater.rs:18-29`. Layer A
(`update_manifest.rs:124`): minisign over verbatim decoded bytes, schema check, canonical SemVer,
outer-feed binding, unique artifact-URL match, signature + size equality. Layer B
(`updater_state.rs`): monotonic floor + pending intent, gate `updater.rs:132`, re-checked under lock
at install (`:304`). Size cap from the **signed** envelope (`:326`) + post-download length equality
(`:362`). Documented residuals: unbounded feed-body buffering before parse (`:213-219`), unbounded
artifact buffering when signed-small/served-large (`:320-325`). `perform_update` accepts only a
`VerifiedUpdate` (private fields, single constructor, `:110`).

**B3 — CA / OS trust stores.** macOS `/usr/bin/security add-trusted-cert -r trustRoot -k <login>`
(`trust/macos.rs:25`), verify (`:49`), delete by SHA-1 (`:79`). Linux `certutil` via safe-path check
(`trust/linux.rs:20,46,62`) into `~/.pki/nssdb` + every Firefox profile (`:188-211`). Windows
`certutil.exe` from hardcoded System32 with env fallback (`trust/windows.rs:36`), serial-keyed trust
with `Disallowed` override (`:102`). CA is **keyless on disk** — signing key in `Zeroizing`, dropped
before writes (`certs.rs:192-210`), name-constrained to loopback (`:103-113`).
`migrate_legacy_ca_key_at` fail-closed (`:247`). Rotation stages `*.new.<pid>` then renames (`:412`);
`swap_into` (`:77`) is **not atomic across three files**, mitigated by `leaf_matches_ca` (`:162`). All
cert transitions serialized by the `https_lifecycle` mutex (`server.rs:135`, claimed `src-tauri/src/server.rs:28`).

**B4 — autostart / crash-recovery OS persistence.** Windows Run value written **quoted**
(`run_value_quote` `autostart.rs:528`, write `:1078-1087`); `StartupApproved\Run` blob (`:1144-1163`).
macOS LaunchAgent plist via a real plist parser (`plist_set_program` `:205`), patched with
`KeepAlive`/`ThrottleInterval` by string insertion (`crash_recovery.rs:142-165`). Linux `.desktop` +
**systemd user unit** (`crash_recovery.rs:304`) with `systemctl --user` shell-outs (`:308,311,333,352`);
`ExecStart` escaped/fail-closed by `systemd_exec_start` (`:225`). Windows Task Scheduler XML by
`task_xml` (`:481`) with `xml_escape` (`:522`), UTF-16LE+BOM temp file, `schtasks /Create /F /XML`
(`:427`); repeating `PT1M` trigger `IgnoreNew` — a live relaunch loop that must be disarmed before
NSIS mutates files (`updater.rs:443`) and on quit (`autostart.rs:1425`). Path preflight
`autostart_path_is_safe` (`crash_recovery.rs:211`). Anti-theft `implicit_arm_gate`/`implicit_arm_allowed`
(`autostart.rs:1521,1547`). Lock nesting: `updater.lock` (outer) → `autostart.lock` (inner)
(`updater.rs:415-529`).

**B5 — NSIS installer hooks.** `POSTINSTALL` renames `update-txn` → `update-txn-done` (the completion
proof). `POSTUNINSTALL` on a **real** uninstall only (guards `$UpdateMode <> 1` AND short-path-normalized
`$EXEDIR != $INSTDIR`) runs `certutil -user -delstore Root "Aztec Accelerator Local CA"` and
`RMDir /r "$PROFILE\.aztec-accelerator\certs"`. Guard correctness is load-bearing.

**B6 — process spawning (full inventory).** `bb prove` (`bb.rs`, paths from a private temp dir, binary
from `find_bb`: env override → verified cache → sidecar → `~/.bb` → PATH on non-Windows) ·
`open`/`xdg-open`/`explorer` (`main.rs:37-41`) · `/usr/bin/security` ×4 (`trust/macos.rs:25,51,62,79`) ·
Linux `certutil` (`trust/linux.rs:255`) · Windows `certutil.exe` (`trust/windows.rs:60,70,83,119,127`) ·
`systemctl --user` (`crash_recovery.rs:308,311,333,352`) · `schtasks.exe` (`:427,456,459`).

**B7 — webview → Rust IPC.** Double-gated: per-window capability ACL (default-DENY once `build.rs`
declares the manifest) plus `require_label`. `respond_auth`/`get_pending_auth` bind the caller label to
`auth-{sha256_16(request_id)}` (`commands.rs:159,210`); `resolve_active` enforces single-active-popup
server-side. CSP blocks inline script/style and restricts `connect-src` to `ipc:`; navigation off the
local asset origin blocked in Rust (`windows.rs:19`) because Linux `<meta>`-delivered CSP ignores
`frame-ancestors`.

**B8 — file writes outside the app dir.** LaunchAgents plist · XDG autostart `.desktop` · systemd user
unit · `HKCU\…\Run` + `StartupApproved\Run` · OS trust stores · Task Scheduler store · log dir (0700,
`main.rs:520`) · prove workspace (`bb.rs:82`) · Windows temp XML.

## 4. Dependency graph (one level)

`main.rs` → `tray`, `windows` (bin-local) + `aztec_accelerator::{commands, certs, config,
verified_sites, log_dir, server, autostart, updater, trust, bb, authorization}` + tauri,
tauri_plugin_updater, tauri_plugin_process, tracing_subscriber, tokio_rustls.
`aztec_accelerator` re-exports `accelerator_core::{authorization, bb, config, log_dir, versions}`;
`server` → core server + `server::tls`; `commands` → authorization, config, verified_sites, autostart,
certs, trust, server, updater, crash_recovery; `certs` → trust, `win_acl`, rcgen, x509-parser, rustls;
`autostart` → crash_recovery, update_marker (win), updater (lock probe), plist, winreg, fs2;
`crash_recovery` → autostart (`appimage_self_from_env`, Linux); `update_marker` lock-free (consumers
inject closures); `updater` → core update_manifest/updater_state + update_marker, autostart,
crash_recovery, commands::ConfigState.
`accelerator_core`: `server` → authorization, bb, config, versions (+ bind/probe/auth/host/prove);
`config` → `authorization::CanonicalOrigin`; `bb` → versions, win_acl; `versions::downloader` →
cache_layout, release_metadata, version_policy; `update_manifest` → minisign-verify, semver;
`updater_state` → semver; `win_acl` → windows-sys.

## 5. Security-relevant libraries

axum 0.8 / hyper 1 / hyper-util / tower-http (CORS, set-header) · tokio-rustls 0.26 + rustls-pemfile 2,
provider aws-lc-rs installed `main.rs:511` · rcgen 0.13 (+zeroize) · x509-parser 0.18 (`verify`) ·
**minisign-verify pinned `=0.2.5`** + tauri-plugin-updater 2 · sha2 0.11 · semver 1 · winreg 0.55
(Run/StartupApproved, `updater.rs:613`) · windows-sys 0.61 (ACLs) · `std::process::Command` +
`tokio::process::Command` (`kill_on_drop`) · serde/serde_json, plist 1, base64 0.22, urlencoding 2 ·
flate2 + tar with explicit decompression cap · reqwest 0.12 (300 s/30 s timeouts) · url 2 · fs2 flock ·
uuid v4 · parking_lot + tokio Semaphore/Mutex/oneshot · `@tauri-apps/api/core` invoke with
`withGlobalTauri: false` · release profile `panic="abort"`, `strip`, `lto`.

## 6. Test surfaces

In-file `#[cfg(test)]` is heavy. Notable: `core/src/server/tests.rs` (1269 LOC — /health shapes + SEC-05
gating, CORS, /prove error contract, auth flows incl. headless + 429 caps + timeout, **DNS-rebinding
rejection**, body caps, version resolution) · `host.rs:76-135` (loopback-authority table) ·
`prove.rs:362-465` (Content-Length parsing, waiter cap, body/permit decoupling, stalled-body timeout on
a virtual clock) · `certs.rs:482-761` (partial-swap detection, mismatched key, 0600, "ca.key must never
be written", fail-closed migration) · `commands.rs:770-816` (label guards, auth-label grammar) ·
`windows.rs:267-341` (navigation allow/deny table incl. `data:`/`file:`/`javascript:`) ·
`updater.rs:699-771` (pubkey-matches-config, guard rearm/defuse) · `main.rs:755-820` · `autostart.rs`
pure-layer tables compiled on every OS · `update_marker.rs` decision tables over injected closures ·
`downloader.rs` tar/gzip bomb caps.
Integration (`src-tauri/tests/`): `autostart_heal.rs` (real `CreateProcessW` on Windows),
`tls_handshake.rs`, `trust_{linux,macos,windows}.rs`.
E2E (not finding-eligible): `packages/accelerator/e2e/` Playwright + `e2e-webdriver/`.
Script guards: `tauri-trust-boundary.test.ts` (set-equality `build.rs` commands ↔ `generate_handler!`),
`tauri-identity.test.ts`, `nsis-hook-test.sh`, `harness.test.nsi`, updater smokes.

## 7. Generated / vendored / fixture — NOT finding-eligible unless production-wired

`src-tauri/frontend/assets/**` (generated from `frontend-src/`, gitignored; integrity enforced at build
time by `build.rs::verify_frontend_bundles`) · `src-tauri/permissions/autogenerated/**` (generated;
hand-written truth is `capabilities/*.json`) · `gen/schemas/**` · `target/`, `node_modules/`, `dist/` ·
`src-tauri/binaries/`, `AZTEC_VERSION` · `core/tests/fixtures/updater/**` · `e2e/tauri-mock.js` (injects
`window.__CLICK_GUARD_MS__ = 0` — disables a production control **in tests only**) ·
`core/examples/update-manifest.rs` (release tooling) · `packages/accelerator/server/**` (owner-excluded) ·
`verified-sites.json` (compile-time embedded, production-wired but explicitly **not** a security
boundary, `verified_sites.rs:3-14`).

## Uncertainty flags (mapper refused to guess — verify before treating as production-wired)

1. **`tauri-plugin-webdriver` / port 4445** (`main.rs:556-560`) is behind the non-default `webdriver`
   feature, which also compiles out the update poller, onboarding auto-show, renewal window, and the
   unattended autostart heal. Believed CI-only; the release build command was not traced.
2. **`trust::stub`** compiles only for targets that are neither macOS/Linux/Windows; no such target ships.
3. **`snapshot_restore_roundtrip_for_tests`** (`autostart.rs:1655`) is a `pub fn` in the production
   library (not `#[cfg(test)]`) that mutates real autostart state; no caller found in `src/`.
4. **`KNOWN_VULNERABLE_VERSIONS`** (`version_policy.rs:194`) is **empty** — `check_version_selectable`
   enforces only canonical-SemVer well-formedness; there is deliberately **no version floor** on
   `x-aztec-version`.
5. **`crash_recovery::schtasks_exe`** (`:388`) resolves via `SystemRoot`/`windir` **without** the
   hardcoded-System32-first preference used by `trust/windows.rs:36`. Asymmetry observed, not adjudicated.
6. **NSIS uninstall guard** relies on `$EXEDIR != $INSTDIR` semantics measured against tauri-bundler
   2.8.1; the file documents that a bundler change could invalidate it.
