# Recon: B3 — bb worker containment surface

Agent: sonnet Explore, 2026-08-16, tree = worktree-v2-release-train @ 0c351bc.

## 1. bb.rs as it is NOW

- Spawn: `prove()` (bb.rs:268-353) → `cmd.kill_on_drop(true)` (:325) → `spawn_capturing_stderr` (:384-389)
  pipes **stderr only**; stdout deliberately inherited (doc :377). F-08b made stderr captured at all.
- Stderr bound: **none while streaming** — `child.wait_with_output()` (:327) drains piped stderr into an
  unbounded `Vec<u8>`; `truncate_stderr` (:395-403) is display-only truncation (500 chars) AFTER full
  collection. This is exactly the B3 gap.
- `PROVE_TIMEOUT = Duration::from_secs(300)` (bb.rs:7); hardcoded const, not injectable (contrast
  `bind_with_retry_inner`'s externalized Durations, bb.rs:13-26 — the testability pattern to imitate).
- Proof read (:347-352): `std::fs::read(proof_path)` → `prepend_field_count_header` (:357-363,
  `field_count = len/32`). **Zero validation**: 0-byte proof → "successful" 4-byte `[0,0,0,0]` response;
  non-32-aligned silently floor-divided. prove.rs:369-378 base64s + 200 with no inspection.
- Workspace lifecycle: `prove_tmp_parent` (:82-113, 0700/fail-closed-Windows), `create_prove_tempdir`
  (:202-235), `write_witness` (:240-261, 0600 + create_new). Cleanup = TempDir Drop (in-process) +
  F-08a startup reaper `reap_orphaned_prove_workspaces` (:142-147 → :156-195), 24h floor (:133),
  symlink-safe, bind-win-gated from server.rs:269-283 after `bind_with_retry` (bind.rs:13-56).
  → input 04's P2 ("no crash-orphan reaper") is CONFIRMED CLOSED by F-08a.

## 2. Bounded-read patterns to reuse

- `CappedReader` (downloader.rs:392-413): sync `std::io::Read`, running counter, fail-closed error on
  overflow; cap `MAX_DECOMPRESSED_BYTES = 512MiB` (:387); test :519-530.
- Closer ASYNC precedent: `download_tarball`'s manual chunk loop (downloader.rs:118-152) — running
  counter over an async stream, cap `MAX_DOWNLOAD_BYTES = 64MB` (:131).
- Design fork flagged: stderr drain wants **cap-and-continue** (keep draining so bb never blocks on a
  full pipe; stop accumulating past N) vs CappedReader's fail-closed abort. Plan must choose.

## 3. Exit/restart/update paths vs in-flight bb

| Site | Behavior | bb-aware? |
|---|---|---|
| tray quit, main.rs:414-429 | Windows `quit_disarm()` then `app.exit(0)` (:428) | NO |
| `RunEvent::ExitRequested` main.rs:778-784 + should_prevent_exit :263-265 | only blocks window-close (code=None) | NO |
| updater.rs:558-568 `perform_update` | `rearm_now()` then `app.restart()` (:567) | NO (`prove_semaphore` absent from updater.rs) |
| Windows install | plugin `update.install()` → NSIS handoff + internal `exit(0)` (update_marker.rs:1-10 doc) | NO |
| commands.rs:624-667 `renew_cert` | **the one mitigation**: `prove_semaphore.try_acquire()` else refuse (:644) before `restart()` (:666); doc :632-643 explicitly names the orphan-bb/witness hazard | YES (refuse-style) |
| crash_recovery.rs | relaunch-after-death only (launchd/systemd/schtasks) | N/A |

- `kill_on_drop` fires ONLY on in-process future drop (timeout :327, client-disconnect handler drop).
  Not on `app.exit()`, `app.restart()`, `std::process::exit`, external kill.
- **No process groups anywhere**: zero hits for process_group/setsid/CREATE_NEW_PROCESS_GROUP/JobObject.
  bb shares the app's pgid. No Job Object.

## 4. Test shapes

- Injection seam: `BB_BINARY_PATH` env (bb.rs:28-33). No DI seam inside prove().
- Template: `prove_success_path_and_status_sequence` (server/tests.rs:474-539) — fake-bb shell script
  writing `$out/proof`, `EnvGuard` RAII (:487-494), `#[serial]` (default bucket), oneshot through router.
- bb.rs test module :405-690: perms tests, F-08b regression :459-487, truncate :489-510, header
  :512-539, find_bb :541-597, F-08a reaper suite :599-689 (mtime-backdate helper :606-609).
- Tests that skip if real bb on PATH: server/tests.rs:281-307, :1158-1183.
- NOT tested today: PROVE_TIMEOUT expiry, stderr spam, pgroup membership, restart-mid-proof.
- New tests must join the DEFAULT serial bucket (collision comments tests.rs:276-280, :1159-1161);
  named bucket precedent only for windows_system_root (crash_recovery.rs:949).

## 5. Panic story

- `panic = "abort"` ONLY in src-tauri/Cargo.toml:117-121 `[profile.release]`. core/ and server/ crates
  have no profile section — headless server does NOT abort-on-panic today (separate build units,
  deliberately non-workspace).
- No `panic::set_hook` anywhere; only unrelated catch_unwind in leases.rs:256,269.
- Tracing init main.rs:545-570 BEFORE tauri builder; global subscriber → a hook installed after :570
  can `tracing::error!` through both layers.
- **HAZARD**: file layer uses `tracing_appender::non_blocking` (:562) — background worker thread may
  not flush before abort terminates the process. Hook needs a synchronous write path or explicit flush.

## 6. Show Logs

- tray.rs:91-113: `show_logs` MenuItem constructed only in `dev_mode` branch; prod branch omits it
  (also omits versions submenu + status item — status item still exists for tooltip, main.rs:706-709).
- `dev_mode` = `cfg!(debug_assertions)` (main.rs:29-31) — compile-time.
- Click handler NOT gated (main.rs:430 `"show_logs" => open_in_browser(&log_dir())`) — enabling in
  prod is menu-construction-only. `log_dir()` = core/src/lib.rs:27-32; Windows path undocumented.

## 7. Conventions + collisions

- New rejection for empty/malformed proof: either new `ProveError` variant (server.rs:462-493) — wire
  contract pinned by `prove_error_responses_stay_text_plain_json_string` (tests.rs:376-463, text/plain
  JSON-shaped, NOT axum::Json) — or inside bb::prove via existing Box<dyn Error> → ProveFailed. Fork
  for the plan.
- Tracing style: structured fields first; `"SECURITY: ..."` literal prefix for security-relevant lines
  (updater.rs convention).
- RAII-guard idiom for every-exit-path work: EnvGuard, StatusGuard (prove.rs:25-35), BindOwnedGuard
  (server.rs:301-314), CrashRecoveryGuard (updater.rs:684-722 with defuse()/rearm_now() for
  before-no-return-call sites). A bb-child guard should take this shape.
- Must subsume or explicitly coexist with renew_cert's try_acquire-refuse pattern — divergent logic
  would be a regression.
- updater.rs restart is inside a lock-heavy transaction with documented "drop lock BEFORE install()"
  invariant (:554) and explicit-before-no-return-call pattern (:672-682) — bb-kill there must be
  explicit, not Drop-based.
- Windows Job Object: `windows-sys` 0.61 in core lacks `Win32_System_JobObjects` feature (add it);
  src-tauri has windows-sys only as dev-dep → Job Object code belongs in core. tokio 1.52.3 has
  `Command::process_group()` on Unix — no bump needed.
