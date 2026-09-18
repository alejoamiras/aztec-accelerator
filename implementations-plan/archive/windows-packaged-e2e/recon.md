# Recon — Windows packaged-E2E legs (composed proof + full uninstall)

Three parallel read-only explorers (harness anatomy / Windows prior art / uninstall semantics), consolidated.
Every claim below carries a `file:line`; the load-bearing trust claims were re-verified by hand.

## 0. Headline: the composed-proof leg is BLOCKED on a product decision (the uninstall leg is not)

- `src-tauri/src/main.rs:121-129` — the launch gate. Linux: `let ca_trusted = || true` (trust-decoupled).
  Not-Linux (macOS + Windows): `ca_trusted = certs::is_ca_trusted`. So on Windows the shipped app REFUSES
  to bind the HTTPS listener (`:59834`) unless its own trust predicate passes. `main.rs:87-90`: trust is
  only ever VERIFIED at launch, never installed (launch must never raise a prompt).
- `src-tauri/src/trust/windows.rs:155-177` `live_present()` — trust == present in **CurrentUser `Root`** by
  SERIAL **and not** in CurrentUser `Disallowed`. Shells out to `certutil.exe -user -store Root <serial>`:
  a store ENUMERATION, not a chain build. `-user` = CurrentUser only; LocalMachine is never queried.
- `src-tauri/src/trust/windows.rs:59-66` `add_store()` = `certutil -user -addstore Root` — the consent write.
- `tests/trust_windows.rs:1-9` — adding to `CurrentUser\Root` (certutil OR .NET `X509Store.Add`) raises the
  root-CA "Security Warning" dialog and CANNOT be done silently; CI exercises only headless-SAFE read paths.
  (`trust/windows.rs:15-17`'s claim that CI "seeds non-interactively via Import-Certificate" is WRONG and
  still uncorrected on disk — a prior codex review flagged it.)
- `scripts/updater-smoke-windows.ps1:156-163` — existing CI DOES silently seed a CA, deliberately into
  **LocalMachine\Root** (runner is admin ⇒ no dialog), and then serves a real HTTPS feed a client validates.
- `src-tauri/Cargo.toml:18-20` — features are only `default = []` and `webdriver`. No `e2e-trust` seam exists.
- Prior art on this exact wall: `implementations-plan/v2-release-train/b4-impl.md:215-216` ("D. Windows leg —
  BLOCKED on the owner decision... Do NOT build until resolved") and `v2-release-train/plan.md:241-270`
  (raw HKCU DER write insufficient: value must be a serialized store-element blob AND CurrentUser\Root
  applies protected-root filtering; options (a) test-only `e2e-trust` feature, (b) pre-trusted self-hosted
  runner, (c) documented manual pre-GA check).

**Why (a) does not fit THIS task**: the goal requires the leg to consume the **staged installers** — the real
shipped artifact. A special-featured build proves something about a binary users never install.

**The uninstall leg is NOT blocked**: it needs no HTTPS and no chain trust. Cert removal is delete-only and
headless-safe (`trust/windows.rs:189-209` `delete_by_cn`, "No dialog on delete"; NSIS belt `hooks.nsi:388`).

## 1. `_e2e-packaged.yml` anatomy (4 jobs, no aggregator)

- `stage-installers` `:57-114` — ubuntu, `timeout 10`, the ONLY `contents: write`; no checkout/no app code.
  Release-asset mode `:64-73` downloads with `--pattern '*Linux-x86_64.deb' --pattern '*macOS-Apple-Silicon.dmg'`
  — **no Windows pattern** (one-line prerequisite fix). Build-artifact mode `:99-105` uses
  `pattern: accelerator-*x86_64` which ALREADY matches `accelerator-windows-x86_64`. Uploads `staged-installers`.
- `packaged-e2e-linux` `:116-241` — the composed reference. xvfb/stalonetray/dbus + `libnss3-tools` `:142-153`;
  verify-manifest (gated on `release_tag != ''`) `:162-164`; `.deb` install `:166-177`;
  `--generate-certs-only` `:180-181`; NSS seed `certutil -A -t "C,," -d "sql:$HOME/.pki/nssdb"` `:183-190`;
  config `{"config_version":2,"https_enabled":true,"auto_approve_localhost":true}` `:192-198`; launch + poll
  `https://127.0.0.1:59834/health` 45x2s `:200-214`; swap-sdk `:218-219`; `test:e2e:packaged` `:221-224`;
  logs on failure `:226-236`; `Cleanup` `if: always()` `:238-240`.
- `packaged-e2e-macos` `:242-372` — same skeleton. `bunx playwright install chromium` `:259-260` (the cache
  action is Linux-only). DMG -> `/Applications`, binary addressed BY NAME
  (`/Applications/$APP_NAME/Contents/MacOS/AztecAccelerator`) `:291-298` — a bare `find|head -1` grabs the
  bundled `bb` sidecar (fixed in `b823374`). Trust seeded BEFORE launch into the System keychain
  (`sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain`) + verified `:307-314`.
  Cleanup carries `timeout-minutes: 3` + `continue-on-error: true` `:356-372` (a hung `remove-trusted-cert`
  once burned a PASSING run to the job timeout — `def1cbe`). **Imitate this for any Windows cleanup.**
- `upgrade-migration-linux` `:378-459` — HTTP `:59833` only; asserts `.config_version == 2`,
  `has("safari_support") | not`, `.approved_origins | index("http://preserved.example")` `:441-454`.
- `uninstall-linux` `:463-506` — no launch, no trust. install `.deb` -> `command -v` -> `--prepare-uninstall`
  -> `apt-get remove` -> `hash -r` -> assert `! command -v` `:486-505`. Header `:460-462` says the per-store
  detail is deliberately left to B5's unit tests. **The asked-for Windows assertions are DEEPER than this —
  an intentional upgrade, not a parity bug.**

Both scripts are OS-agnostic and should port to Git Bash unchanged: `packaged-e2e-swap-sdk.sh` (bun/npm pack/
tar/rm/mkdir) and `packaged-e2e-verify-manifest.sh` (awk exact-name match + sha256sum).

## 2. Caller wiring — the new jobs fold in automatically (and become release-blocking)

`release-accelerator.yml:1075-1096` `packaged-e2e-on-draft` passes only `ref: github.sha` + `release_tag`, at a
`contents: write` ceiling. `tag` `:641-651` and `finalize` `:1101-1113` gate on
`needs.packaged-e2e-on-draft.result == 'success'`. Because that is the WHOLE called workflow's result, adding
jobs needs ZERO caller edits — and equally means **an unconditional Windows job that cannot pass permanently
reds every future release**. Convention: all 4 existing jobs are unconditional at job level, with the
`release_tag != ''` gate applied PER-STEP.

## 3. Contract test — one count to change

`packages/accelerator/scripts/release-contract.test.ts:244-256` counts 6-space-indented permission lines:
`contents: write` must be 1, `contents: read` must be 4. Two new read-only jobs ⇒ **4 -> 6** (plus the prose
message). Nothing else in that file pins a job name or count from inside `_e2e-packaged.yml`.

## 4. Browser-proof mechanics (exact witnesses)

Spec `packages/playground/e2e/accelerator.packaged-e2e.spec.ts`, project `packaged-e2e`
(`playwright.config.ts:51-64`, 15min timeout, `retries: 1`, webServer `bun run dev` :5173).
- `HTTPS_PROVE_URL = "https://127.0.0.1:59834/prove"` `:29`; page `/?forceProofs=true&httpsOnly=true` `:55`
  (`httpsOnly` -> `new AcceleratorProver({ accelerator: { httpsOnly: true, allowInsecureDowngrade: false }})`,
  `playground/src/aztec.ts:196-204`).
- Network witness: a response where `url === HTTPS_PROVE_URL && status === 200 &&
  headers["x-prove-duration-ms"] !== undefined` `:38-46,:73-81` (header only set after a real `bb::prove`).
- Phase witness: `window.__ACCEL_PHASES__` (`aztec.ts:475-481`) asserted `.toContain("receive")`,
  `.not.toContain("fallback")`, `.not.toContain("denied")` `:87-99`. **NOT `"proved"`** — WASM fallback also
  emits `proved` (`b4-impl.md:140`), so it can't discriminate.
- Ports are single-sourced: `core/src/server.rs:32-33` (`PORT 59833`, `HTTPS_PORT 59834`). COOP/COEP come from
  `playground/vite.config.ts:154-158` — OS-independent.
- Windows would need NO xvfb/tray/dbus: `accelerator.yml:635-790` already launches this tray app on
  `windows-latest` and polls `/health` with zero virtual-display setup (WebView2 runs unattended).

## 5. Windows prior art to reuse (install / launch / cleanup)

- Silent NSIS install is ASYNC: `updater-smoke-windows.ps1:226-231`
  (`Start-Process -ArgumentList "/S" -PassThru` + `WaitForExit(120000)` + `Kill()` on timeout; a non-silent
  prompt would hang the runner forever). Simpler `-Wait` form at `accelerator.yml:691`.
- Install dir `%LOCALAPPDATA%\Aztec Accelerator\` (`installMode: "currentUser"`, `tauri.conf.json:71-73`);
  exe found via `Get-ChildItem -Recurse -Filter AztecAccelerator.exe` — no PATH registration.
- **Defender**, not SmartScreen, is the surface actually handled: every install step first does
  `Add-MpPreference -ExclusionPath` (`accelerator.yml:686-688`, `updater-smoke-windows.ps1:169`).
  Required additive step with no Linux/macOS analogue.
- `AZTEC_ACCEL_NO_UPDATE=1` (`accelerator.yml:706`) stops the launched app polling the prod feed in CI.
- **Ordering gotcha**: disarm crash-recovery BEFORE any `Stop-Process -Force`, else Task Scheduler treats the
  kill as a crash and relaunches mid-assertion (`accelerator.yml:723-729`).
- pwsh steps auto-append `exit $LASTEXITCODE`; a trailing native non-zero (e.g. `schtasks /Delete` on an
  absent task) needs an explicit `exit 0` (`accelerator.yml:786-790`).
- Shell convention: bash (Git Bash) for portable glue, `pwsh` only for cert-store/registry/schtasks steps
  (`_e2e-webdriver.yml:27-32`, whose Windows cleanup uses `taskkill //F //IM AztecAccelerator.exe`).
- `windows-latest` drifts; `_e2e-crash-recovery-windows.yml:5-7,21` pins `windows-2025` (targeted, not repo-wide).
- **Playwright on Windows is UNPROVEN in this repo** — `playwright-cache/action.yml:6` is Linux-only, macOS
  works around it with a direct `bunx playwright install chromium`; no Windows Playwright usage exists anywhere.

## 6. Uninstall: what a real Windows install leaves behind (the assertion surface)

| Artifact | Identifier | Source |
|---|---|---|
| Install dir | `%LOCALAPPDATA%\Aztec Accelerator\` | `update_marker.rs:546-548`, `tauri.conf.json:71-73` |
| Main exe / legacy exe | `AztecAccelerator.exe` / `aztec-accelerator.exe` | `update_marker.rs:536-544` |
| bb sidecar | `bb-x86_64-pc-windows-msvc.exe` next to the exe | `tauri.conf.json:46-48`, `core/src/bb.rs:53-58` |
| Autostart | HKCU `...\CurrentVersion\Run`, value **"Aztec Accelerator"** (quoted abs path) | `autostart.rs:54,976-981` |
| StartupApproved | HKCU `...\Explorer\StartupApproved\Run` — **deliberately never deleted** | `autostart.rs:1094-1096` |
| Crash recovery | Scheduled task **"Aztec Accelerator Crash Recovery"**, PT1M relaunch trigger | `crash_recovery.rs:488-489,604-643` |
| Trust anchor | CurrentUser `Root`, CN **"Aztec Accelerator Local CA"** | `certs.rs:118`, `trust/windows.rs:23-24` |
| Removed | `%USERPROFILE%\.aztec-accelerator\certs\` ONLY | `uninstall.rs:216-233`, `certs.rs:14-19` |
| KEPT (assert survival) | `config.json`, `versions\` (bb cache), `*.lock`, update markers, `updater-state.json`; and `%LOCALAPPDATA%\aztec-accelerator\{logs,prove-tmp}` | `uninstall.rs:227`, `README.md:294-305`, `core/src/lib.rs:23-32` |
| ARP/uninstall registry key | stock bundler template (`$MANUPRODUCTKEY`) — **not vendored; verify empirically** | `update_marker.rs:554-558` |

B5 structure: **primary** = NSIS `PREUNINSTALL` -> `ExecWait "...AztecAccelerator.exe --prepare-uninstall"`
(`hooks.nsi:101-120`), guarded by `$UpdateMode <> 1` AND `$EXEDIR != $INSTDIR`; **belt** = `POSTUNINSTALL`
native-inline, ownership-checked redo of certutil delete + Run-value delete + owned-recovery-task delete
(`hooks.nsi:271-443`), idempotent by design (`uninstall.rs:20-21`). Ownership gate is three-state
(`ConfirmedOurs`/`NoEntry` proceed; `ForeignOrUncertain` -> leave everything, still exit 0) `uninstall.rs:131-163`.

**Existing Windows coverage (do NOT duplicate)**: `accelerator.yml:127-333` `cert-trust` runs `trust_windows`
(read paths only), `autostart_heal` (real HKCU lifecycle via the library API), `uninstall_ownership`
(`recovery_ownership` round-trip only) and a **synthetic** NSIS hook harness driving a STUB exe.
`accelerator.yml:635-790` `windows-build` really installs + launches but **never runs the uninstaller**.
**Net gap** = real installer + real `uninstall.exe /S` + broad post-assertions — which is exactly backlog #61
("live Windows NSIS belt harness", scoped at `v2-release-train/lessons/b5.md:100-105` as a RELEASE-gate item).

## 7. Collision / dedup risks

- Add 2 STANDALONE named jobs, not a `strategy: matrix` — matrix interpolation would rename the check names
  that branch protection may pin.
- Keep jobs unconditional at job level; gate `release_tag != ''` per-step (existing convention).
- Failure-log artifact names are per-job unique (`packaged-e2e-linux-logs`, `-macos-logs`) — a copy-paste
  collision would shadow within a run. `uninstall-linux` has no log upload; parity ⇒ none needed.
- `stage-installers`'s missing Windows `--pattern` is a SINGLE shared mutation point — add once.
