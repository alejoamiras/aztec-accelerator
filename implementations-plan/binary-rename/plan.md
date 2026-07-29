# Plan — mainBinaryName "AztecAccelerator" + bundle metadata (rename piece)

`/blueprint light`. Final piece of the autostart arc (pieces 1–3 merged: #422/#423/#425). The exe
renames `aztec-accelerator` → `AztecAccelerator` (no space — the space was the old plan's chief
hazard and the goal settled against it); `productName` stays "Aztec Accelerator", so install dir,
bundle/artifact FILENAMES, shortcuts, config dir, identifier, and log dirs are all UNCHANGED.
Bundle metadata folds in (it is tauri.conf-only). Prior art: `pre-release-polish/plan.md` Phase 3
(site categories reused; its line numbers and its spaced name are dead).

## Facts (verified today, this worktree @ 80af700)

1. **Every release ever shipped (v1.0.0 … v1.0.7) and HEAD build with @tauri-apps/cli 2.10.1**
   (bun.lock at each tag). The bundler's NSIS template saves `MainBinaryName` in UNINSTKEY at
   install and DELETES the old-named exe when it changes (`installer.nsi:666-674`, fetched at
   tauri-bundler-v2.8.1) — so on the first renamed upgrade, EVERY existing install deletes
   `aztec-accelerator.exe` and lands `AztecAccelerator.exe` in the SAME dir. No stale-exe /
   update-loop hazard anywhere in the install base.
2. **The rename-boundary autostart story is already built** (the arc's thesis): old Run value →
   deleted old exe → non-resolving → piece-1 heal rewrites to `current_exe()` at N's first
   launch → piece-1 rearm. The CALL-path positive smoke (real v1.0.7 N−1, #96 arming) will prove
   exactly this on the next release run, for free. Marker interplay: v1.0.7 writes no marker; if
   a marker-bearing release ever ships pre-rename, piece-2's rename-tolerance clause
   (`try_exists()==Ok(false)` proven-absent) admits removal because fact-1 deletes the old exe.
3. **`_e2e-webdriver.yml` builds with PLAIN `cargo build`** (`:66` release+webdriver, `:68` dev —
   deliberate, F-012 root cause) — plain cargo output keeps the CARGO bin name; only
   `tauri build` applies mainBinaryName renaming (old plan measured it as a MOVE in target/).
   ⇒ conf-only mainBinaryName would fork the binary name BY BUILD MODE and break two of three
   webdriver modes (the old plan's Phase 3 would have shipped this bug).
4. `main.desktop` templates `Exec=env GDK_BACKEND=x11 {{exec}}` / `StartupWMClass={{exec}}` — no
   literal, and no-space name ⇒ no quoting work.
5. NOT rename sites (verified): `release-accelerator.yml:1142` (CARGO CRATE names — unchanged),
   `build-test-bundle.yml:71` (artifact label), `updater-smoke-linux.sh` (AppImage FILE names,
   productName-based; its pkill anchors on `\.AppImage`), `crash_recovery.rs:760` (arbitrary test
   path), hooks.nsi (fully `${MAINBINARYNAME}`-templated), all `.aztec-accelerator` config-dir /
   identifier / log-dir / `aztec-accelerator.dev` refs.
6. Bundle metadata today: no `publisher` (Windows renders the identifier-fallback "aztec"), no
   copyright/licenseFile/category/homepage. `LICENSE` exists at repo ROOT only
   (`../../../LICENSE` from src-tauri/).
7. Current call-path smoke launches N−1 = real v1.0.7 (OLD-name exe) and its cleanup uses
   `Get-Process -Name "aztec-accelerator"`; macOS `updater-smoke.sh:38` hardcodes
   `Contents/MacOS/aztec-accelerator` for the N−1 .app. Post-rename these must resolve BOTH names
   (old for N−1 fixtures, new for N) — `pkill -f "aztec-accelerator"` does NOT match
   "AztecAccelerator" (case-sensitive).

## Design

**Core call: rename at the CARGO layer, mirror in conf.** `Cargo.toml` gets
`[[bin]] name = "AztecAccelerator", path = "src/main.rs"` (crate name UNCHANGED — keeps
Cargo.lock, the release-version sed at fact 5, and `use aztec_accelerator::…` imports intact),
plus `tauri.conf.json` `"mainBinaryName": "AztecAccelerator"` (explicit, self-documenting, and
what the NSIS `${MAINBINARYNAME}` define reads). Every build path — plain cargo, tauri dev,
tauri build — now produces ONE name; no per-mode forks. Simpler alternative rejected: conf-only
mainBinaryName (fact 3 — mode-dependent breakage).

**Lockstep sites (one commit):**
- `_e2e-webdriver.yml:91-93` APP_CMD ×3 → `AztecAccelerator$EXE`; `:158` taskkill `//IM
  AztecAccelerator.exe`; `:163` pkill pattern → `AztecAccelerator`
- `accelerator.yml:617,619` windows-build heal-scenario exe lookup → `AztecAccelerator.exe`
- `smoke-updater-windows.yml:112` sentinel `$INSTDIR\AztecAccelerator.exe` (dispatch = both ends
  current-ref ⇒ new name only)
- `updater-smoke-windows.ps1`: `:220` installed-exe lookup and `:345` Q lookup → dual-name
  (`-Include "AztecAccelerator.exe","aztec-accelerator.exe"`, prefer new) — the call path's N−1
  is OLD-named until a renamed release becomes the fixture; `:82` cleanup kills BOTH process
  names; `:371` stale path cosmetic rename
- `updater-smoke.sh:38` APP_BIN → resolve via find/glob in `Contents/MacOS/` (dual-name, same
  reason)
- `release-accelerator.yml:250` bundle-shape `EXPECTED` → `AztecAccelerator\nbb`; `:404` DMG find
  → `AztecAccelerator`; `:452` pkill → cover both names
- `UPDATER_TESTING.md:15` doc tuple
- **NEW `packages/accelerator/scripts/tauri-identity.test.ts`** — drift guard pinning the
  identity tuple from tauri.conf.json + Cargo.toml: mainBinaryName == Cargo `[[bin]]` name ==
  "AztecAccelerator", productName "Aztec Accelerator", identifier unchanged, publisher/copyright/
  licenseFile/category/homepage present. Cheap TS test in the existing bun:test suite; catches
  any future half-rename in milliseconds on every platform.

**Bundle metadata (same conf edit):** `publisher: "Aztec Accelerator"`, `copyright: "© 2026
Aztec Accelerator contributors"`, `licenseFile: "../../../LICENSE"` (verify bundling accepts the
out-of-package path; fall back to copying LICENSE into src-tauri/ if not), `category:
"DeveloperTool"`, `homepage: "https://aztec-accelerator.dev"`.

## Explicitly NOT in scope
productName/identifier changes; crate rename; any product .rs change (everything runtime uses
`current_exe()` — grep-verified, the one literal is a test fixture string); updater-feed /
artifact naming (productName-based).

## Gates
1. Local: `bun run test` (incl. the new identity test) + `bun run lint:actions` +
   `cargo clippy --all-targets` (piece-2 lesson) + shellcheck-clean.
2. PR → full CI: the three webdriver legs (all OS) EXECUTE the renamed binary (fact 3 makes them
   real proof); windows-build heal scenarios exercise NSIS + heal under the new name.
3. Post-merge: dispatch burn-in `smoke-updater-windows.yml` (barrier) on main — marker lifecycle
   + PREINSTALL sentinel under the new name.
4. Residual (stated): the v1.0.7→renamed-N CALL-path boundary is only executable on a release
   run (workflow_call). The next release's positive leg IS the boundary proof (fact 2); the
   4-point preflight guards fixture rot until then.
5. Review loop per the goal: ONE codex audit of this plan → fold → implement → PR CI green →
   ONE post-impl codex audit → fix → ONE resumed verify → merge.

## Asks
None. (Light floor: 7 verified Facts; no silent Asks.)

## Audit fold (codex approve-with-changes, session `019faf31-78d8-7941-8db7-42484ce5a8a1`)

1. **Publisher DEFERRED (blocking, adopted).** `publisher` feeds NSIS `${MANUFACTURER}` (today the
   identifier-fallback "aztec"), which namespaces `Software\<manufacturer>\<productName>` — the key
   the installer uses to restore a custom `$INSTDIR` and to locate the old uninstaller
   (`installer.nsi:331,658`). Changing it in the SAME release as the exe rename would strand
   custom-directory installs (new install in default dir, old exe left alive → resolve-heal keeps
   booting the old version). Publisher ships in a LATER release with a migration story; the
   "Publisher: aztec" cosmetic stays for now. Residual, documented.
2. **Cargo `default-run` must move too** (`Cargo.toml:7`) — and conf `mainBinaryName` is OMITTED:
   tauri applies it as an unconditional filesystem rename after resolving the cargo main target,
   so bin-name == mainBinaryName risks a same-path rename; the bundler derives
   `${MAINBINARYNAME}` from the built binary when the conf key is absent. Verified locally via
   `tauri build --debug --no-bundle` before pushing (gate 1 addition).
3. **Boundary asserts strengthened + names plumbed, not dual-globbed**: ps1 gains
   `-N1BinaryName` (call path passes `aztec-accelerator.exe` while the fixture is v1.0.7; both
   workflows pass explicitly). Post-update the positive tail now asserts: NEW-name exe exists;
   OLD-name exe ABSENT when names differ (the `installer.nsi:666-674` delete, observed); Run
   value == quoted new exe path (heal-or-unchanged — same assert both paths).
4. **Missed sites folded**: `autostart.spec.ts:148` (basename assert — would fail PR CI),
   `README.md:292`, `PLATFORM_SUPPORT.md:29`, and `accelerator.yml` `desktop` paths-filter gains
   the workflows the identity guard greps (`_e2e-webdriver.yml`, `release-accelerator.yml`,
   `smoke-updater-windows.yml`, `_e2e-updater-windows.yml`) so edits there re-run the suite.

Non-blocking folded: `bundle.license` SPDX (`AGPL-3.0-only` per LICENSE) alongside licenseFile;
`/S` suppresses the NSIS license page (silent updates unaffected); post-build inspect
CFBundleExecutable + desktop Exec/StartupWMClass. Identity test pins the tuple INCLUDING
`default-run` and new-name presence in the five grep-bearing CI/script files.
