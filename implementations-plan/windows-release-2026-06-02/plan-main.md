# Windows release — MAIN independent draft (pre-consolidation)

My independent take, written before reading the codex/opus plans, to keep it honest. Consolidation merges the strongest of all three into `plan.md`.

## Locked scope
bb.exe from the aztec-packages release asset `barretenberg-amd64-windows.tar.gz` (versioned to @aztec/bb.js); x64 only; **unsigned** v1; **full auto-update** in v1; **full CI parity** (build smoke + WebDriver E2E + updater-smoke + crash-recovery) before GA.

## Distinctive calls / insights (mine)
1. **Phase 0 is a bb.exe runtime spike — the single highest-risk unknown.** Before ANY app work: a throwaway `workflow_dispatch` on `windows-latest` that downloads `barretenberg-amd64-windows.tar.gz` (from the aztec-packages release matching the bb.js version), extracts `bb.exe`, and runs `bb --version` (and a tiny prove). Use `dumpbin /dependents bb.exe` (or `Dependencies`) to see if it needs the **VC++ redistributable** or ships DLLs that must sit beside it. If bb.exe needs DLLs/redist, the whole sidecar + installer story changes. **Resolve this first.**
2. **The Linux EADDRINUSE bug is already pre-fixed for Windows.** `bind_with_retry` in `server.rs` is cross-platform (no `cfg`), and Windows raises the same `AddrInUse` (WSAEADDRINUSE) — so the relaunch race we found on Linux is already covered on Windows. One fewer trap.
3. **The Windows-specific traps I'd actually watch (the EADDRINUSE-analogues):**
   - **Windows Defender quarantining the unsigned `.exe`/`bb.exe`** — real risk on `windows-latest`; could delete the installer or sidecar mid-build/mid-smoke. May need a Defender exclusion in the smoke (`Add-MpPreference -ExclusionPath`).
   - **UAC on the silent updater install** — the updater downloads + runs the NSIS installer; unsigned → UAC elevation prompt. In CI the runner user is admin, but a GUI UAC prompt would hang the headless smoke. Need NSIS `perMachine=false` (per-user install, no elevation) OR run elevated + `installMode` passive/quiet.
   - **Does Tauri actually RELAUNCH the app after the Windows installer runs?** (The Linux smoke proved Tauri *does* apply + relaunch; Windows is a different mechanism — verify the updater relaunches, else `/health==N` never comes up.)
   - **WebView2 absent** on a runner / a user's machine → app won't start; `webviewInstallMode: downloadBootstrapper`.
   - **Path/separator assumptions** in the Rust (audit `versions.rs`, `bb.rs`, `config.rs`, `certs.rs` for `/`-joins or `/etc`-isms).
4. **Per-user NSIS install** (`installMode: "perUser"` / NSIS `perMachine: false`) is probably the right call — avoids UAC entirely (installs to `%LOCALAPPDATA%`), which makes the unsigned auto-update path AND the headless smoke far simpler. Trade-off: no machine-wide install. For a single-user tray app, per-user is the right default anyway.

## Phasing
- **P0** bb.exe spike (above) — gate everything on it.
- **P1** `copy-bb.ts` win32 branch: fetch+checksum-verify+extract the tarball → `binaries/bb-x86_64-pc-windows-msvc.exe`. (The prebuild now needs network on the Windows build; pin the version to the bb.js version.)
- **P2** Windows Tauri build: `tauri.conf.json` bundle → NSIS only (narrow from "all"), `webviewInstallMode: downloadBootstrapper`, `installMode: perUser`. cfg-gate the Rust — Safari/HTTPS (`certs.rs`) macOS-only → Windows HTTP-only; crash_recovery Windows path; any Unix-only code. Output: an unsigned NSIS installer + a launch/`/health` build smoke.
- **P3** platform parity: crash-recovery via **Task Scheduler** (`schtasks /create` a restart task — least-invasive vs a Windows service, no admin if per-user logon trigger); autostart (auto-launch crate supports Windows); tray; paths.
- **P4** auto-update: updater config (relaunch verify), `latest.json` `windows-x86_64` key + the NSIS `.sig`, release-pipeline windows artifact.
- **P5** the Windows updater-smoke (HARDEST): adapt `updater-smoke-linux.sh`. Windows equivalents — `certutil -addstore -f Root ca.pem` (trust the local CA), hosts at `C:\Windows\System32\drivers\etc\hosts`, the OS-agnostic feed server under Bun-on-Windows, install N-1 NSIS silently (`installer.exe /S` per-user → no UAC), preseed `%USERPROFILE%\.aztec-accelerator\config.json` `auto_update:true`, launch, auto-update to N, poll `/health==N`, assert a `download/` hit. Add a Defender exclusion up front. Disambiguate harness-vs-updater failures (the Linux smoke's lesson).
- **P6** CI parity: build matrix + `windows-latest`; build smoke; WebDriver E2E (tauri-plugin-webdriver via WebView2 + `msedgedriver`); the updater-smoke — all blocking before GA.
- **P7** release integration + a Windows `1.0.x-rc` dry-run → green gates → GA. (Code-signing = the explicit deferred follow-up; unsigned auto-update works via minisign but warns.)

## Security & adversarial (mine)
- **Trust model:** the minisign `.sig` (embedded pubkey) protects update *integrity* on Windows exactly as on macOS/Linux — a tampered installer is rejected regardless of Authenticode. Authenticode absence = SmartScreen/UAC *warnings* (UX), not an integrity hole. Document this clearly so "unsigned" isn't read as "unverified."
- **Supply chain:** fetching `bb.exe` from a GitHub release is a new ingress — pin by exact version/tag AND verify a checksum (does the aztec-packages release publish a checksum/`.sha256`? if not, pin the asset digest). Don't fetch "latest."
- **MITM smoke hygiene:** the local-CA + hosts impersonation of `aztec-accelerator.dev` runs inside the ephemeral runner (same as Linux); use a run-unique CA name; clean up the cert store + hosts on exit.
- **Least privilege / per-user install** removes the elevation surface entirely.
- **Defender exclusion** in the smoke is a deliberate weakening of the test runner — scope it to the smoke's temp dir only, never disable Defender wholesale.

## Top risks ranked
1. bb.exe runtime deps (VC++/DLLs) — P0 spike resolves.
2. The updater-smoke (UAC + Defender + does-Tauri-relaunch) — the Linux-class make-or-break.
3. WebDriver E2E on Windows (WebView2 + msedgedriver headless) — may be the slowest gate.
4. Crash-recovery mechanism actually surviving a kill — Task Scheduler semantics.
5. Unsigned-install UX (SmartScreen) — accepted for v1, but the auto-update UAC prompt may be worse than expected → reinforces per-user install.
