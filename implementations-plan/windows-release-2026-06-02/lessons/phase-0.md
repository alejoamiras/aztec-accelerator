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

### Result
_(pending the windows-latest run — PR open; will record: --version output, the full DEPENDENTS list, and the SHA-256 to pin.)_
