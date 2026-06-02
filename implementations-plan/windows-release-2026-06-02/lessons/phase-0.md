# Phase 0 — bb.exe spike

**Goal:** convert opus's off-runner bb.exe inspection into a fact proven on the real
`windows-latest` image, before any app code changes. Gate everything else on it.

## Local research (pre-CI, done from macOS — no Windows needed)

- **Installed `@aztec/bb.js` = `4.2.0`** (bun.lock pin + the package's own `package.json` `version` field). This is the version the macOS/Linux sidecar already ships, so the Windows `bb.exe` must come from the matching **`v4.2.0`** aztec-packages release to stay version-consistent.
- **GOTCHA — `src-tauri/AZTEC_VERSION` is STALE:** it's committed as `4.2.0-aztecnr-rc.2`, but the installed bb.js is `4.2.0`. `copy-bb.ts` *generates* this file at prebuild from the live `bb.js` `package.json.version`, so the committed value drifted (last regenerated when an rc was pinned). **Implication for P1:** derive the Windows release tag from the **live** `bb.js` `package.json` version (`v${version}`), **never** from the committed `AZTEC_VERSION` file — that file would point at the wrong tag (`v4.2.0-aztecnr-rc.2`, a *different* build: 4,849,906 B vs 4,849,818 B).
- **npm `@aztec/bb.js/build/` has no Windows variant** — only `amd64-linux`, `amd64-macos`, `arm64-linux`, `arm64-macos`. Confirms the npm package can't supply Windows bb; fetching the GitHub release tarball is mandatory (as the plan assumed).
- **Tarball URL confirmed (HEAD 200 from macOS):**
  `https://github.com/AztecProtocol/aztec-packages/releases/download/v4.2.0/barretenberg-amd64-windows.tar.gz` → 200, **4,849,818 bytes**. (The `v4.2.0-aztecnr-rc.2` tag also has the asset, 4,849,906 B — do not use it.)

## CI spike (windows-bb-spike.yml on windows-latest)

Throwaway workflow, `permissions: {}`, no checkout (self-contained). Proves on the runner:
1. tarball downloads + extracts (`tar -xzf`, bsdtar is built into Win10+),
2. `bb.exe --version` exits 0 (executes at all),
3. `dumpbin /DEPENDENTS` (located via `vswhere`) shows only Win10+ system DLLs — **no `VCRUNTIME140`/`MSVCP140`** ⇒ no VC++ redist needed,
4. captures the tarball **SHA-256** to pin as the supply-chain checksum in `copy-bb.ts` (P1).

Why dumpbin and not just `--version`: the runner has the VC++ redist pre-installed, so `--version` succeeding there does NOT prove a *bare* user machine can run it. The import table is the real proof.

### Result — PASS (PR #267, run 26839873907, windows-latest = Windows Server 2025, 13s)

- **`bb.exe` is the ONLY file in the tarball** — `D:\...\bb.exe`, **21,731,840 bytes** (~20.7 MB). No stowaway `.dll` shipped beside it ⇒ nothing extra to bundle/manage in the sidecar dir.
- **`bb.exe --version` → exit 0**, prints **`4.2.0`** — matches `@aztec/bb.js@4.2.0` exactly. Confirms the `vN` release tag and the npm package are the same bb build (version-consistency invariant holds).
- **`dumpbin /DEPENDENTS` — 100% system DLLs, no redist:**
  `WS2_32`, `KERNEL32`, `SHELL32`, `PSAPI`, `bcrypt` + the 12 `api-ms-win-crt-*-l1-1-0` UCRT forwarders. **No `VCRUNTIME140`/`MSVCP140`/`MSVCR*`** ⇒ statically linked (zig-static); the only runtime is the Universal CRT, present on every Windows 10+ / Server 2019+. **A bare user machine needs nothing extra.**
- **Tarball SHA-256 (pin this in `copy-bb.ts` P1):**
  `55043d74d20afd55cb3d3c5fd690b79f9d964ba52bfebd13bcba71b74a3d0c8f`

**Verdict:** the #1 feared Windows risk (DLL hell / VC++ redist) is eliminated, proven on the actual CI image — not just opus's local box. `bb.exe` is a drop-in self-contained sidecar. **Proceed to P1** (the `copy-bb.ts` win32 fetch branch + checksum pin).

**Carry-forward for P1:**
- Derive the tag as `v${bbJs.version}` from the LIVE `@aztec/bb.js` `package.json` (= `v4.2.0`), never from the stale committed `AZTEC_VERSION`.
- Pin the SHA-256 above; fail the prebuild on mismatch (supply-chain gate).
- Sidecar dest triple = `x86_64-pc-windows-msvc`, so write `binaries/bb-x86_64-pc-windows-msvc.exe` (note the `.exe` — Tauri sidecar resolution on Windows expects the extension).
