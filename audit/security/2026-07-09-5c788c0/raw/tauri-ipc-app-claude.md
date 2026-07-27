# Cluster: tauri-ipc-app — security audit

Scope: `packages/accelerator/src-tauri/src/{main,lib,commands,server,tray,windows,crash_recovery}.rs`
(+ direct deps peeked for reachability: `core/src/authorization.rs`, `core/src/server/auth.rs`,
`src-tauri/capabilities/default.json`, `src-tauri/frontend/{authorize,settings,update-prompt}.html`,
`src-tauri/frontend/tauri-bridge.js`).

## Reachability analysis (requested explicitly — reported even though it resolves as a non-finding)

**Question:** can a compromised/injected page reach `respond_auth` / `remove_approved_origin` /
`set_auto_update` / `enable_safari_support` from the localhost web server or a webview page, and does
`respond_auth` authenticate its caller?

**Finding: not reachable, and no — but the gaps that would matter are closed by other layers, not by
`respond_auth` itself.**

- The Tauri `invoke_handler` (main.rs:447-460) and the HTTP accelerator server
  (`server.rs` → `accelerator_core::server`) are two independent channels. The localhost HTTP server on
  :59833/:59834 exposes `/prove`, `/health`, etc. via axum — it has no code path into
  `tauri::generate_handler!`. A malicious dApp running in the user's ordinary browser (the actual
  "attacker web page" in this app's threat model) can only reach the HTTP server, never
  `window.__TAURI__`/IPC — that bridge is injected solely into webviews Tauri itself creates, which in
  this app load only bundled local assets (`authorize.html`, `settings.html`, `update-prompt.html` via
  `WebviewUrl::App`, windows.rs:44, 63, 97, 141). No window loads a remote or attacker-influenced URL,
  and none of the three pages contain `<iframe>`/`window.open` navigation to attacker content.
- `capabilities/default.json` has no `windows`/`webviews` restriction and no `remote` block, so in
  principle every window the app creates shares one broad permission set (all 11 commands, including
  `respond_auth`, `remove_approved_origin`, `set_auto_update`, `enable_safari_support`) — there is no
  per-window least-privilege split between `settings`, `auth-*`, and `update-prompt`. This is a real
  defense-in-depth gap, but I could not find a concrete initial-foothold (XSS/DOM injection) in any of
  the three bundled pages to actually exercise it: `authorize.html` writes `origin` via `textContent`
  (frontend/authorize.html:38), `settings.html` renders the approved-origins list via
  `document.createElement`/`textContent` (frontend/settings.html:134-151), and `update-prompt.html`
  also uses `textContent` (frontend/update-prompt.html:28-29). Per the audit's rules this stays a
  **non-finding** (theoretical, no concrete bypass) — noted here only because the task asked for the
  reachability reasoning explicitly.
- `commands::respond_auth` (commands.rs:111-139) indeed does **not** authenticate its caller: it takes
  no `tauri::Window`/label parameter and never checks that the invoking window is the specific
  `auth-{sha256(request_id)[..6]}` popup for that request. It trusts `request_id` alone (the `origin`
  argument is decorative — used only for a debug log at commands.rs:134, not for resolving the
  decision). What actually protects this today is upstream, in `core/src/authorization.rs`:
  `request_id` is a UUIDv4 (128 bits, authorization.rs:253) minted server-side and disclosed **only** to
  the one popup window it belongs to, via that window's own `window.location.search`
  (windows.rs:89-93). No command discloses pending `request_id`s to other windows, so even though
  `respond_auth` would happily "resolve" any `request_id` string handed to it by *any* window, nothing
  in this app's current surface hands another window a live one to guess or read. Net effect: the
  authentication is missing at the command layer but the value it would forge is unobtainable through
  any other reachable path — so this does not currently rise to a certified finding either (matches
  SEC-06's stated model: wrong/guessed ids are harmless no-ops).

No certified finding follows from the above two paragraphs — they're recorded for completeness per the
task's explicit ask, not as vulnerabilities.

## Certified findings

### Finding 1: Linux crash-recovery systemd unit file — unescaped exe path allows directive injection (`ExecStartPre=`) if the binary's path is attacker-influenced

1. **Title**: Unescaped `current_exe()` path interpolated into the systemd user-unit file lets an
   embedded newline inject arbitrary `[Service]` directives (e.g. `ExecStartPre=`), unlike the sibling
   Windows Task Scheduler path which is properly XML-escaped in the same file.

2. **Impact factors**: Integrity + Availability violated (arbitrary additional systemd directive
   execution as the victim user at every service start/crash-restart); blast radius: one user (the
   account whose autostart/crash-recovery is enabled) — not all users, not the system, since the
   systemd unit is a `--user` unit. Data sensitivity: none directly leaked (this is code-execution
   persistence, not a data leak). Exploitability: attack vector **Local**; attack complexity **High**
   (attacker needs to control a path segment of the running binary's location, a narrow precondition);
   privileges required **Low** (an unprivileged local account, or write access to a shared staging
   path, is enough — no privilege escalation needed to *plant* the directory name); user interaction
   **Required** (the victim must run the accelerator from that path and toggle "Start on Login").

3. **Evidence confidence**: High on the code defect itself (verified: no escaping function is called
   on `exe` before interpolation, confirmed by contrast with the Windows sibling in the same file which
   *does* escape). Low-moderate on real-world exploitability given the unusual precondition (see below).

4. **OWASP category**: A03:2021 – Injection. **CWE**: CWE-93 (Improper Neutralization of CRLF
   Sequences / CRLF Injection into a config file), secondarily CWE-88 (Argument Injection, since the
   injected `ExecStartPre=` line is itself a new argument-bearing directive systemd will execute).

5. **Trace** (`packages/accelerator/src-tauri/src/crash_recovery.rs`):
   - Source: `std::env::current_exe()` — crash_recovery.rs:134 (`let exe = match std::env::current_exe()`),
     bound at crash_recovery.rs:135 (`Ok(p) => p`). This path is attacker-influenceable only insofar as
     the attacker controls where the accelerator binary physically resides/is launched from.
   - Sink 1 (format, no escaping): crash_recovery.rs:156-171, specifically line 163
     `ExecStart=\"{exe}\"\n\` with `exe = exe.display()` at line 170 — the path is placed verbatim
     inside a Rust `format!` with no newline/quote sanitization.
   - Sink 2 (write to disk): crash_recovery.rs:173 `std::fs::write(&service_path, &service_content)`,
     writing to `~/.config/systemd/user/aztec-accelerator.service` (service_path built at
     crash_recovery.rs:155 from `service_dir` at crash_recovery.rs:142-148).
   - Sink 3 (load/execute): crash_recovery.rs:179-184, `systemctl --user daemon-reload` then
     `systemctl --user enable aztec-accelerator` — this is what makes systemd parse the poisoned unit
     file and, on next unit start, execute any injected `ExecStartPre=`/similar directive.
   - Caller path into `enable_impl()`: `commands::set_autostart(enabled: true)` (commands.rs:45-57) →
     `crate::crash_recovery::enable_crash_recovery()` (crash_recovery.rs:22-24) → platform dispatch
     (crash_recovery.rs:37-44) → Linux `enable_impl()` (crash_recovery.rs:133).

6. **Missing control**: no neutralization/escaping of `exe.display()` before interpolating it into the
   INI-style unit-file content — contrast with the Windows counterpart in the very same file,
   `task_xml()` (crash_recovery.rs:352-391), which explicitly calls `xml_escape(exe_path)`
   (crash_recovery.rs:354) and has a regression test proving escaping holds
   (crash_recovery.rs:406-432, `task_xml_uses_repeating_trigger_and_escapes_exe`). No equivalent
   `systemd_escape`/newline-stripping exists for the Linux sink, and there is no unit test covering a
   path containing a newline or `"` for the Linux branch (the macOS/Windows tests exist; there is no
   Linux-specific `enable_impl`/unit-content test at all).

7. **Exploit/violation scenario**:
   1. On a machine where a different local, unprivileged account (or anyone with write access to a
      shared staging/extraction directory such as a shared `/tmp` subtree) can create directories, the
      attacker creates one whose name contains a literal newline byte followed by an injected
      directive, e.g. `innocuous-folder\nExecStartPre=/tmp/evil.sh\n#` (POSIX filenames permit any byte
      except `/` and NUL, so this is a legal directory name).
   2. The victim is led to extract/copy/run the Aztec Accelerator binary from inside that directory
      (e.g., "unzip the release into the shared build folder and run it from there").
   3. The victim opens Settings and enables "Start on Login" (`set_autostart(true)`,
      commands.rs:45-57), which calls `enable_crash_recovery()` → Linux `enable_impl()`.
   4. `current_exe()` (crash_recovery.rs:134) returns the path containing the embedded newline +
      `ExecStartPre=` directive; `format!` (crash_recovery.rs:156-171) embeds it verbatim; the unit
      file at `~/.config/systemd/user/aztec-accelerator.service` now contains, under `[Service]`, an
      extra line `ExecStartPre=/tmp/evil.sh`.
   5. `systemctl --user daemon-reload && systemctl --user enable aztec-accelerator`
      (crash_recovery.rs:179-184) loads the poisoned unit. On the next start of the unit (next login,
      or the very crash-restart this feature exists to trigger), systemd runs `/tmp/evil.sh` as the
      victim user before launching the accelerator — arbitrary code execution + persistence across
      reboots, entirely inside the crash-recovery mechanism meant only to relaunch the app.

8. **Preconditions**: (a) attacker can create a directory/file-path segment with an embedded newline
   somewhere the victim will later run the accelerator binary from (a distinct-user or shared-path
   scenario — not simply "the user names their own folder oddly", since that's self-inflicted and not
   a trust-boundary crossing); (b) the victim enables autostart via Settings. No network component —
   this is Linux desktop-only (`#[cfg(target_os = "linux")]`), and only the crash-recovery/autostart
   path, not the core proving/witness path.

9. **Why existing mitigations fail**: none of the documented SEC-0N mitigations target this file — the
   nearest related control is the app's OWN Windows-side fix for the identical bug class
   (`xml_escape`, crash_recovery.rs:394-400, tested at crash_recovery.rs:406-432), proving the team
   already recognizes "exe path may contain characters meaningful to the artifact format" as a real
   risk; it was simply not carried over to the Linux/systemd sink. There is no other guard (no path
   validation, no `current_exe()` canonicalization/newline check) anywhere upstream of `enable_impl()`.

10. **Instances**: single root cause, one location — crash_recovery.rs:156-171 (format) feeding
    crash_recovery.rs:173 (write) feeding crash_recovery.rs:179-184 (load). No other unescaped
    interpolation of external/path data was found in this cluster's files (macOS `enable_impl`,
    crash_recovery.rs:61-90, only inserts a fixed constant string into an existing plist and does not
    interpolate the exe path at all; Windows `task_xml`, crash_recovery.rs:352-391, does escape).

## Explicitly investigated, not flagged (for completeness)

- **HTTPS launch gate** (`main.rs` `classify_launch_https`/`try_start_https`): matches its own unit
  tests (main.rs:583-625); the documented reset-vs-skip asymmetry (missing certs → reset;
  untrusted-but-present → skip without reset) is intentional and tested, no bypass found.
- **Window label / URL construction** (`windows.rs`): `origin`, `request_id`, `current`, `version` are
  all `urlencoding::encode`-d before being placed in a window URL (windows.rs:90-93, 136-139); window
  labels are derived via `sanitize_window_label` (commands.rs:145-149, SHA-256[..6] hex) not raw
  concatenation — no URL/label injection found.
- **`AuthorizationManager` request/resolve** (`core/src/authorization.rs`): resolution is keyed by an
  unguessable UUIDv4 `request_id`, not by origin (SEC-06); a wrong/tampered id is a verified no-op
  (authorization.rs:397-408, `resolve_ignores_wrong_request_id`). Piggybacked concurrent requests from
  the same origin correctly share one id; different origins never share one. No cross-origin
  resolution bug found.
- **Windows crash-recovery temp file** (`crash_recovery.rs:274-303`): random `tempfile::Builder` name
  (not a predictable `%TEMP%` path), closed before `schtasks` reads it — no TOCTOU/predictable-path
  planting vector found for the XML itself (and it carries no sensitive data — just the exe path).
- **Persisted crash-recovery artifacts contain no sensitive data**: the macOS plist, systemd unit, and
  Windows scheduled-task XML each carry only the app's own executable path/description — no witness
  bytes, proofs, approved-origin lists, or other user data are ever written into these files.
- **`respond_update_prompt`** (commands.rs:224-274): cannot be used to forge/install an arbitrary
  update — it only ever acts on the `Update` object already fetched and stored by the (out-of-cluster)
  updater module; no update payload is constructable from IPC args.
