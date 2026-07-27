# Phase 1 — C8 autostart `enable() → Result` rollback

**Status: ✓ green** (fmt --check · clippy -D warnings · 37/37 src-tauri tests, incl. 5 new `enable_transaction` cases).

## What changed
- `CrashRecovery::enable(&self)` → `Result<(), String>`; `enable_crash_recovery()` → `Result<(), String>`.
- All three `enable_impl` bodies now return `Result` with the per-exit classification below.
- New `enable_transaction(prior_enabled, plugin_enable, crash_arm, plugin_disable, crash_disarm)` — the
  full-transaction rollback helper (D20), generic over injected closures → unit-testable on Linux without a
  real `AppHandle`.
- `commands.rs::set_autostart` uses it: snapshots `manager.is_enabled()`, then runs the transaction; on any
  failure rolls back failure-observably and returns a combined error.
- Log-and-continue callers updated: `main.rs` startup rearm + `updater.rs` post-update rearm now
  `if let Err(e) … tracing::warn!` (never abort, never silently swallow) (D12).

## Per-exit classification (v3 codex-final cond. 3 / D16) — EVERY early return

### macOS `enable_impl`
| Exit | Result | Why |
|---|---|---|
| `read_to_string(plist)` fails | **Err** | plugin's LaunchAgent plist absent where expected ⇒ arming did not happen |
| plist already contains `<key>KeepAlive</key>` | **Ok** | idempotent already-armed — SUCCESS, must NOT trigger rollback |
| `patch_plist_with_keepalive` → `None` (no `</dict>`) | **Err** | cannot insert the KeepAlive block ⇒ arming impossible |
| `write(patched)` fails | **Err** | arming write failed |
| write succeeds | **Ok** | armed |

### Linux `enable_impl`
| Exit | Result | Why |
|---|---|---|
| `current_exe()` fails | **Err** | can't build ExecStart |
| `config_dir()` → `None` | **Err** | can't locate `systemd/user` |
| `create_dir_all(service_dir)` fails | **Err** | can't place the unit |
| `systemd_exec_start()` → `None` (unsafe path, F-010) | **Err** (after `disable_impl()`) | fail-closed: removes any stale unit, reports failure |
| `write(service_path)` fails | **Err** | unit write failed |
| `systemctl --user daemon-reload` result | *ignored* | best-effort refresh, NOT the arming step |
| `systemctl --user enable` exit 0 | **Ok** | armed |
| `systemctl --user enable` non-zero | **Err** | THIS is the actual arming step — a failure is a real failure |
| `systemctl` fails to run | **Err** | arming step didn't execute |

### Windows `enable_impl`
| Exit | Result | Why |
|---|---|---|
| `current_exe()` fails | **Err** | can't build the task XML `<Command>` |
| `tempfile::Builder…tempfile()` fails | **Err** | can't stage the XML |
| `write_all(bytes)` fails | **Err** | XML not written |
| `flush()` fails | **Err** | XML possibly truncated |
| `schtasks /Create` exit 0 | **Ok** | task registered |
| `schtasks /Create` non-zero | **Err** | registration (arming) failed |
| `schtasks` fails to run | **Err** | arming step didn't execute |

## Rollback semantics (D20) — verified by the 5 injected-closure tests
- **happy path** → no rollback closure called.
- **crash_arm fails, prior disabled** → `plugin_disable` + `crash_disarm` both run; error names both.
- **crash_arm fails, prior ENABLED** → `plugin_disable` NOT called (restore prior state — don't clobber a
  pre-existing-enabled launcher); error notes "kept".
- **plugin_enable fails** → `crash_arm` never runs; no `plugin_disable`; `crash_disarm` runs defensively.
- **rollback `disable` ALSO fails** → surfaced ("autostart may still be active"), disarm-not-confirmed
  surfaced — no short-circuit; both cleanup ops attempted.

## Notes
- `is_enabled()` for the prior-state snapshot comes from `tauri_plugin_autostart::ManagerExt` (already
  imported in `set_autostart`). The two forward/rollback closures capture `&manager` immutably (both plugin
  ops take `&self`) — no borrow conflict.
- Local build setup needed before `cargo`: `bun run --cwd packages/accelerator frontend:build` (F-012
  build.rs guard) + `prebuild` (bb sidecar). CI's setup-accelerator does both.
