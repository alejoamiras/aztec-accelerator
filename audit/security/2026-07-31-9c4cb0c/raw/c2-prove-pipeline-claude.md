# c2-prove-pipeline — Claude (Opus) raw findings

Two findings meet the bar; both are witness-confidentiality issues.

## F-C2-1 — `bb` child stdio is inherited, so the stderr truncation/containment control is dead code

**Impact factors.** Confidentiality (sensitive-data egress to an uncontrolled sink) + integrity of a
security control that provably never runs. Blast radius: every `/prove`, every platform. Data
sensitivity high — the code's own comment says this stream may carry "file paths, witness data".
Vector: local to collect; trigger is any authorized origin driving `bb` into an error state.
Complexity low. Privileges: none to trigger; local read to collect. UI: none beyond one-time approval.

**Confidence.** High on mechanism (tokio source read). Moderate on realized disclosure (destination
varies by launch mode).

**CWE.** CWE-532, CWE-209, CWE-1188. OWASP A09:2021 / A04:2021.

**Trace.**
1. `core/src/server/prove.rs:246` — witness buffered: `read_body(raw_body, MAX_BODY_SIZE, BODY_READ_TIMEOUT)`.
2. `core/src/server/prove.rs:329` — `bb::prove(&body, version_for_prove.as_ref(), threads)`.
3. `core/src/bb.rs:198` — `write_witness(&input_path, ivc_inputs)`; `:207-219` passes `--ivc_inputs_path`.
4. `core/src/bb.rs:206` — `tokio::process::Command::new(&bb_path)` — **no `.stdout()`/`.stderr()` set**
   anywhere in `:206-228` (only args, env, `kill_on_drop`).
5. `core/src/bb.rs:229` — `cmd.spawn()` → tokio `src/process/mod.rs:863-867` → `std::process::Command::spawn`,
   which defaults all three stdio to `Stdio::inherit()`.
6. `core/src/bb.rs:230` — `child.wait_with_output()` → tokio `src/process/mod.rs:1457-1463`: with inherited
   stdio `self.stderr` is `None`, so `Output.stderr` is **unconditionally empty**.
7. Dead sink: `core/src/bb.rs:238-241` `if !stderr.is_empty() { warn!("bb stderr:\n{}", truncate_stderr(..)) }`
   — guard always false; `truncate_stderr` (`:272-280`) unreachable from production.
8. Real sink: the child writes to inherited fd 1/2. App tracing is split at `src-tauri/src/main.rs:534-537`
   into a stdout layer + a `tracing_appender` file layer rooted at the 0700 log dir (`:513-521`); bb's raw
   output reaches **neither** the rolling log file nor the truncation.

**Missing control.** No `.stderr(Stdio::piped())` (capture+truncate) and no `Stdio::null()` (explicit
discard). Insecure-by-omission default. Also no test asserts the warn line is emitted — the three tests
at `bb.rs:331-352` exercise `truncate_stderr` as a pure function, giving false assurance.

**Exploit story.** Approved origin (or absent-Origin caller) POSTs semantically malformed IVC input →
`bb prove` fails and writes diagnostics (per `bb.rs:244-245`, possibly workspace path + witness-derived
material) to fd 2 → appended verbatim, untruncated, to the inherited stderr. On a Linux desktop autostart
(`.desktop` `Exec=`) that is the session stderr (systemd --user journal / `~/.xsession-errors`), outside
the 0700 log dir and outside the 7-day/7-file rotation (`main.rs:522-528`). Unbounded repetition; no rate
limit on this path.

**Why mitigations fail.** Truncation unreachable. The generic-HTTP-error mitigation (`bb.rs:243-248` →
`server.rs:438`) DOES hold — only the server-side containment half is broken. The 0700 log-dir hardening
never sees this data. Honest counterweight: macOS LaunchAgent sets no `StandardErrorPath`
(`autostart.rs:245-252`) so launchd routes to `/dev/null`; a Windows GUI-subsystem build has no console.
Realized leak is Linux-desktop + terminal/dev + CI — hence moderate impact, but the dead control is
high-confidence and platform-independent.

**Instances.** Root cause `core/src/bb.rs:206-229`; dead code `core/src/bb.rs:238-241`, `:272-280`.

## F-C2-2 — Plaintext witness deleted only by RAII; the app's own exit paths skip it, and there is no startup reaper

**Impact factors.** Confidentiality — sensitive data at rest retained beyond its intended lifetime. Blast
radius: one full private witness per interrupted prove, accumulating for the life of the install; never
cleaned/rotated; survives reboots and updates. Sensitivity maximum (on a privacy chain the IVC witness is
the plaintext of everything the chain hides). Vector local/offline (disk image, backup). Complexity low —
predictable path. Privileges: same-user read or offline volume access. Trigger is a routine user action.

**Confidence.** High — deletion is exclusively RAII, exits are `process::exit`-family, and a repo-wide grep
for `prove-tmp`/`prove_tmp` finds only `bb.rs` and one `win_acl.rs` doc comment: no reaper exists.

**CWE.** CWE-459, CWE-226, CWE-212. OWASP A02:2021 / A04:2021.

**Trace.**
1. `core/src/server/prove.rs:246` witness in memory → `:329` to `bb::prove`.
2. `core/src/bb.rs:194` `create_prove_tempdir()` → `:120-153` → `:82-113` `prove_tmp_parent()`, a
   **persistent** dir at `dirs::data_local_dir()/aztec-accelerator/prove-tmp`.
3. `core/src/bb.rs:195,198` — plaintext witness written to `<tmp>/ivc-inputs.msgpack`.
4. Only deletion is `TempDir::drop` when `bb::prove` returns (`:255`) or unwinds. No explicit cleanup.
5. Exposure window up to `PROVE_TIMEOUT` = 300 s (`core/src/bb.rs:7`).
6. Drop-skipping exits in shipped code: tray Quit → `app.exit(0)` (`src-tauri/src/main.rs:396`);
   `app_handle.exit(0)` (`main.rs:301`); `app.restart()` after auto-update install (`updater.rs:542`);
   `window.app_handle().restart()` (`commands.rs:666`); Windows `install()` → `std::process::exit(0)`
   (documented `update_marker.rs:3`). These terminate without unwinding the tokio worker stack holding
   `tmp_dir`, so `TempDir::drop` never runs.
7. No graceful drain: grep for `graceful`/`with_graceful_shutdown`/`shutdown` across `core/src/server.rs`,
   `main.rs`, `src-tauri/src/server.rs` returns nothing — in-flight proves are not awaited before exit.
8. No reaper: nothing enumerates/deletes stale `prove-*` dirs at startup. Contrast: the version cache DOES
   have eviction (`versions/version_policy.rs:303`, `versions/downloader.rs:252,301,308`) — the omission is
   specific to the witness workspace.

**Missing control.** Any one of: a startup sweep of `prove_tmp_parent()`; an explicit `tmp_dir.close()` /
best-effort `remove_dir_all` plus a shutdown hook draining `prove_semaphore` before exit/restart; or an
updater gate refusing to restart while a prove permit is held (`updater.rs:530-542` performs no such check).

**Exploit story (accidental exposure, in scope).** User proves a shielded transfer; witness at
`~/.local/share/aztec-accelerator/prove-tmp/prove-XXXXXX/ivc-inputs.msgpack` (0600). Mid-proof the user
clicks tray Quit (or the default-on auto-updater restarts, or OS logout/OOM — `crash_recovery.rs` exists
precisely because abrupt death is anticipated). `process::exit` → no unwind → file remains, forever. Over
months a directory of plaintext witnesses accumulates, harvestable by same-user code execution (the
supply-chain threat this repo hardens against elsewhere), a stolen/decommissioned laptop without FDE, or a
user-scoped backup restore (Time Machine covers `~/Library/Application Support`).

**Why mitigations fail.** 0700/0600 modes (`bb.rs:86-96,126,158-178`) and Windows PROTECTED DACLs
(`win_acl.rs:306-364`) are **spatial** controls (other local users); they give zero **temporal** control.
`create_new(true)`/`CREATE_NEW` defends creation, not deletion. `kill_on_drop(true)` (`bb.rs:228`) covers
the child, not the workspace, and is equally bypassed by `process::exit`. The prove-path `StatusGuard`
(`prove.rs:23-35`) shows the authors reasoned about Drop-based cleanup — but its enumeration stops at
*panic*, and both it and `TempDir` are defeated by `process::exit`, the app's own normal quit mechanism.
Secondary (not a separate finding): the witness also lives in a plain heap `Bytes` (`prove.rs:246→329`)
with no zeroization/mlock, so it is swap- and core-dump-reachable — noted as blast radius only.

**Instances.** `core/src/bb.rs:194-198` (+ returns `:234,:247,:255`); dir definition `:82-113`;
drop-skipping exits `main.rs:301,396`, `updater.rs:542`, `commands.rs:666`, Windows NSIS handoff
(`update_marker.rs:3`); missing reaper = repo-wide absence.

## Considered and CLEARED (do not re-cover)

- **Resource controls sound.** `reject_declared_oversize` (`prove.rs:144-186`) handles comma-lists, rejects
  non-`1*DIGIT`, parses `u64` (no 32-bit wrap), enforces RFC 7230 §3.3.2 agreement. `axum::body::to_bytes`
  bounds during accumulation, so the 50 MB cap applies BEFORE full buffering; peak resident bounded at
  8 × 50 MB. Body read (`:246`) and version download (`:275`) both run OUTSIDE the single prove permit,
  taken only at `:321-326`. `try_enter` sheds 429 rather than queueing.
- **Argument injection: not present.** Paths derive from a `tempfile` random name under a fixed root; no
  shell; `x-aztec-version` is parsed into a validated `AztecVersion` (`prove.rs:67`) before reaching a path.
- **Error-message path disclosure: does not occur.** Every `Err` from `bb::prove` → `ProveError::ProveFailed`
  (`prove.rs:350` → `server.rs:438`); `cache_layout.rs:150-219` formats only version strings and bare
  `io::Error` displays (std does not append paths).
- **`BB_BINARY_PATH` precedence over the version pin** (`bb.rs:28-33` before `:37-40`): real but is the
  documented trusted-operator override; setting it already implies owning the process env.
- **Verify-then-exec TOCTOU** (`bb.rs:38` → `:206`): window real, but only a same-user process can win it,
  and that actor can already read the witness directly. No privilege gain.
- **Windows ACLs correctly ordered — no create-then-harden data window.** `secure_create_file`
  (`win_acl.rs:337-364`) applies+verifies the DACL on the EMPTY file before `write_all` (`bb.rs:164-165`);
  `create_prove_tempdir` (`bb.rs:139-140`) creates inside an already-owner-only parent. Fails closed:
  `verify_owner_only` (`:229-302`) rejects null/empty DACLs, foreign and non-ALLOWED ACEs, insufficient
  masks; `verify_owner_sid` (`:195-225`) closes the foreign-owner `WRITE_DAC` bypass; `reject_if_reparse`
  (`:78-90`) closes the create→open junction swap; `prove_tmp_parent` returns `None` rather than falling
  back to `%TEMP%` (`bb.rs:130-138`). FFI hygiene correct.
- **Unix modes applied at the creation syscall** — verified `tempfile-3.27.0 src/dir/imp/unix.rs` uses
  `DirBuilder::mode()`; no create-then-chmod window, so the `bb.rs:117` comment is accurate.
- **`cleanup_old_versions` can evict a version another concurrent prove is using** (`prove.rs:287-299`
  exempts only the just-downloaded version, but up to 8 proves on different versions can be in flight) —
  impact is a spurious prove failure ⇒ routed to `/harden bugs`, not security.
