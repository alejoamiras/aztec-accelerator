# Phase 4 — Windows CI gates (DESIGN, drafted while SSH flaked)

The make-or-break phase. Four gates: Windows updater-smoke (positive + negative),
WebDriver(win) E2E, the crash-recovery 3-gate tests, + the dual-launch fix (codex High #1).
All need windows-latest CI iteration (like P2's 5 iterations) — execute when SSH is stable.

## 1. Windows updater-smoke (port of updater-smoke-linux.sh)
Same trust model: serve the already-signed N artifact from a local HTTPS server
impersonating `aztec-accelerator.dev`; unmodified N-1 minisign-verifies + self-updates; poll
`/health==N`. Linux→Windows mapping:
| Linux | Windows |
|---|---|
| `.AppImage` + `.AppImage.sig` | `*-setup.nsis.zip` + `.nsis.zip.sig` (the v1Compatible updater artifact) |
| `update-ca-certificates` (system store) | `certutil -addstore -f -user Root <ca.pem>` → `CurrentUser\Root` (reqwest→schannel reads it). Fall back to machine `Root` only if schannel ignores user store. |
| `/etc/hosts` | `C:\Windows\System32\drivers\etc\hosts` (`127.0.0.1 aztec-accelerator.dev`) |
| `sudo bun feed-server` on :443 | **no sudo** — Windows lets any user bind :443; run `bun updater-feed-server.ts` directly |
| `cp + chmod +x` AppImage | run N-1 `-setup.exe /S` (silent NSIS, installMode currentUser → `%LOCALAPPDATA%`), then launch the installed exe |
| Xvfb + stalonetray + dbus | windows-latest has an interactive desktop + native tray — **no display setup** (but: does the tray app actually run in the runner session? = risk #1, this smoke answers it) |
| in-place-swap = AppImage sha changes | the updater downloads `.nsis.zip` → extracts `-setup.exe` → runs it silently → reinstalls N over N-1. Proof: `/health==N` + a `/releases/download/` feed hit + the installed exe's version/hash changed |
| config at `$HOME/.aztec-accelerator` | `%USERPROFILE%\.aztec-accelerator\config.json` (app uses `dirs::home_dir()`) — preseed `auto_update:true` |
| — | **Defender exclusion** (`Add-MpPreference -ExclusionPath <smoke dirs>`) — scoped to the install + serve dirs ONLY, ephemeral runner only, never shipped |
- Negative mode: append a byte to N's `.nsis.zip` (genuine sig untouched) → schannel/minisign over tampered bytes must FAIL → assert `/health` never reports N + a download hit happened (so rejection was actually exercised).

## 2. The synthetic-N-1 bootstrap (the hard part — no prior Windows release)
Linux downloads N-1 from the last stable; Windows has NO prior release. So build N-1 in-job,
self-contained with ONE ephemeral minisign keypair (no prod key needed — same posture as the
P2 build-smoke's ephemeral key):
1. `tauri signer generate` → `K_pub`/`K_priv`.
2. **Build N-1:** patch tauri.conf `version`→`0.0.1` AND `plugins.updater.pubkey`→`K_pub`;
   `tauri build --bundles nsis` with `TAURI_SIGNING_PRIVATE_KEY=K_priv` → N-1 `-setup.exe`
   (embeds `K_pub`).
3. **Build N:** patch `version`→`9.9.9` (keep `K_pub`); sign with `K_priv` → N `*.nsis.zip` + `.sig`.
4. Install N-1, feed serves N; N-1 verifies the sig against its embedded `K_pub` (matches
   `K_priv`) → updates → `/health==9.9.9`.
- Cost: **two release builds** (~16min each) per run. Heavy but unavoidable for the first
  release. Once a real Windows stable exists, switch to the Linux pattern (download N-1,
  serve the prod-signed N — no key in the smoke). Note this as the bootstrap-only path.
- A `_e2e-updater-windows.yml` reusable (mirror `_e2e-updater-linux.yml`): inputs n-version,
  mode; checkout + setup-bun + Defender-exclude + build-N-1 + build-N + run the smoke script.

## 3. WebDriver(win) E2E
The `accelerator.yml` e2e-webdriver matrix is macos+linux (it EXCLUDES Windows — codex noted).
Add a `windows` matrix leg using `tauri-plugin-webdriver` + WebView2's `msedgedriver`
(preinstalled on windows-latest). The webdriver build opens the Settings window (main.rs:366)
so WebDriver has a browsing context. Risk: the tray/WebView2 driving headlessly on the runner.

## 4. Crash-recovery 3-gate tests + the dual-launch fix (codex High #1)
- **Gate A (crash→relaunch):** `taskkill /F` the app → Task Scheduler RestartOnFailure
  relaunches → `/health` recovers + `schtasks /query` shows the task.
- **Gate B (quit→stays-down):** trigger the intentional quit (exit 0) → assert NO relaunch.
- **Gate C (updater-handoff):** during an N-1→N update, assert Task Scheduler does NOT relaunch
  the OLD build mid-handoff (would race the installer).
- **Dual-launch fix (the real bug codex found):** the Run key AND the Task Scheduler logon
  trigger both launch at logon; the loser of the `:59833` bind does NOT self-terminate
  (`main.rs:373-381` just sets a tray tooltip) → ghost tray. **Candidate fix: exit-on-bind-fail**
  — when `bind_with_retry` exhausts (another instance owns the port), `app.exit(1)` instead of
  staying resident. Makes double-launch benign single-instance on ALL platforms. **CONSULT CODEX**
  on this fork (exit-on-bind-fail vs suppress-the-Run-key-when-Task-Scheduler-active vs watchdog)
  before implementing — it touches mac/linux behavior too.

## Sequencing
Updater-smoke first (highest risk — proves the Windows update mechanism + answers "does the
tray app run on the runner"). Then WebDriver(win). Then crash-recovery + the dual-launch fix.
Each is its own PR, windows-latest-validated, codex-reviewed. Expect P2-style multi-iteration.

## Status
Designed during the AFK window while the 1Password/SSH agent was flaking (blocked reliable CI
iteration). Execute when SSH is stable. P0–P3 are banked (P3 committed locally `c161015`,
auto-pushes on SSH recovery via watcher b7y1txmfx).
