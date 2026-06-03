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
- **Dual-launch fix (codex consult `019e…` done — DESIGN LOCKED, implement with CI):**
  the Run key AND the Task Scheduler logon trigger both launch at logon; the loser of the
  `:59833` bind does NOT self-terminate (`main.rs:373-381` sets a tray tooltip + stays resident)
  → ghost tray. Fix = **exit-0-on-bind-fail, but CLASSIFY first** (codex's key correction):
  - `bind_with_retry` only proves "port busy 5s," not "a *healthy Aztec* instance owns it."
  - So on bind exhaustion, **probe `http://127.0.0.1:59833/health`** (new
    `server::healthy_aztec_on_port()` — true iff `status=="ok" && api_version==1`).
    - healthy Aztec answers → `app_handle.exit(0)` (redundant instance bows out; **exit 0**, NOT
      non-zero, or it loops against Task Scheduler `RestartOnFailure`/systemd `on-failure`/launchd
      `KeepAlive`). Cross-platform.
    - foreign process / no answer → keep the current visible "port in use" tooltip + stay resident
      (do NOT exit non-zero → avoids a restart loop on a persistent foreign conflict).
  - Wire in `main.rs:372-385` (add `app.handle().clone()` into the spawn). Add a unit test for
    the classifier. **Sharp edge codex flagged (→ Gate C):** updater handoff — if the NEW build
    bows out (exit 0) while the OLD build still owns the port and then dies, nothing runs + no
    supervisor restart. The 5s retry is the gating assumption; Gate C must exercise this.
  - `tauri-plugin-single-instance` is NOT a fit (it'd reject the legit updater-relaunch overlap
    that `bind_with_retry` is designed to tolerate). Confirmed by codex.
  - **NOTE:** this changes SHIPPED macOS/Linux behavior (a redundant instance now exits instead
    of ghosting) — must land with green mac/linux e2e/smoke, so implement it with stable CI, not
    blind in an AFK/flaky-SSH window.

## Sequencing
Updater-smoke first (highest risk — proves the Windows update mechanism + answers "does the
tray app run on the runner"). Then WebDriver(win). Then crash-recovery + the dual-launch fix.
Each is its own PR, windows-latest-validated, codex-reviewed. Expect P2-style multi-iteration.

## Status
Designed during the AFK window while the 1Password/SSH agent was flaking (blocked reliable CI
iteration). Execute when SSH is stable. P0–P3 are banked (P3 committed locally `c161015`,
auto-pushes on SSH recovery via watcher b7y1txmfx).

## P4 updater-smoke — PROVEN + merged (#273)
Both legs GREEN on windows-latest, reproducibly:
- **positive**: install synthetic N-1 (0.0.1) → click-free auto-update to N (9.9.9) via local
  minisign-signed feed → quiet-install → relaunch → /health == 9.9.9.
- **negative**: tampered `.nsis.zip` (genuine sig) REJECTED — never reaches 9.9.9.
Harness: `scripts/updater-smoke-windows.ps1` + reusable `_e2e-updater-windows.yml`
(ephemeral minisign keypair, builds N-1 + N in-job, synthetic-N-1 bootstrap).
Key fixes that got it green (from app-log evidence over 4 rounds):
- CA must go to `LocalMachine\Root` (CurrentUser\Root pops a Trusted-Root GUI dialog → freezes
  the headless runner — regardless of certutil/Import-Certificate/X509Store).
- `rm -rf target/release/bundle/nsis` after copying N-1's artifact, else N reuses N-1's stale zip.
- `plugins.updater.windows.installMode: "quiet"` — the only fully click-free mode (default
  `passive` shows a progress window that freezes the runner). Also = parity with silent mac/linux.

### Codex review (session 019e8e71, ship-with-changes)
- **FIXED now**: Defender exclusion was added but never removed — added `Remove-MpPreference`
  for the same scoped paths ($InstallRoot, $ServeDir) to the `finally` Cleanup. No impact on
  ephemeral GH runners but never leave an AV hole in a committed harness.
- **Deferred to P5 blocking-flip (#93)**: (1) HIGH — single-slot release concurrency means a
  hung advisory Windows leg holds the run `in_progress`, delaying a later dispatch; (2) MED — a
  failed advisory leg makes the overall run conclude `failure` (harmless in-repo; resolved by
  the blocking flip). Did NOT touch the proven-green timeout to chase a low-probability hang.
- **Accepted**: `quiet` UX (no native progress/error UI) is a deliberate tray-app choice +
  parity with mac/linux silent auto-update.

### P5 partial (in #273)
Per-PR smoke scaffolding removed from accelerator.yml; the proven jobs now live in
release-accelerator.yml as ADVISORY (needs:[validate], absent from tag/release needs).
Flip to blocking after a green Windows rc dry-run (#93/P6).

## P4c crash-recovery — RestartOnFailure is BROKEN (codex 019e8e9e + empirical proof)
Codex flagged that Task Scheduler `<RestartOnFailure>` may not relaunch on a non-zero/crash
exit. Spiked it on a real windows-2025 runner (SYSTEM principal to isolate the mechanism):
```
exit0 runs=1  (stays down — fine)
exit1 runs=1  (graceful non-zero does NOT relaunch)
kill  runs=1  (abnormal kill = real crash does NOT relaunch)  -> crash->relaunch BROKEN
```
RestartOnFailure is for "the engine couldn't start the action", NOT "the action ran then died".
mac (launchd KeepAlive{SuccessfulExit:false}) + linux (systemd Restart=on-failure) genuinely
work and key on the exit code; Windows had no working equivalent. Merged-but-never-released, so
zero users affected — exactly what this test push exists to catch pre-release.

### Fix (user chose: repeating-trigger task)
Two parts:
1. **task_xml**: replace `LogonTrigger + RestartOnFailure` with a REPEATING `TimeTrigger` (PT1M,
   no Duration) + keep `MultipleInstancesPolicy=IgnoreNew`. Every minute TS tries to start the
   action; IgnoreNew = no-op if alive, RELAUNCH if dead. ~<=1min recovery latency.
2. **disable-on-quit** (app code): a repeating trigger relaunches ANYTHING not running, incl.
   after an intentional quit — which would break quit->stays-down. So the Quit path
   (main.rs `"quit" => app.exit(0)`) must call `disable_crash_recovery()` (delete the task)
   BEFORE exit. A crash skips that path -> task remains -> relaunched. Clean quit deletes it
   first -> stays down. (Updater handoff: restart() launches the NEW build at the same exe path;
   task either way points there.)

Round-2 spike (crash-recovery-smoke-windows.ps1) confirms the two OS behaviors the fix needs:
relaunch-on-death (runs grows) + no-dup-when-alive (IgnoreNew, runs stays 1). Spike FIRST,
then implement — same discipline that caught the round-1 bug.
