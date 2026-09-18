# Recon — pre-release-polish

Phase 0.4 codebase recon. Three read-only sweeps against `main` @ `80c6f4c`. Every claim below was
verified by reading files (and, for the Tauri mechanics, the pinned crate sources in
`~/.cargo/registry` — `tauri-utils 2.9.2`, `tauri-plugin-updater 2.10.1`, `tauri 2.11.0`), not
inferred from names.

This file feeds the draft AND every audit. Audits are asked explicitly: *does the design duplicate or
ignore what recon found reusable?*

---

## Part A — Authorization popup ("allow once" removal)

### A1. What exists

| File | Purpose |
|---|---|
| `packages/accelerator/src-tauri/frontend-src/authorize.js` | Real popup source (ES module). The only editable copy. |
| `packages/accelerator/src-tauri/frontend/authorize.html` | Popup markup (tracked). |
| `packages/accelerator/src-tauri/frontend/assets/authorize.js` | **Generated**, gitignored (`src-tauri/.gitignore:6`). `build.rs` SHA-256-checks it against `frontend-src/` and FAILS the Rust build if stale. Regenerate with `bun run --cwd packages/accelerator frontend:build`. |
| `packages/accelerator/src-tauri/frontend-src/bridge.js` | `invoke`, `isClickGuardActive()` (700 ms post-focus), `showErrorHint`, `wireButton`. |
| `packages/accelerator/src-tauri/src/commands.rs:130-174` | `respond_auth(window, app, auth, request_id, origin, allowed, remember)`. |
| `packages/accelerator/core/src/server/auth.rs` | `authorize_origin()` — the real gate. NOT `src-tauri/src/server/auth.rs` (doesn't exist; `src-tauri/src/server.rs` is a thin `pub use accelerator_core::server::*` wrapper + the HTTPS lifecycle lock). |
| `packages/accelerator/core/src/authorization.rs` | `AuthorizationManager`, `AuthDecision`, piggyback + active/queued arbiter. |
| `packages/accelerator/core/src/config.rs` | `approved_origins: Vec<CanonicalOrigin>`, `lock_mutate_save`. |

Current markup (`authorize.html:27-38`) — checkbox ships `disabled` (a codex fix: a pre-JS click
can't pre-check a disabled input); `authorize.js` enables it only once the popup is active and past
the click guard:

```html
<label class="popup-remember">
  <input type="checkbox" id="remember" disabled />
  Always allow this site
</label>
...
<button class="btn btn-secondary" id="deny">Deny</button>
<button class="btn btn-primary"   id="allow">Allow once</button>
```

### A2. The finding that matters most: "Allow once" is not a session

`core/src/server/auth.rs:89-115`:

```rust
AuthDecision::Allow { remember } => {
    if remember { /* push origin into cfg.approved_origins, save */ }
    Ok(())
}
```

When `remember == false` this function does **nothing** beyond returning `Ok(())` for the requests
currently blocked on the decision. There is no in-memory grant table, no session object, no TTL
cache. `PendingState` only tracks requests *awaiting* a decision and drops the entry the moment it
resolves.

The only reason "Allow once" covers more than one HTTP request is the **piggyback**
(`authorization.rs:318-344`): concurrent `/prove` calls from the same origin that arrive *while the
popup is open* share the request id and therefore the decision. Once the popup closes, the very next
`/prove` — a second later — re-enters `authorize_origin`, finds `is_approved == false`, and opens a
fresh popup.

So the accurate description is **"approved for the request(s) in flight when you clicked"**, not a
session. `README.md:126` currently claims *"approved for this session only"* — **that is wrong today,
independent of this plan**, and overstates the grant's lifetime.

### A3. This reverses an audited decision (must be documented, per the task)

- `implementations-plan/security-hardening/clusters/C9-plan.md:16,29-30,59` and
  `audit/security/2026-07-09-5c788c0/raw/frontend-trust-ui-claude.md:32,52`.
- Pre-F-014 the popup had **Remember checked by default**. The audit flagged it:
  > *"Authorization: violated — a malicious origin can obtain a persisted (remember is checked by
  > default) grant… 'Remember' defaulting to checked removes the natural rate-limit of re-prompting
  > on each request."*
- F-014 (PR #392, merged `90f9573`) fixed it by defaulting **unchecked** + the "Allow once" primary /
  "Always allow" opt-in split, with the stated threat model: *"the user Allows a malicious look-alike
  origin… believing it's trusted; or grants accidental persistent trust."*

Our change does not restore the pre-F-014 state exactly — it is stricter in one way (no checkbox to
misread) and looser in another (no ephemeral option at all). But the property the audit objected to,
*an accidental Allow becoming a permanent grant*, returns.

**Counter-argument the audit did not have**: what shipped is not a rate-limit, it is a prompt storm —
re-prompting on the *next proof* trains click-through habituation, or drives the user to tick "Always
allow" out of irritation, reaching the same permanent grant by a worse path. Prompt fatigue is itself
a documented failure mode. The compensating control (Settings → Approved Sites → Remove) exists and
is tested.

**Consequence for the design: once Allow is permanent, the origin display IS the entire defense.**
`authorize.html`'s F-014 origin line (full canonical origin, never truncated, `dir=ltr` + bidi
isolation, punycode never decoded) and the verified-sites badge become load-bearing, not decorative.

### A4. Reuse as-is (no changes)

- `packages/accelerator/server/` (headless) — `AppState::headless` sets `show_auth_popup: None`, so
  `auth.rs:57-61` short-circuits before the `Allow{remember}` branch is reachable. The headless
  binary can **never** exercise remember. Zero impact.
- `src-tauri/capabilities/authorize.json` + `WINDOW_MATRIX.authorize` in
  `scripts/tauri-trust-boundary.test.ts:155` — command **names** only, indifferent to parameter
  shape. The `handlers.length === 18` pin stays green as long as no command is added/renamed.
- Revocation surface, already complete and tested: `remove_approved_origin`
  (`commands.rs:80-91`, settings-window-gated), `settings.html:12-19`, `settings.js:77-106`,
  `e2e/settings.spec.ts:79-109`, WebDriver `removeTestOriginViaUI()`.
- `core/src/config.rs` storage/save/load, `lock_mutate_save` — only the *caller's* conditional changes.
- `bridge.js` click-guard / `wireButton` — Allow/Deny still need the guard; nothing remember-specific.
- `AuthorizationManager` arbiter (piggyback, promotion, 60 s backstop) — orthogonal.

### A5. Adapt-with-changes

- `authorize.html:27-38` — delete the `<label class="popup-remember">` block; `"Allow once"` → `"Allow"`.
- `authorize.js` — delete `rememberEl` (L12), its init-clear (L26), its bespoke click-guard listener
  (L32-34), the `rememberEl.disabled` line in `setControlsEnabled` (L41), and the read at L108.
- **Fork point for the plan** — `commands.rs:130-174`:
  - **(a)** keep the `remember: bool` param, frontend always sends `true`. Zero Rust/enum churn,
    smallest diff, but a permanently-true wire parameter.
  - **(b)** drop the param; `commands.rs` constructs `AuthDecision::Allow { remember: true }`.
    Smaller IPC surface, but if the *enum* shape changes too, 5 direct construction sites need
    updating (`authorization.rs:503,524,625,633`, `server/tests.rs:680`).
- `README.md:121-132,375` — rewrite the prose model (and fix the false "session" claim, A2).

### A6. Tests that must change

| File:line | Asserts |
|---|---|
| `e2e/authorize.spec.ts:45-60` | Allow payload is exactly `{…, allowed:true, remember:false}` |
| `e2e/authorize.spec.ts:62-75` | Deny payload includes `remember:false` |
| `e2e/authorize.spec.ts:77-94` | checking "Always allow" sends `remember:true` |
| `e2e/authorize.spec.ts:105-112` | a QUEUED popup keeps `#remember` disabled |
| `e2e-webdriver/auth-flow.spec.ts:175-214` | real config.json gains the origin after Allow+remember |
| `e2e-webdriver/auth-flow.spec.ts:241-268` | **"allow without remembering"** — origin NOT persisted. Premise disappears entirely. |

**Coverage gap to close in this plan**: there is NO fast Rust test driving `Allow{remember:true}`
through `authorize_origin` and asserting `approved_origins` actually grew. Only the slow WebDriver
test proves that pipeline. `core/src/server/tests.rs:815` (`prove_approves_remembered_origin`) is
misleadingly named — it pre-seeds config and asserts no popup fires; it never exercises the write.
Add an axum `oneshot` test modelled on `prove_triggers_popup_for_unknown_origin` (L664-702).

### A7. Collision / dedup risks

- **`.popup-remember` CSS (`style.css:484-495`) is SHARED** with `update-prompt.html:14`
  ("Keep me updated automatically"). Delete the *use* in authorize.html, **not** the class.
- `respond_auth`'s `remember` is currently computed and sent on the **Deny** path too
  (`authorize.js:108,111`) and silently discarded Rust-side (`commands.rs:150`). A naive checkbox
  deletion leaves that call site without a value.
- `authorize.json:4`'s description says "ONLY the two commands" while granting three — pre-existing
  stale wording; don't propagate it.

---

## Part B — `mainBinaryName` + bundle identity

### B1. How the name is derived today

`Cargo.toml`: `[package] name = "aztec-accelerator"`, `default-run`, `autobins = false`, a single
`[[bin]] name = "aztec-accelerator"`, **no `[lib]`**. `tauri.conf.json` has **no** `mainBinaryName`.

Per `tauri-utils-2.9.2/src/config.rs:3595-3608`, Tauri defaults the main binary to cargo's output and
`mainBinaryName` renames it in `tauri build` / targets `tauri bundle` at it. **`Cargo.toml` needs no
change** — this is a post-`cargo build`, pre-bundle CLI step.

### B2. What the rename actually breaks — the finite list

| # | File:line | Effect |
|---|---|---|
| 1 | `.github/workflows/accelerator.yml:575,577` — `Get-ChildItem -Filter "aztec-accelerator.exe"` after a real `tauri build --bundles nsis` | **Fails on the introducing PR itself.** Must be fixed in the same PR. |
| 2 | `.github/workflows/release-accelerator.yml:250` — `EXPECTED="$(printf 'aztec-accelerator\nbb\n' \| sort)"` bundle-shape invariant | Fails the first release build, before smoke/signing. This guard exists *because of* the 1.0.0→1.0.1 stowaway-binary bug; team convention is to update it by hand when the bundle shape intentionally changes. |
| 3 | `.github/workflows/release-accelerator.yml:404` — `find … -name "aztec-accelerator" -path "*/MacOS/*"` in DMG smoke | `smoke` gates `tag`/`release`. |
| 4 | `packages/accelerator/scripts/updater-smoke-windows.ps1:191` — `Get-ChildItem -Filter "aztec-accelerator.exe"` | **Highest severity.** `_e2e-updater-windows.yml`'s N-1 is a **synthetic 0.0.1 built from the current checkout** (only the version is patched), so N-1 also carries the NEW name. Blocks `tag`+`release` for **all four platforms**, and means the Windows rename-across-upgrade path ships with **no CI proof**. |

Low-consequence (all `|| true` / `SilentlyContinue`): `release-accelerator.yml:452`,
`_e2e-webdriver.yml:158,163`, `updater-smoke-windows.ps1:61`.

### B3. RESOLVED BY MEASUREMENT — it is a MOVE (break #5)

`_e2e-webdriver.yml:81-92` (`built-debug` mode) runs a **real** `bunx tauri build --debug --no-bundle`
then launches a hardcoded `./src-tauri/target/debug/aztec-accelerator$EXE`. `--no-bundle` skips
bundling but not the CLI's post-build rename.

Measured on this machine (Linux, tauri-cli 2.10.1) by setting `mainBinaryName: "Aztec Accelerator"`
and running `bunx tauri build --debug --no-bundle` with a dedicated `CARGO_TARGET_DIR`:

```
target/debug/
  -rwxrwxr-x  2  393304344   Aztec Accelerator     ← renamed binary (hardlink, count 2)
  -rw-rw-r--  1      16184   aztec-accelerator.d   ← dep-info leftover only
```

**No `aztec-accelerator` executable survives the rename.** So this is a hard break, not a silent
coverage gap: the `built-debug` WebDriver leg 404s on `APP_CMD`. It is a **PR-gate** job, so like
break #1 it fails on the introducing PR and must be fixed in the same commit.

(`dev`/`release` modes use raw `cargo build`, which never reads `tauri.conf.json` — confirmed
unaffected. Only `built-debug` goes through the CLI.)

### B4. What is NOT affected (keeps the plan scoped)

- **macOS auto-update across a rename is upstream-solved.** `tauri-2.11.0/src/process.rs:74-130`
  `restart_macos_app()` reads the freshly-swapped `Contents/Info.plist`'s `CFBundleExecutable`
  (which the bundler sets from `mainBinaryName`) — the comment says so explicitly: *"on macOS on
  updates the binary name might have changed."* `install_inner` swaps the whole `.app` directory
  atomically; `extract_path` derives from the *running* process, never from N's new name.
- **NSIS upgrade is clean**: N-1's own uninstaller was compiled with N-1's `${MAINBINARYNAME}`, so it
  deletes the right file; `$INSTDIR` keys off identifier/productName (unchanged). `hooks.nsi` has
  **zero** `${MAINBINARYNAME}` references — it keys on `$UpdateMode` / `$EXEDIR`≠`$INSTDIR` and the CA's CN.
- **Linux**: updater targets `$APPIMAGE` env var, not an internal name.
- **Artifact naming + signature verification**: `productName`-derived and envelope/version-keyed
  respectively. `release-accelerator.yml:729-822`'s 16-file `EXPECTED` list contains no binary name.
- `updater-smoke.sh` (launches a **real downloaded N-1** that predates the rename — old name still
  correct) and `updater-smoke-linux.sh` (`APP_BIN` is a script-chosen local filename).
- `crash_recovery.rs` binary-name strings are unit-test fixtures; its real `ExecStart` comes from
  `std::env::current_exe()` at arm time.
- `accelerator-core` / `accelerator-server` crates; all `~/.aztec-accelerator` path literals; the CA
  CN `"Aztec Accelerator Local CA"`; the Task Scheduler name; `main.desktop` (Handlebars);
  `wdio.conf.ts` (TCP only); `bump-source`'s Cargo.lock `sed` (package name, unchanged).

### B5. Signing reality

macOS **is** fully codesigned + notarized in CI (`APPLE_*` secrets on the `build` job), re-verified in
`smoke` via `codesign --verify --deep --strict` + `xcrun stapler validate`. Windows is **not**
Authenticode-signed — the release-notes template says so verbatim (`release-accelerator.yml:927-932`).
Update payloads are minisign-signed separately (version/envelope-keyed, name-independent).

---

## Part C — Professionalization inventory

### C1. Already correct — DO NOT churn on these

- `icon.icns`: complete retina ladder `ic04…ic10`, 16→1024px (ICNS chunk table parsed directly).
- `icon.ico`: 7 images, 256/128/96/64/48/32/16 @32bpp (ICONDIR parsed directly).
- **Tray icon is a correct macOS template image**: every non-transparent pixel in `tray-idle.png`,
  `tray-proving-1.png`, `tray-proving-12.png` is exactly `(0,0,0)` (raw buffers decoded);
  `icon_as_template(true)` at `tray.rs:127` and re-asserted every frame at `tray.rs:154,165`.
- Version IS user-visible: `tray.rs:79-86` disabled item `v{app_version} · Aztec {bb_version}`, in
  both dev and production menus.
- Zero TODO/FIXME/placeholder/lorem/console.log in the shipped frontend (repo-wide grep).
- Product naming consistent everywhere rendered: window titles (`windows.rs:108,128,157,202,257`),
  tray tooltip (`tray.rs:128`), landing page, HTML `<title>`s.
- `dev.aztec.accelerator` appears in exactly ONE file — `tauri.conf.json` itself. No leak.
- macOS LaunchAgent filename is `"Aztec Accelerator"` (`crash_recovery.rs:126-129`, with a comment
  explaining it must match productName). Linux systemd id is kebab by necessity.
- `tauri.conf.json` version == `Cargo.toml` version (`1.0.8-rc.1`) — the two that ship agree.
- `main.desktop`'s `Comment=` is NOT blank — `shortDescription` falls back to `Cargo.toml`'s
  `description`, which is set.
- NSIS `installMode: "currentUser"` is a deliberate no-UAC choice.

### C2. Real gaps

| Sev | Gap | Where a user sees it | Fix location |
|---|---|---|---|
| Med | `publisher` unset AND `Cargo.toml` has no `authors` → tauri-bundler falls back to `bundle_identifier.split('.').nth(1)` = **`"aztec"`** | Windows *Apps & Features* → Publisher; `apt show` → Maintainer | `tauri.conf.json` `bundle.publisher` |
| Med | `licenseFile` unset though the repo is **AGPL-3.0-only** (root `package.json:3`) → NSIS installer has **no license page** | Windows install flow | `bundle.licenseFile` |
| Low-Med | `category` unset → `Categories=` blank in the `.desktop` (both `.deb` and AppImage — AppImage reuses `debian::generate_data()`) | Linux app-menu placement | `bundle.category` |
| Low | `homepage` unset though `aztec-accelerator.dev` is already used in the same file (`plugins.updater.endpoints`) | `apt show` omits `Homepage:` | `Cargo.toml` `homepage` or `bundle.homepage` |
| Low | `copyright` unset (no fallback exists, unlike publisher/shortDescription) | NSIS `BrandingText` footer blank; installer `.exe` Properties → Copyright blank | `bundle.copyright` |
| Low | `icons/256x256.png`, `icons/512x512.png` on disk but absent from `bundle.icon` | Linux hicolor theme scales instead of using exact sizes | `tauri.conf.json:41-47` |
| Low | No version string in the **Settings** window (the window users actually open) | `settings.html` | `settings.html` + `settings.js` |

**Hypothesis worth testing during implementation**: the owner reported the macOS "App Background
Activity" dialog showing *both* the lowercase name *and* no icon. Since the icon assets are complete
(C1), both symptoms likely share one cause — that dialog reads the **binary's** identity. Setting
`mainBinaryName` may fix both. Verify on the owner's Mac; do not assume.

### C3. Descriptions disagree

`Cargo.toml:4` = `"Native proving accelerator for Aztec transactions"`;
`packages/accelerator/package.json:5` adds `" — bypasses browser WASM throttling"`.
**Cargo.toml's is the one that reaches users** (via the `shortDescription` fallback into the Linux
`.desktop` `Comment=` and the `.deb` `Description:`), so treat it as canonical.
`packages/accelerator/package.json` version is `0.0.0` — private, build-tooling only, never rendered.

---

## Conventions to match

- **`frontend-src/*.js` is the only editable frontend source.** `frontend/assets/*.js` is generated +
  gitignored + SHA-256-guarded by `build.rs`. Run `frontend:build` before any cargo build/test.
- **Comments justify a decision against a specific attack/regression**, citing stable IDs
  (`F-0XX`, `C9 (D8/D14…)`, `SEC-0X`, `codex r2 #N`). Match that density; explain *why*, not *what*.
- **Playwright popup tests**: `tauri-mock.js` init script + `callsFor(page, "cmd")` + exact-object
  equality on the recorded args.
- **WebDriver tests**: real binary, real IPC, assert against the on-disk `config.json` via
  `readConfig()`, with `removeTestOriginViaUI()` for isolation.
- **Rust tests**: inline `#[cfg(test)] mod tests` at the bottom of the file under test. HTTP-level
  behaviour via axum `Router` + `tower::ServiceExt::oneshot` (`core/src/server/tests.rs`), never
  through Tauri. `src-tauri/tests/*.rs` is reserved for TLS/OS-trust integration.
- **Config-drift guard precedent**: `src-tauri/src/updater.rs:508-518`
  (`updater_pubkey_matches_config`) reads `tauri.conf.json` at test time and asserts the pinned Rust
  constant equals it. `crash_recovery.rs:126-129`'s `APP_NAME` has the same risk shape with only a
  comment and **no guard test** — this is the pattern to copy if the plan adds any hardcoded mirror
  of `mainBinaryName`.
- **Narrow-anchor `pkill`**: `implementations-plan/ci-release-overhaul-2026-06-01/lessons/phase-a2b.md:55`
  records a real incident where `pkill -f "aztec-accelerator"` matched the smoke script's own argv
  (the checkout path contains that string), causing a false exit 143. Fixed by anchoring to
  `aztec-accelerator\.AppImage`. Any cleanup pattern touching a renamed binary follows that discipline.
- **Bundle-shape invariant is intentionally hardcoded** (`release-accelerator.yml:238-258`): the team
  updates it by hand when the shape intentionally changes, rather than deriving it from config.
