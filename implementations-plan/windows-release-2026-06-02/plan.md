# Windows release — Aztec Accelerator (consolidated Tier-A plan)

## Context
Barretenberg/Aztec now publish a Windows `bb` build, so the accelerator (a Tauri 2 tray app that bundles `bb` for local zk proving) can finally ship on Windows. It currently ships macOS (arm64+Intel) + Linux (x86_64). This is a deep, cross-cutting port: a new sidecar source, a new bundle/installer, a brand-new updater path, platform parity (crash-recovery, paths, HTTPS-off), and a hard CI test story. Consolidated from three independent plans (main + codex `019e…` + opus subagent); provenance + rejected ideas documented at the end.

**Locked decisions (owner):** bb.exe from the `barretenberg-amd64-windows.tar.gz` asset on the `AztecProtocol/aztec-packages` release (versioned with `@aztec/bb.js`); **x64 only**; **UNSIGNED v1** (Authenticode deferred); **FULL auto-update in v1**; **FULL CI parity before GA** (build-smoke + WebDriver + updater-smoke + crash-recovery, all green; rc-shadow advisory→blocking is fine within that).

## Empirically VERIFIED facts that shape the work (opus probed the real artifact)
1. `@aztec/bb.js@4.2.0` ships **no** Windows build → fetching the GitHub tarball is the *only* path (forces a dual-source `copy-bb.ts`).
2. The tarball is **exactly `bb.exe`** (21.7 MB, `PE32+ console x86-64`), NOT `bb`. Breaks every hardcoded `"bb"`: `versions.rs:400` (extract), `versions.rs:75` (cache path), `versions.rs:176` (list), `bb.rs:38` (sidecar probe).
3. `bb.exe` is **fully self-contained** — import table = `api-ms-win-crt-*` (UCRT, in-box Win10 1709+), `KERNEL32/SHELL32/PSAPI/WS2_32/bcrypt`. **No VC++ redist, no sidecar DLLs** (zig-static). The biggest feared risk is gone.
4. `createUpdaterArtifacts: "v1Compatible"` → the Windows updater artifact is a **`.nsis.zip` + `.zip.sig`** wrapping the NSIS `-setup.exe` (the analogue of macOS `.app.tar.gz` / Linux `.AppImage`). `latest.json`'s `windows-x86_64` URL points at the `.nsis.zip`.
5. **No `icons/icon.ico`** — Tauri NSIS requires it; must generate from `icon.png`.
6. `main.rs` already has `windows_subsystem = "windows"` (no console) + a Windows `explorer` arm for `open_in_browser`. `dirs::home_dir()` → `C:\Users\<u>` so `~/.aztec-accelerator` is a valid Windows path. The `#[cfg(unix)]` 0o600/0o700 perm-restrictions are simply skipped on Windows (files inherit user-profile ACLs).
7. **`crash_recovery::enable/disable_crash_recovery` are defined only for macOS+Linux but called unconditionally** (`commands.rs:36,39`, `main.rs:263`) → **the Windows build will not link** without a Windows arm. This is a compile blocker, not a feature gap.

## Decision summary
```
Installer ........ NSIS, installMode = currentUser   (%LOCALAPPDATA%, no admin → NO UAC)
Updater install .. plugins.updater.windows.installMode = "quiet"  (click-free; works because currentUser)
WebView2 ......... webviewInstallMode = downloadBootstrapper (default; verify clean-VM first-run pre-GA)
bb.exe source .... GitHub release tarball, SHA-256 pinned to the @aztec/bb.js version (fail closed)
bb.exe runtime ... single self-contained file, no redist (VERIFIED via import table)
Crash recovery ... Task Scheduler RestartOnFailure (built-in, no new bundled binary) — CI kill-test; watchdog = fallback
HTTPS / Safari ... HTTP-only on Windows (no Safari) — code already self-disables; hide the Settings toggle
Runner ........... windows-latest (Server 2022/2025), x86_64-pc-windows-msvc
```

## Phasing (each independently verifiable)
```
[ ] P0  bb.exe: spike + dual-source copy-bb.ts + bb_exe_name() across versions.rs/bb.rs
[ ] P1  Windows Tauri build green (NSIS -setup.exe + updater .nsis.zip/.sig + icon.ico)
[ ] P2  Platform parity (crash_recovery Windows arm = LINK FIX; HTTP-only; AddrInUse ErrorKind match)
[ ] P3  Auto-update wiring (latest.json windows-x86_64 key + updater.windows.installMode)
[ ] P4  CI gates (build-smoke + WebDriver(win) + updater-smoke(win) + crash kill-test)  ← critical path
[ ] P5  Release-pipeline integration (matrix + flatten/rename + S3 feed + shell:bash)
[ ] P6  Windows rc dry-run (rc-shadow advisory; synthetic N-1 to bootstrap the first release)
[ ] P7  GA (flip Windows jobs blocking; docs/README/CLAUDE.md)
```

### P0 — bb.exe sidecar (foundation)
- **Spike first (de-risk):** a throwaway `windows-latest` job downloads the tarball, runs `bb.exe --version` + a real `/prove`, and `dumpbin /dependents bb.exe` asserts no `vcruntime/msvcp` (the supply-chain canary). *(opus already verified this off-runner; the spike confirms on the actual GH image.)*
- **`copy-bb.ts` dual-source:** macOS/Linux unchanged (npm-resolve); Windows fetches `https://github.com/AztecProtocol/aztec-packages/releases/download/v{VERSION}/barretenberg-amd64-windows.tar.gz` (`{VERSION}` = the `@aztec/bb.js` version already read at `copy-bb.ts:70`), **SHA-256-verifies against the release asset `digest`** (reuse the runtime pattern at `versions.rs:206-243`), **fails closed**, extracts `bb.exe` → `binaries/bb-x86_64-pc-windows-msvc.exe`. `getTargetTriple()` gains a `win32 → "x86_64-pc-windows-msvc"` arm; guard the `chmod`/`xattr` to non-Windows.
- **Rust bb naming (both paths):** add a shared `bb_exe_name()` (`#[cfg(windows)] "bb.exe"` else `"bb"`); use it in `versions.rs` (`version_bb_path`, `extract_bb_from_tarball` — accept both names so it's cross-OS testable, `list_cached_versions`) and `bb.rs` sidecar probe. Add `current_platform()` arm `amd64-windows` (`versions.rs:86`) so the **runtime multi-version downloader** (codex's catch) fetches the right Windows tarball.
- **`externalBin`** (`tauri.conf.json:25` = `binaries/bb`) needs no change *if* Tauri auto-appends `.exe` per host — **verify in P1**; fallback is a per-platform conf override.
- Tests (inline): synthetic-tarball test packing `bb.exe`; `current_platform()` includes `amd64-windows`; `bb_exe_name()` cfg test. All run cross-OS.

### P1 — Windows Tauri build
- Generate `icons/icon.ico` (multi-res from `icon.png`); add to `bundle.icon`.
- New `tauri.conf.json` `bundle.windows`: `nsis.installMode = "currentUser"`, `webviewInstallMode.type = "downloadBootstrapper"`. (Keep `targets:"all"` — it includes NSIS on Windows.)
- Verify: `prebuild` + `bunx tauri build --target x86_64-pc-windows-msvc` on `windows-latest` emits `*-setup.exe`, `*-setup.nsis.zip`, `*-setup.nsis.zip.sig`.

### P2 — platform parity
- **`crash_recovery.rs` Windows arm (LINK FIX + the parity mechanism).** See "Crash recovery" below — Task Scheduler `RestartOnFailure` for v1.
- **HTTP-only:** the HTTPS/Safari path already self-disables on Windows (`try_start_https` early-returns on `!safari_support`; `is_ca_trusted()`→false; `enable_safari_support` is a non-macOS `Err` stub — `commands.rs:184`, `certs.rs:285`). Cleanup: hide the Safari row in `settings.html` when `platform==="windows"`; confirm no dead-code clippy warnings.
- **`server.rs` / `main.rs:376` AddrInUse:** match on `e.kind() == ErrorKind::AddrInUse` (not the substring) so the friendly "port in use" message fires on Windows (the Windows error text differs). `bind_with_retry` itself already covers the relaunch race (all three plans agree; opus: Windows' overlap is *wider* → watch the 5 s budget in P4).
- Verify: `cargo test` + `cargo clippy -D warnings` green on `x86_64-pc-windows-msvc`.

### P3 — auto-update wiring
- `tauri.conf.json` `plugins.updater`: add `"windows": { "installMode": "quiet" }`. Endpoint + minisign pubkey unchanged (platform-agnostic; **minisign — not Authenticode — is the integrity gate, which is why full auto-update is viable unsigned**).
- `latest.json` gen (`release-accelerator.yml:578-621`): add `windows-x86_64` (sig from `*-setup.nsis.zip.sig`, url to the renamed `.nsis.zip`); add to the all-sigs-present assertion + `verify-live-feed` `has(...)` check (`:767`).

### P4 — CI gates (critical path; updater-smoke detailed below)
- **Build matrix** (`release-accelerator.yml:91`): add `x86_64-pc-windows-msvc / windows-latest / windows-x86_64`. **Add `shell: bash`** to the bash build step (windows-latest defaults to PowerShell — classic trap). `setup-accelerator` composite: add `Windows-X64) HOST="x86_64-pc-windows-msvc"` to the host-map (`action.yml:50`) — it currently fails on Windows.
- **`smoke-windows`** (mirror `smoke`): silent-install `-setup.exe /S` (currentUser → no UAC), launch the installed `.exe`, poll `/health` == version, assert the bundle contains exactly `{aztec-accelerator.exe, bb-…-windows-msvc.exe}` (the bundle-shape invariant, adapted) + the import-canary on `bb.exe`.
- **WebDriver(windows)** in `_e2e-webdriver.yml`: no Xvfb/tray-host needed (Windows has a session); parameterize the bash/Unix bits (binary name `.exe`, `/tmp`→a Windows temp, `kill`→`taskkill`, the launch + cleanup). `wdio.conf.ts`: Windows capability `msedge` (not `webkit`). The app already opens the Settings window for webdriver builds (`main.rs:365`) → a browsing context even if the tray is flaky. **This is risk #1 (headless tray + msedgedriver) — prove a minimal launch+/health+:4445 job before the full E2E.**
- **`update-smoke-windows`** — see below.
- **crash kill-test** — see "Crash recovery".
- **PR-gate Windows clippy/test legs** in `accelerator.yml` (behind the `changes.desktop` filter) — cheap regression insurance.

### P5 — release-pipeline integration
- Flatten/rename (`:525`): `*-setup.exe` → `…-Windows-x86_64-setup.exe` (direct download, the `.dmg` analogue), `*-setup.nsis.zip`(+`.sig`) → `…-setup.nsis.zip`. Add all three to `EXPECTED[]` + the `gh release create` glob.
- Upload-artifacts (`:206`): add the NSIS paths.
- **Headless `accelerator-server` on Windows: DEFER** (no demand; would add a 5th leg). Desktop app is the Windows deliverable.
- Release notes + README + CLAUDE.md + UPDATER_TESTING.md: add Windows; note unsigned→SmartScreen on first install, click-free minisign-verified updates after.

### P6 — Windows rc dry-run (rc-shadow)
- Land Windows jobs **advisory** (`continue-on-error` inside the reusable + absent from `tag/release.needs`), exactly as Linux did.
- **Bootstrapping problem (opus):** the *first* Windows release has **no N-1 Windows asset**. The updater-smoke's "find latest stable + download N-1 Windows asset" finds nothing → must **skip-with-`::notice::`** (not hard-fail) when no N-1 exists. To actually exercise the updater before GA, the dry-run builds a **synthetic N-1** (a hand-bumped lower-version installer) and proves N-1→N. Flip to a real N-1 smoke once a Windows stable exists.
- rc prereleases skip S3/`bump-source` (`is_prerelease` gates) → safe.

### P7 — GA
- Flip `smoke-windows` / `e2e-webdriver(windows)` / `update-smoke-windows` / crash-test to **blocking** (into `tag.needs`+`release.needs`, drop `continue-on-error`). First **stable** cut ships `windows-x86_64` in `latest.json`.

## The hard problem — Windows updater-smoke (concrete design)
Headless on `windows-latest`; the Windows analogue of `updater-smoke-linux.sh`. New `_e2e-updater-windows.yml` + `scripts/updater-smoke-windows.ps1` (PowerShell — idiomatic for `Start-Process`/`certutil`/`taskkill`); **reuse `updater-feed-server.ts` unchanged** (already cross-platform). Linux→Windows mapping:
| step | Linux | Windows |
|---|---|---|
| local CA+leaf | `openssl` | same `openssl` (on the runner) |
| trust CA | `update-ca-certificates` | `certutil -addstore -f -user Root <ca.pem>` → **`CurrentUser\Root`** (less invasive; reqwest on Windows = schannel reads the user store too). Only fall back to machine `Root` if the smoke proves schannel needs machine scope. |
| impersonate host | `/etc/hosts` | `C:\Windows\System32\drivers\etc\hosts` + `ipconfig /flushdns` |
| feed :443 | `sudo bun feed-server` | `bun feed-server` (no sudo; runner is admin) |
| install N-1 | `cp AppImage; chmod` | `Start-Process $N1setup.exe /S -Wait` (currentUser → no UAC) |
| preseed | `~/.aztec-accelerator/config.json auto_update:true` | `%USERPROFILE%\.aztec-accelerator\config.json` (same JSON) |
| assert | `/health==N` + `download/` hit + AppImage sha changed | `/health==N` + `download/` hit + **installed `.exe` sha256 changed** (the in-place-swap proof) |
| negative leg | tampered tarball + genuine sig → rejected | tampered `.nsis.zip` + genuine sig → `/health` never N (proves minisign teeth **even unsigned** — the single most important security assertion) |
| cleanup | `pkill -f` (argv footgun) | `taskkill /F /IM aztec-accelerator.exe` (image-scoped, **safer** — no argv match); run-unique CA CN + `certutil -delstore`; anchored hosts-line delete |

**The two experiments this job decides:** (a) does `quiet`+`currentUser` give a genuinely **click-free** install on the runner (Tauri #6955 reported `passive` needing a click — fallback: `passive` + accept a progress window)? (b) does the relaunched N app **rebind `:59833` within the 5 s `bind_with_retry` budget** given Windows' wider `TIME_WAIT`/re-exec overlap (the predicted Linux-EADDRINUSE analogue)? Both are make-or-break and only resolvable here. **Defender:** pre-emptively `Add-MpPreference -ExclusionPath` (scoped to the smoke's install+temp dirs, ephemeral runner only) to avoid quarantine of the unsigned `.exe`.

## Crash recovery (the one consolidation fork)
`enable/disable_crash_recovery` must gain a Windows arm or the build won't link. Two distinct semantics: **(1) restart-at-login** — already provided by `tauri-plugin-autostart`'s `HKCU\…\Run` entry (so `set_autostart(true)` already covers reboot/logout on Windows); **(2) restart-on-crash-within-session** (the launchd `KeepAlive` / systemd `Restart=on-failure` parity the owner asked for).
- **codex:** a per-user **Task Scheduler** task (`AtLogOn`, `RestartOnFailure`, `MultipleInstances IgnoreNew`) backing *both* autostart + crash-restart; have `set_autostart`/`get_autostart_enabled` query the task on Windows (avoid stacking registry + task).
- **opus:** Task Scheduler's GUI-app failure-detection is unreliable; prefers autostart-at-login now + a **sibling-crate watchdog** deferred — but a watchdog ships *in the bundle* → reintroduces the 1.0.1 stowaway risk.
- **CONSOLIDATED DECISION (PROVISIONAL — gated by 3 tests, per the final codex review):** for v1 parity, **Task Scheduler `RestartOnFailure`** is the *provisional* mechanism (built-in, **no new bundled binary**, no stowaway risk), driven by exit code (0 on intentional quit → no restart; non-zero on crash → restart). codex rightly flagged that a `taskkill` test alone overstates what's proven — it's adopted **only if it passes all three gates:**
  1. **Crash test:** `taskkill /F` the app → it relaunches + `/health` recovers + `schtasks` shows the entry.
  2. **Quit test (the one a kill-test misses):** trigger the *intentional* quit (tray Quit, exit 0) → assert it **stays down** (no relaunch). Without this the app is un-quittable.
  3. **Updater-handoff test (the real trap):** while N-1 auto-updates (old process exits to be replaced), assert Task Scheduler does **NOT** relaunch the *old* build mid-handoff (it would race the installer). This is exactly where exit-code restart bites.
  **If any gate fails → the sibling-crate watchdog is the mandatory fallback** (sole new bundled binary, asserted by the bundle-shape invariant, held to the same three gates).
  Note: the Windows arm is also a hard **compile blocker** — `set_autostart` calls `enable_crash_recovery` unconditionally (`commands.rs:31`), so the Windows build won't even link without *some* Windows implementation. **OWNER DECISION (2026-06-02): Task Scheduler.** The 3 gates stand; the watchdog is the fallback only if a gate fails.

## Security & adversarial
- **Trust model (unsigned but cryptographically safe):** no Authenticode → SmartScreen friction on first install (social, accepted). But the **update channel is gated by minisign** (`.zip.sig` vs embedded Ed25519 pubkey) verified before run — independent of Authenticode. An attacker MITM-ing the feed **cannot** push a malicious update without the signing key. The **negative updater-smoke leg proves this on Windows** — the most important security test in the port.
- **Supply chain (bb.exe fetch):** version-pinned + SHA-256-verified against the GitHub asset digest, fail-closed; immutable per-tag; the import-table canary catches an ABI switch or a swapped binary. Residual: a compromised AztecProtocol GH account (out of scope; same as today's runtime download).
- **Smoke MITM hygiene:** the local-CA + hosts impersonation is a MITM-of-ourselves on an **ephemeral** runner; run-unique CA CN, anchored cleanup, `certutil -delstore`. Don't run on shared self-hosted Windows without teardown.
- **Least privilege:** Windows jobs inherit `contents: read`; updater-smoke needs only that; no new secrets (S3/OIDC + the GitHub-App bump token are unchanged). `currentUser` install removes the elevation surface entirely.
- **File perms:** `#[cfg(unix)]` 0o600/0o700 skipped on Windows → user-profile ACLs (private already). **No CA private key is ever written on Windows** (HTTPS disabled) → the sensitive-file concern is moot. Optional `icacls` hardening is a follow-up.
- **Defender exclusion** in the smoke is a deliberate, scoped CI weakening (ephemeral VM, smoke dirs only) — never disable Defender wholesale, never ship it.

## Risks, ranked
1. **Headless tray + WebView2 + msedgedriver on windows-latest** (WebDriver E2E) — the biggest unproven assumption (the Linux stalonetray analogue). Mitigate: minimal launch+/health job first; the Settings-window-for-webdriver gives a context.
2. **`msedgedriver` ↔ WebView2 version match** — classic Windows-CI flake; auto-driver + retries.
3. **Defender quarantining the unsigned `.exe`/`bb.exe`** in CI — pre-emptive exclusion.
4. **EADDRINUSE budget on Windows** (the direct Linux-analogue) — the relaunched app may not rebind `:59833` within 5 s; the updater-smoke catches it; bump the Windows budget / add `SO_REUSEADDR` if it trips. A **real user-facing update bug**, not just a flake.
5. **`quiet`/`passive` updater needing a click** (Tauri #6955) — fallback to `passive`; the smoke decides.
6. **`externalBin` `.exe` auto-suffix** — verify in P1.
7. **Clean-VM first-run (no WebView2)** — CI never tests it; one manual fresh-VM smoke pre-GA; `embedBootstrapper` fallback.
8. **`shell:` PowerShell default** on the build step — add `shell: bash`.
9. **First-Windows-release has no N-1** — skip-with-signal + synthetic N-1.
10. **Watchdog reintroducing the bundle-stowaway break** — avoided by choosing Task Scheduler; if a watchdog is ever needed, sibling crate + bundle-shape assertion.

## Verification (end-to-end)
P0 `prebuild`+`cargo test` cross-OS; P1 a Windows `tauri build` produces the 3 artifacts; P2 clippy/test green on msvc; P4 the four Windows jobs green (the updater-smoke proving click-free minisign-verified N-1→N + the negative leg + the kill-test relaunch); P6 a green advisory rc dry-run; P7 a stable cut shipping `windows-x86_64` that real Windows rc users auto-update from.

## Files touched (index)
Rust: `copy-bb.ts` (rewrite), `versions.rs`, `bb.rs`, `crash_recovery.rs` (**link fix**), `server.rs`/`main.rs:376` (AddrInUse). Config/assets: `tauri.conf.json` (+`bundle.windows`, `plugins.updater.windows`, `bundle.icon`), new `icons/icon.ico`. Frontend: `settings.html` (hide Safari on Windows). CI: `release-accelerator.yml`, `_e2e-webdriver.yml`, `accelerator.yml`, `setup-accelerator/action.yml`, new `_e2e-updater-windows.yml`, new `scripts/updater-smoke-windows.ps1`, `wdio.conf.ts`, reuse `updater-feed-server.ts`. Docs: README, CLAUDE.md, UPDATER_TESTING.md.

## Plan provenance (adopted / rejected, per source)
- **opus** (empirical backbone): bb.exe self-contained (eliminates the DLL risk), tarball=bb.exe, `.nsis.zip` updater artifact, no `.ico`, crash_recovery link-blocker, `currentUser`+`quiet` linchpin, the full updater-smoke mapping, the bootstrapping-first-release problem, the 10-risk ranking, the security section. **Adopted wholesale.**
- **codex:** the *second* bb path (runtime downloader hardcodes `"bb"`) — **adopted** (opus detailed the lines); the autostart-can't-restart-on-crash point + Task-Scheduler mechanism — **adopted for v1** (over opus's watchdog, to avoid the stowaway risk); `wdio` webkit→msedge — **adopted**; setup-composite host-map + `shell:bash` — **adopted**.
- **main (me):** spike-first framing, `bind_with_retry`-already-covers-Windows (both agents confirmed), per-user-dodges-UAC — **adopted**.
- **Rejected:** opus's autostart-only-v1 crash-recovery (doesn't meet the owner's "crash-recovery parity"); a Windows **service** (session-0 can't host a tray); **MSI** (no `currentUser`, irrelevant GPO strength); shipping a **headless Windows server** in v1 (no demand); `embedBootstrapper`/`offlineInstaller` WebView2 (size, unnecessary above the UCRT floor). Deferred: **Authenticode signing** (owner's explicit follow-up); the in-session **watchdog** (only if Task Scheduler's kill-test fails).

See [eli5.html](eli5.html).
