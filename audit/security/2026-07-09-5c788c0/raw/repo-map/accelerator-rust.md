# Repo Map — packages/accelerator (Rust): core + server + src-tauri

3 crates: `core` (GUI-agnostic proving server), `server` (headless), `src-tauri` (desktop GUI, re-exports core).

## HTTP(S) routes (both listeners, router at server.rs:207)
- `GET /health` (server.rs:218,265) — reads Origin. Minimal `{status,api_version}` for unapproved cross-origin; detailed (app ver, aztec_version, available_versions, bb_available, https_port) for absent/approved origin. Debug build adds parallelism.
- `POST /prove` (prove.rs:101) — reads Origin (auth), `x-aztec-version` (prove.rs:127), raw body (msgpack witness, 50MB cap prove.rs:99). Returns `{proof: base64}` + `x-prove-duration-ms`. Errors = text/plain.
- Listeners: HTTP 127.0.0.1:59833 (server.rs:191); HTTPS 127.0.0.1:59834 (tls.rs:15, GUI only). CORS `allow_origin(Any)`, GET/POST, headers content-type+x-aztec-version.

## Tauri IPC commands (main.rs:447-460, commands.rs)
get_config, get_autostart_enabled, set_autostart(toggles crash recovery), set_speed, remove_approved_origin(origin str), get_system_info, get_verified_info(origin->display_name), respond_auth(request_id+origin+allowed+remember; resolves by opaque UUID), enable/disable_safari_support(macOS), set_auto_update, respond_update_prompt.

## Env/CLI
Headless: `--allow-all`, `ACCEL_ALLOW_ALL`, `ALLOWED_ORIGINS`, `AZTEC_BB_VERSION`, `RUST_LOG`. Desktop: `AZTEC_ACCEL_NO_UPDATE`, `AZTEC_ACCEL_FORCE_UPDATE_CHECK`. Core: `BB_BINARY_PATH`, `HOME`, `HARDWARE_CONCURRENCY`(set for subprocess).

## Trust boundaries
- **Host allowlist** (host.rs:50 guard, OUTERMOST middleware server.rs:228, before CORS/routes). Parses via Authority, exact expected_port, rejects userinfo, only 127.0.0.1/localhost/::1. Reads HTTP/1.1 Host AND HTTP/2 :authority; fails closed on disagree/absent (host.rs:57-64). Per-listener port.
- **Origin authz** (auth.rs:15, top of /prove BEFORE body buffer, prove.rs:110). Order: no auth_manager=>allow (headless); **absent Origin=>allow (auth.rs:33, curl/script bypass)**; malformed=>InvalidOrigin; approved(persisted or auto-localhost)=>allow; else popup gate / headless deny. is_approved authorization.rs:291. Popup resolves by opaque UUID (authorization.rs:236,261).
- `/health` detail gate: health_is_detailed keys off Origin approval (server.rs:240).
- Both HTTP/HTTPS identical router+guards; loopback only.

## Witness lifecycle (private ZK inputs, msgpack)
1. Body -> `to_bytes(raw, 50MB)` in-memory Bytes (prove.rs:112). Authz runs first.
2. Held as `Bytes`, passed by ref to `bb::prove`. Size logged debug-only.
3. **bb::prove writes witness verbatim to `<tmpdir>/ivc-inputs.msgpack` via fs::write (bb.rs:90)**. tempfile::tempdir() (0o700 dir), **NO explicit chmod on msgpack file** (contrast config/certs 0o600).
4. Subprocess args: `--ivc_inputs_path <tmp>` `-o <out>` (bb.rs:99-111). Content via file, not argv/stdin/env. HARDWARE_CONCURRENCY env maybe set.
5. Proof read from `<out>/proof` (bb.rs:142), 4-byte header prepended.
6. base64 -> `{proof}` response.
7. Cleanup: TempDir RAII-drop at end of bb::prove; kill_on_drop(true); 300s timeout. **No zeroization** of witness in memory or on disk.
- Error leak control: bb stderr logged server-side; only generic `bb prove failed (exit N)` to client (bb.rs:135-139).

## Crypto inventory
- **TLS cert gen** (certs.rs): rcgen 0.13 (pem,x509-parser,zeroize). CA+leaf ECDSA P-256 (certs.rs:143-145). **Keyless-CA**: CA priv key in-memory, signs leaf, dropped, never written (certs.rs:151). Only ca.pem/localhost.pem/localhost.key written. NameConstraints 127.0.0.1/::1/localhost. Files 0o600, dir 0o700. Legacy ca.key deletion fail-closed (certs.rs:189). rustls via tokio-rustls 0.26. macOS trust via `security` subprocess (certs.rs:363-411). Rotation stage->trust->verify->swap.
- **Update sig** (updater.rs): tauri-plugin-updater 2 (minisign/Ed25519). Pubkey in tauri.conf.json:18; endpoint https://aztec-accelerator.dev/releases/latest.json. Sig check inside download()/install(). **size_from_feed cap 500MB is feed-controlled, availability-only** (updater.rs:96-108, SEC-03).
- **Download digest** (versions): sha2 0.11. Expected digest from GitHub API asset `digest` (release_metadata.rs:83). verify_digest fail-closed (downloader.rs:157). **SEC-02: digest+binary share one GitHub trust plane** — no upstream publisher sig on bb.
- request_id = uuid v4.

## Filesystem sinks
- config `~/.aztec-accelerator/config.json` 0o600, dir 0o700 (config.rs).
- certs dir 0o700, ca/localhost pem+key 0o600 (certs.rs).
- bb cache `~/.aztec-accelerator/versions/<v>/bb` 0o755, atomic install.
- **temp witness/proof: OS-temp ivc-inputs.msgpack + output/proof, no explicit perms, RAII-clean (bb.rs:86-90)**.
- logs dir 0o700, daily rotate, max 7.
- crash-recovery: macOS plist, Linux systemd unit, Windows schtasks XML via temp file.

## Subprocess
- **bb prove** (bb.rs:98-121): tokio Command, args `prove --scheme chonk --ivc_inputs_path <tmp> -o <out>`, HARDWARE_CONCURRENCY env, no shell, kill_on_drop, 300s. find_bb order: BB_BINARY_PATH -> cache -> sidecar -> ~/.bb/bb -> which (Unix only; Windows skips PATH to avoid hijack).
- macOS: `xattr -cr`, `codesign --force --sign -` on downloaded bb (downloader.rs:71-98).
- macOS cert trust: `security add-trusted-cert/verify-cert/find-certificate/delete-certificate`.
- Linux crash recovery: `systemctl --user`, ExecStart from current_exe().
- Windows crash recovery: System32\schtasks, exe path XML-escaped.

## Notable anchors (surface, not verdicts)
- witness temp file no explicit 0o600 (bb.rs:90) vs config/certs 0o600.
- absent-Origin auto-approve (auth.rs:33).
- headless localhost auto-approve (server/main.rs:78).
- download digest+binary one trust plane (SEC-02).
- update size cap feed-controlled (SEC-03).
- `withGlobalTauri: true`, no explicit CSP in tauri.conf.json.
