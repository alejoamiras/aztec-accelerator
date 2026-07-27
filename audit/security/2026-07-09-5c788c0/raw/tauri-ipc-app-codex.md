1. **Global Tauri IPC commands are not scoped to their intended window**

   1. **Title:** Any compromised app webview can invoke settings/update/auth IPC commands.

   2. **Impact factors:** Violates Authorization, Integrity, and Confidentiality. Blast radius: one desktop user. Data sensitivity: approved origin list and trust/update/autostart preferences; for `respond_auth`, authorization to submit private witnesses from an origin. Exploitability: local app-webview script execution or compromised bundled frontend asset; attack complexity low once script execution exists; privileges none inside the webview; user interaction required only to open the affected window or trigger auth/update flow.

   3. **Evidence confidence:** High.

   4. **OWASP + CWE:** OWASP A01 Broken Access Control; CWE-862 Missing Authorization. `get_config` also exposes CWE-200 sensitive information disclosure.

   5. **Trace:**  
      `packages/accelerator/src-tauri/src/main.rs:447`-`460` registers all commands in one global `invoke_handler`.  
      `packages/accelerator/src-tauri/src/windows.rs:43`-`50` creates app webviews without per-window command scoping. Settings, auth, and update windows are created at `windows.rs:58`-`70`, `windows.rs:87`-`104`, and `windows.rs:135`-`152`.  
      Sinks:
      `commands.rs:34`-`36` returns full config; config contains `approved_origins` at `packages/accelerator/core/src/config.rs:42`-`61`.  
      `commands.rs:45`-`55` toggles autostart and crash recovery.  
      `commands.rs:67`-`75` removes approved origins.  
      `commands.rs:111`-`128` resolves auth decisions by `request_id`.  
      `commands.rs:155`-`184` enables Safari HTTPS/trust flow on macOS.  
      `commands.rs:191`-`210` disables Safari support.  
      `commands.rs:213`-`218` persists auto-update preference.  
      `commands.rs:225`-`273` handles update-prompt action and can dismiss or start a pending update.  
      For auth specifically, the server creates a UUID request id at `packages/accelerator/core/src/authorization.rs:253`, calls the popup at `packages/accelerator/core/src/server/auth.rs:63`-`72`, and the auth window URL carries that id at `windows.rs:89`-`93`; `respond_auth` then resolves it at `commands.rs:120`-`128`, and `remember=true` persists the origin at `packages/accelerator/core/src/server/auth.rs:84`-`99`.

   6. **Missing control:** No command checks the caller window label, expected page, per-window nonce, or user-confirmed session. `respond_auth` validates only possession of `request_id`; settings/update commands require no caller-specific capability at all.

   7. **Exploit/violation scenario:**  
      1. A malicious origin triggers `/prove`, causing the desktop app to open `authorize.html?...&requestId=<uuid>`.  
      2. Attacker-controlled script executing in that auth webview reads the URL `requestId`.  
      3. It invokes `respond_auth({ requestId, origin, allowed: true, remember: true })`.  
      4. The origin is approved and optionally persisted without a legitimate user click. Future private witness submissions from that origin pass authorization.  
      5. The same compromised webview can call `get_config` to read approved origins, `set_auto_update(false)` to silence automatic updates, `respond_update_prompt("later")` to dismiss a pending update, or `set_autostart` / Safari-support commands to manipulate local trust and persistence settings.

   8. **Preconditions:** Attacker needs script execution in any Tauri app webview. For auth self-approval, the script must execute in the auth popup or otherwise learn the unguessable `request_id`. A normal external browser page cannot directly invoke Tauri IPC, and guessing the UUIDv4 request id is not a viable path.

   9. **Why existing mitigations fail:** Host-header allowlisting and Origin authorization protect the localhost HTTP proving API, not Tauri IPC. SEC-06’s opaque UUID prevents resolving auth with only an origin string, but the auth popup is intentionally given the UUID in its URL, and `respond_auth` does not verify the invoking window or a user gesture. The update signature/digest controls do not protect the local preference commands that can dismiss prompts or disable auto-update.

   10. **Instances:**  
       `packages/accelerator/src-tauri/src/main.rs:447`-`460`;  
       `packages/accelerator/src-tauri/src/commands.rs:34`-`36`;  
       `packages/accelerator/src-tauri/src/commands.rs:45`-`55`;  
       `packages/accelerator/src-tauri/src/commands.rs:67`-`75`;  
       `packages/accelerator/src-tauri/src/commands.rs:111`-`139`;  
       `packages/accelerator/src-tauri/src/commands.rs:155`-`184`;  
       `packages/accelerator/src-tauri/src/commands.rs:191`-`210`;  
       `packages/accelerator/src-tauri/src/commands.rs:213`-`218`;  
       `packages/accelerator/src-tauri/src/commands.rs:225`-`273`;  
       `packages/accelerator/src-tauri/src/windows.rs:43`-`50`, `58`-`70`, `87`-`104`, `135`-`152`.

2. **Linux crash-recovery systemd unit allows executable-path unit-file injection**

   1. **Title:** Unescaped `current_exe()` path is embedded in a systemd service file.

   2. **Impact factors:** Violates Integrity and Availability. Blast radius: one Linux desktop user. Data sensitivity: no witness data, but user-level persistence and command execution environment can be altered. Exploitability: local or supply-chain/install-path attack vector; attack complexity moderate; privileges low if attacker can influence where the user runs the app from; user interaction required to run from that path and enable autostart, or to launch with autostart already enabled.

   3. **Evidence confidence:** Moderate.

   4. **OWASP + CWE:** OWASP A03 Injection; CWE-74 Improper Neutralization of Special Elements in Output Used by a Downstream Component.

   5. **Trace:**  
      `packages/accelerator/src-tauri/src/commands.rs:45`-`55` exposes `set_autostart`; enabling calls `crate::crash_recovery::enable_crash_recovery()` at `commands.rs:51`. Startup also calls crash recovery when autostart is enabled at `packages/accelerator/src-tauri/src/main.rs:471`-`476`.  
      Dispatch enters `packages/accelerator/src-tauri/src/crash_recovery.rs:21`-`24`.  
      Linux implementation reads `std::env::current_exe()` at `crash_recovery.rs:134`-`140`.  
      It formats the path directly into `ExecStart="{exe}"` at `crash_recovery.rs:156`-`170`.  
      It writes the unit to disk at `crash_recovery.rs:173` and enables it with `systemctl --user enable` at `crash_recovery.rs:178`-`184`.

   6. **Missing control:** No rejection or escaping of systemd unit metacharacters in the executable path, especially quotes, newlines, percent specifiers, and control characters. The Windows path is XML-escaped at `crash_recovery.rs:351`-`399`; Linux has no equivalent unit-file escaping.

   7. **Exploit/violation scenario:**  
      1. Attacker causes the app to run from a crafted Linux path containing a quote and newline, such as a directory name that injects `ExecStartPre=` or `Environment=` into the `[Service]` section.  
      2. User enables autostart, or the app starts while autostart is already enabled.  
      3. The generated `aztec-accelerator.service` contains attacker-controlled extra unit directives.  
      4. On the next user-service start, systemd parses those injected directives, allowing attacker-chosen user-level commands or environment changes to run with the victim user’s privileges.

   8. **Preconditions:** Linux target; attacker can influence the executable path used by `current_exe()` and get the victim to run that copy; autostart/crash recovery must be enabled.

   9. **Why existing mitigations fail:** `Command::new("systemctl").args(...)` avoids shell injection in the immediate `systemctl` calls, but the vulnerable sink is the persisted systemd unit parsed later by systemd. The Windows XML escaping mitigation does not apply to the Linux unit writer.

   10. **Instances:**  
       `packages/accelerator/src-tauri/src/commands.rs:45`-`55`;  
       `packages/accelerator/src-tauri/src/main.rs:471`-`476`;  
       `packages/accelerator/src-tauri/src/crash_recovery.rs:134`-`184`.