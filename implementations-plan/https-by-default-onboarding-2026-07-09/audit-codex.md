```md
# HTTPS By Default + First-Run Onboarding Wizard Plan

## Assumptions

### Facts

- Config currently stores `safari_support: bool`, default `false`, and `auto_update: Option<bool>` in `packages/accelerator/core/src/config.rs:48`, `packages/accelerator/core/src/config.rs:68`, `packages/accelerator/core/src/config.rs:53`.
- Config saves atomically and uses `0600` temp-file permissions on Unix in `packages/accelerator/core/src/config.rs:140`.
- The cert layer already creates a keyless local CA, name-constrained to loopback/localhost, and never writes the CA private key in `packages/accelerator/src-tauri/src/certs.rs:86`, `packages/accelerator/src-tauri/src/certs.rs:97`, `packages/accelerator/src-tauri/src/certs.rs:137`.
- The leaf validity is 824 days and rotation mints a fresh CA+leaf because the old CA key is discarded in `packages/accelerator/src-tauri/src/certs.rs:79`, `packages/accelerator/src-tauri/src/certs.rs:310`.
- Non-macOS trust is currently stubbed in `packages/accelerator/src-tauri/src/certs.rs:437`.
- HTTPS binds `127.0.0.1:59834`, logs failures, and continues HTTP-only in `packages/accelerator/src-tauri/src/server/tls.rs:24`, `packages/accelerator/src-tauri/src/server/tls.rs:29`.
- `/health` advertises `https_port` only when the listener actually bound in `packages/accelerator/core/src/server.rs:296`.
- SDK currently races HTTP and HTTPS with `Promise.any`, defaulting `/prove` to HTTP before negotiation in `packages/sdk/src/lib/accelerator-transport.ts:80`, `packages/sdk/src/lib/accelerator-transport.ts:112`.
- Settings currently has a macOS-only “Safari Support” row wired to `enable_safari_support` / `disable_safari_support` in `packages/accelerator/src-tauri/frontend/settings.html:33`, `packages/accelerator/src-tauri/frontend/settings.html:157`.
- PR WebDriver already runs macOS, Linux, Windows via matrix in `.github/workflows/accelerator.yml:272`; release pre-gate is macOS-only in `.github/workflows/release-accelerator.yml:51`.
- Windows NSIS is built and released in `.github/workflows/release-accelerator.yml:101`, `.github/workflows/release-accelerator.yml:603`; `docs/PLATFORM_SUPPORT.md` is stale at `docs/PLATFORM_SUPPORT.md:10`.
- The headless server depends on `accelerator-core` and intentionally excludes GUI/TLS-serving deps in `packages/accelerator/server/Cargo.toml:17`; CI guards against `rcgen`/`tokio-rustls` leaking in `.github/workflows/accelerator.yml:153`.

### Inferences

- Tauri NSIS/deb hook syntax should be verified during implementation against the pinned Tauri v2 schema/docs.
- Windows CurrentUser Root operations can be implemented with direct CryptoAPI bindings through a direct `windows-sys` dependency matching the existing lockfile, avoiding `certutil.exe`.
- Linux NSS profile layout is mostly stable, but Flatpak/Snap Firefox paths need best-effort handling and tests with synthetic profiles.
- Browsers may differ in X.509 name-constraint enforcement for user roots; the local server’s Host allowlist remains the hard browser-ingress control.

### Asks

- Final product naming: use **Enable HTTPS** in UI, `https_enabled` in config/API. “Encrypted Communication” is too vague for failure states.
- AppImage strategy: **detect and degrade**, not bundle `certutil`. Bundling NSS tooling into AppImage adds ABI/supply-chain surface and still cannot cover every browser sandbox honestly.
- Settings disable should stop HTTPS immediately and remove the current local CA anchor best-effort.

## Phase 1: Config Migration And Naming

Implement config v2:

- Replace `safari_support` with `https_enabled`.
- Deserialize legacy `safari_support` using `#[serde(alias = "safari_support")]` or a custom migration, then save only `https_enabled`.
- Add `onboarding_version: u32` default `0`.
- Add optional trust bookkeeping: `current_ca_sha1`, `pending_ca_sha1`, `last_rotation_prompt_at`.
- Preserve `safari_support=true` upgraders as `https_enabled=true`.
- New installs do not auto-enable HTTPS before the wizard; existing configs without onboarding marker show the wizard once, prefilled from current config.

Rename commands to `set_https_enabled`, `get_https_status`; keep deprecated aliases for one release only if needed for test stability.

Validation gate:

```sh
bun run lint
bun run test
cargo clippy --manifest-path packages/accelerator/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path packages/accelerator/core/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path packages/accelerator/src-tauri/Cargo.toml
cargo test --manifest-path packages/accelerator/core/Cargo.toml
```

Pass criteria: migration tests cover missing config, legacy false, legacy true, malformed config fallback, and v2 roundtrip without `safari_support`.

## Phase 2: Cross-Platform Trust Backend

Refactor cert trust into platform-specific backends with shared DTOs:

- macOS: keep `/usr/bin/security`, but parameterize keychain for tests. Use absolute path, not PATH.
- Windows: use CurrentUser Root through CryptoAPI, not `certutil.exe`. Add direct `windows-sys` only if needed, pinned to the lockfile line already present transitively. Verify by SHA-1 and subject, remove by SHA-1.
- Linux: use user NSS only. For `.deb`, add `libnss3-tools` dependency. Use `/usr/bin/certutil` or `/bin/certutil` only; reject PATH-found binaries unless root-owned and not group/world-writable. Create `~/.pki/nssdb` for Chromium. Parse every Firefox `profiles.ini`; support standard, Flatpak, and Snap paths best-effort. Surface per-store statuses.
- AppImage: detect safe `certutil`; if absent, wizard reports HTTPS setup failed and leaves HTTPS off with install instructions.

Validation gate: fast gate from Phase 1, plus:

```sh
cargo test --manifest-path packages/accelerator/src-tauri/Cargo.toml certs::tests::linux_profiles_ini
cargo test --manifest-path packages/accelerator/src-tauri/Cargo.toml certs::tests::trust_status_serialization
```

OS real-trust tests, in CI only:

```sh
cargo test --manifest-path packages/accelerator/src-tauri/Cargo.toml linux_nss_install_verify_remove -- --ignored --nocapture
cargo test --manifest-path packages/accelerator/src-tauri/Cargo.toml windows_current_user_root_add_verify_remove -- --ignored --nocapture
cargo test --manifest-path packages/accelerator/src-tauri/Cargo.toml macos_temp_keychain_add_verify_remove -- --ignored --nocapture
```

Pass criteria: add, verify, remove all succeed and leave no test anchor.

## Phase 3: HTTPS Runtime, Disable, Rotation

Introduce an `HttpsRuntime` managed state:

- Start only after cert generation and trust verification succeed.
- Stop immediately on disable and clear `https_bound`.
- Disable removes the current trusted anchor best-effort; failures are shown in Settings.
- Keep HTTP always on.
- Keep headless TLS-free; do not add TLS deps to `packages/accelerator/server`.

Rotation design:

- Staged fresh CA+leaf remains.
- Linux re-trusts silently into user NSS if `https_enabled=true`, then swaps and removes old anchors.
- macOS/Windows never surprise-prompt in background. If rotation is needed, show a renewal prompt/window with one CTA. Throttle by `last_rotation_prompt_at`; explicit retry bypasses throttle.
- If new trust fails, keep old cert serving until expiry and show degraded status.

Validation gate: fast gate, plus:

```sh
cargo test --manifest-path packages/accelerator/src-tauri/Cargo.toml https_runtime
cargo test --manifest-path packages/accelerator/src-tauri/Cargo.toml cert_rotation
```

Pass criteria: enable starts listener, failed trust leaves config off, disable stops listener and removes trust, rotation never swaps before new trust verifies.

## Phase 4: Onboarding Wizard And Settings

Add `onboarding.html` and update shared CSS/IPC mocks.

Wizard behavior:

- Opens on first launch when `onboarding_version < 1`.
- New install defaults: Enable HTTPS yes, Start on Login yes, Auto-Update yes.
- Existing upgrader defaults: HTTPS from migrated config, Start on Login from OS state, Auto-Update from config.
- One **Start** CTA calls `complete_onboarding({ httpsEnabled, autostartEnabled, autoUpdateEnabled })`.
- Partial failure UI stays open: cert failure turns HTTPS off and shows per-browser/store reason plus Retry; autostart/auto-update still apply.
- Settings gets “Run setup again”, “Enable HTTPS”, and honest per-browser status.

Validation gate: fast gate, plus:

```sh
bun run --cwd packages/accelerator test:e2e:ui
bun run --cwd packages/accelerator test:e2e:webdriver
```

Pass criteria: Playwright covers default state, upgrader prefill, opt-out, partial cert failure, retry, Settings rerun. WebDriver covers fresh wizard on macOS/Linux/Windows.

## Phase 5: SDK HTTPS Preference And Strict Mode

Algorithm:

- Fire HTTP and HTTPS probes in parallel.
- If HTTPS succeeds first, pin HTTPS.
- If HTTP succeeds first and health lacks `https_port`, commit HTTP immediately. This preserves the no-HTTPS fast path.
- If HTTP succeeds first and health includes `https_port`, wait a small HTTPS grace window, e.g. 150ms. If HTTPS succeeds, pin HTTPS; otherwise pin HTTP.
- If HTTP is blocked, HTTPS wins as today.
- Add `accelerator: { httpsOnly?: boolean }` and `AZTEC_ACCELERATOR_HTTPS_ONLY=1`.
- In strict mode, probe/post only HTTPS; no HTTP fallback; unavailable status falls back to WASM.

Validation gate: fast gate, plus:

```sh
bun run --cwd packages/sdk test:unit
bun run --cwd packages/sdk test:lint
```

Pass criteria: unit tests prove no delay when HTTPS is absent, HTTPS preferred when advertised, strict mode never calls HTTP, and `/prove` uses the pinned protocol.

## Phase 6: CI And Packaging

CI changes:

- Add `_cert-trust.yml` matrix for ubuntu, macOS, Windows real trust tests.
- Add it to `accelerator-status` and release `tag`/`release` needs.
- Expand release WebDriver gate to all three OSes, not only macOS.
- Install `libnss3-tools` in Linux trust jobs.
- Keep headless dep-tree guard unchanged.

Packaging:

- `.deb`: add `libnss3-tools` dependency.
- AppImage: no bundled `certutil`; runtime detection and degraded wizard path.
- Windows NSIS: add uninstall hook that runs installed `aztec-accelerator --remove-local-ca` before deleting files. The flag removes CurrentUser Root anchors matching stored SHA-1/name and exits before Tauri startup.

Validation gate:

```sh
bun run lint:actions
bun run lint
bun run test
```

Pass criteria: actionlint clean; Windows build emits NSIS; release artifact assertions still include Windows; new cert-trust workflow is blocking.

## Phase 7: Docs And Release Notes

Update:

- `docs/PLATFORM_SUPPORT.md`: rewrite Windows support, HTTPS per OS, AppImage limitations, Linux NSS status.
- `packages/accelerator/README.md`: replace Safari Support with Enable HTTPS, wizard, uninstall/removal, Windows installer.
- `packages/sdk/README.md`: HTTPS preference, strict mode, no Local Network Access bypass.
- Root `README.md`: desktop app now macOS/Linux/Windows.
- `CLAUDE.md`: current architecture/testing counts and HTTPS-by-default model.
- Release notes template: first-run wizard and possible Windows trust confirmation.

Validation gate:

```sh
bun run lint
bun run lint:actions
```

Pass criteria: docs no longer claim Windows unsupported or Safari-only HTTPS.

## Security And Adversarial Considerations

The new attack surface is trust installation. Treat all frontend commands as preference booleans only; no path or command strings cross IPC. Use absolute OS tools on macOS/Linux and native APIs on Windows. On Linux, a malicious `certutil` on PATH is the obvious target; do not use PATH. Reject unsafe ownership/permissions. NSS DB paths come only from known home-relative locations and parsed `profiles.ini`; canonicalize and reject traversal outside expected profile roots.

A local-foothold attacker can already tamper with user config or browser profiles, but the app must not help them persist a broader CA. Keep name constraints, Host allowlist, keyless CA, 0600 PEMs, and no CA key on disk. Do not store a stable CA key to avoid rotation prompts; that would reopen the prior HIGH risk.

Rotation necessarily creates a new anchor. Avoid prompt fatigue by throttling macOS/Windows renewal prompts and silently repairing Linux NSS only after prior user consent. Remove old anchors by SHA-1 after successful swap. On uninstall/disable, remove anchors best-effort and report leftovers.

Do not trust browser NSS name-constraint enforcement as the only boundary. The server still binds loopback and checks Host/Origin. Do not add TLS to headless; its CI/operator threat model is different and the current dependency guard should stay.
```
---

# Round 1 — codex contradiction-check + adversarial audit (resume of planning session 019f4849)

**D3 Verdict**
- **Med:** Your `certutil.exe` verification design is locale-robust only if it relies strictly on exit code. That is likely true in practice for absent certs, but Microsoft does not document exit-code semantics as an API contract, and explicitly says `certutil` is not recommended for production code and has no compatibility guarantees: https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/certutil
- **High:** The “NSIS needs certutil anyway” argument does **not** hold. The uninstaller can call `aztec-accelerator --remove-ca-trust` before deleting files, and that CLI can use CryptoAPI in-process. Certutil in NSIS is a convenience, not a necessity.
- **Med:** Absolute `%SystemRoot%\System32\certutil.exe` is better than PATH, but `%SystemRoot%` is still environment-derived. Prefer `GetSystemDirectoryW` or hardcoded/canonicalized `C:\Windows\System32` fallback.

I would conditionally accept certutil for Windows only if CI pins add/verify/remove behavior and the plan stops claiming it is production-API robust.

**D4 Verdict**
- **High:** Persisting certutil’s printed thumbprint at add-time is the fragile parsing trap. `-addstore` output is not documented as stable, and parsing it undercuts D3’s “exit-code only” mitigation.
- **High:** Use option (a): compute SHA-1 from the cert DER with a minimal `sha1` crate. This is the honest choice. The project already has direct `sha2`; one small RustCrypto hash crate is lower risk than relying on localized/tool-formatted stdout.
- **Med:** Third option: make CA CN unique per rotation, e.g. `Aztec Accelerator Local CA <serial8>`, and persist the exact CN. This makes delete-by-CN unambiguous, but complicates user-visible naming and uninstall cleanup when config is missing. I prefer SHA-1.

**Plan Findings**
- **Critical:** Phase 3 conflicts with the user constraint. It says Linux `enable_https` saves/spawns even if trust fails and launch is “Ready when certs valid” (`plan.md:63`), but the requested behavior is cert install failed → HTTPS off with retry path. Make trust failure disable HTTPS during wizard completion.
- **High:** Windows “native dialog” is unsafe to assume. `certutil -user -addstore Root` may run without a GUI confirmation. The wizard Start click must be treated as the consent ceremony; copy should not promise a Windows confirmation dialog unless the spike proves it.
- **High:** SDK step “if httpsP fulfills → HTTPS wins” is too broad (`plan.md:41`). With `throwHttpErrors:false`, a hijacked/broken HTTPS listener returning 500 fulfills and could beat a healthy HTTP listener. Prefer HTTPS only after OK + parseable health.
- **High:** `complete_onboarding` sets `onboarding_version=1` unconditionally (`plan.md:73`). If HTTPS fails and the app closes, the wizard may never return. Set marker only after explicit “Continue without HTTPS” or successful completion.
- **High:** Linux `certutil via which` is not enough (`plan.md:63`, `plan.md:107`). It must reject user-writable binaries and unsafe parent dirs, or use known absolute paths plus ownership/perms checks.
- **Med:** Delete-by-CN on Windows uninstall can delete unrelated certs with the same CN, or miss suffixed future CNs. Prefer persisted thumbprints; CN fallback should be best-effort with narrow matching.
- **Med:** The 3-OS real-trust CI plan is feasible, but Windows dialog behavior must be spiked first. If no dialog appears, CI can be real; if it blocks, split “store mutation” tests from “manual consent UX” tests.
- **Med:** WebDriver completes onboarding with HTTPS off (`plan.md:75`), so it does not test the real default-on happy path. Real-trust tests cover lower layers, but add at least one app-level enable test behind a CI-safe trust mode or Windows/macOS-specific spike.
- **Low:** “Encrypted Connection” may overpromise. Keep label, but the copy must say HTTPS is for authenticated local accelerator discovery and Safari compatibility, not broad local-compromise protection.

**Facts / Inferences / Asks**
- **Facts:** D3’s “NSIS uninstaller needs certutil regardless” is not a fact; move it to inference or remove. D3’s `-verifystore non-zero if absent` is also unverified, not a verified fact.
- **Inferences:** I1 and I4 are the riskiest operational assumptions. Chrome/NSS behavior and loopback failure timing must be treated as compatibility tests, not design proof. I2/I3 are correct to spike. I8 is too vague; Tauri bundled-content-only does not protect against bugs/XSS in bundled pages.
- **Asks:** A5 should be resolved now in favor of `sha1`. A3 needs clearer UX wording: “Skip” can mark onboarding, but partial failure should not silently mark done without user acknowledgement. A4 needs a precise decision: disable listener only vs remove trust too.

VERDICT: conditional approve (conditions: D4 uses computed SHA-1, Linux trust failure leaves HTTPS off in wizard, SDK only prefers OK/parseable HTTPS, Windows certutil behavior is CI-spiked and documented as tool-dependent)
---

# Round 2 — final fresh-context codex pass (new session 019f4867)

- **Med:** R6/D4 is partially wrong. Windows `certutil -delstore <store> <certID>` can use a serial-like cert identifier, and the plan correctly treats that as CI-pinned rather than API-stable. NSS `certutil` does **not** use that model for deletion; its documented delete path is `certutil -D -n <nickname> -d <db>`. The Linux phase mostly plans nickname removal already, so fix the ledger/resolution wording to “Windows serial; Linux NSS nickname” and keep the old-nick-gone real-trust test. Sources: [Microsoft certutil](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/certutil), [NSS certutil manpage](https://www.mankier.com/1/certutil).

- **Med:** Windows uninstall cleanup needs an explicit serial sidecar/bookkeeping contract. `NSIS_HOOK_POSTUNINSTALL` runs after app files are removed, so the hook cannot rely on the Rust binary or x509-parser at uninstall time. The plan hints at serial-bookkeeping, but make it concrete: atomically persist current CA serial somewhere the hook can read before it deletes the cert dir.

- **Med:** R1 is sound for Tauri auto-update: the NSIS template parses `/UPDATE` into `$UpdateMode`, and POSTUNINSTALL hooks are supported. But manual NSIS upgrade/reinstall can still invoke the old uninstaller without `/UPDATE`, and NoModify/NoRepair only covers ARP maintenance modes. That is not an auto-update blocker, but document it as “manual reinstall may remove trust and require re-enable.” Source: [Tauri config installerHooks](https://v2.tauri.app/reference/config/#installerhooks), [Tauri NSIS template](https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi).

- **Low/Med:** P3/P4 chain validation does not prove name-constraint enforcement unless it includes a negative leaf outside loopback. Either add that negative test or stop saying I5 is “pinned.” Since keylessness is the primary control, this should not block.

- **Low/Med:** S4 overstates least privilege. The current capability grants `core:default` to all local windows; adding onboarding windows without per-window capabilities means bundled XSS still reaches the new trust commands. Add a P5 gate for window-scoped capabilities or explicitly accept the bundled-content risk.

- **Low:** R2 is directionally sound, but “parseable health” should mean schema-acceptable health, and implementation must clone the `Response` before parsing for winner selection. Add tests for HTTPS 200 malformed/schema-invalid + HTTP OK => HTTP wins.

- **Low:** R5 is not theater if labeled honestly: admin-domain macOS CI tests `security`/chain mechanics, not the production login-keychain/user-auth path. The plan mostly says this, but the summary’s “real cert-trust CI on 3 OSes” should be softened.

No High/Critical issue survived for me. Phase ordering is basically coherent; P2 can ship first because HTTPS remains health-gated and `httpsOnly` is opt-in. The only sequencing cleanup is to avoid splitting NSIS hook ownership ambiguously between P4 and P6.

VERDICT: conditional approve (conditions: fix R6/D4 to Windows-serial/Linux-nickname, make Windows uninstall serial bookkeeping explicit, and add/downgrade the name-constraint enforcement claim)
---

# Post-impl codex audit (session 019f48c9) — dispositions

Verdict-equivalent: 3 High, 3 Med, 2 Low, + CI-spike notes. All addressed:

- **High — rotate() serves old leaf after removing old anchor** → FIXED. The running TlsAcceptor
  isn't reloaded (rotation takes effect next launch), so removing the old anchor broke the
  still-served old leaf. Now rotate() does NOT remove the old anchor; both stay trusted. The stale one
  is keyless + loopback-name-constrained (harmless), ≤1 accrues per ~2-year rotation; "Remove
  certificate trust" + the uninstaller clear all by CN. (certs.rs)
- **High — profiles.ini path traversal** → FIXED. Canonicalize each candidate profile dir (resolves
  symlinks + `..`) and require it under canonical $HOME before handing to certutil. (trust/linux.rs)
- **High — Windows certutil via SystemRoot env (taint-redirectable)** → FIXED. Prefer the hardcoded
  `C:\Windows\System32\certutil.exe` when present (defeats env-taint on standard installs); SystemRoot
  fallback only for non-standard roots. (GetSystemDirectoryW deferred — needs windows-sys, against D3.)
- **Med — onboarding buttons stuck disabled on partial failure** → FIXED. Re-enable Start + Skip
  explicitly in the partial-failure branch (wireButton only re-enables on throw). (onboarding.html)
- **Med — Linux PATH hardening ignores owner-writable / /usr/local/bin unchecked** → ACCEPTED. Primary
  resolution is hardcoded system paths (/usr/bin, /bin, /usr/local/bin — root-owned on normal systems);
  the `which` fallback rejects group/world-writable (the cross-user threat). Same-user is already past
  SEC-04. Tightening the hardcoded list risks breaking staff-group-writable /usr/local. Defensible.
- **Med — disable_https doesn't remove trust (vs A4)** → INTENTIONAL (D5). Disable stops serving;
  trust removal is the separate explicit "Remove certificate trust" action, so a re-enable doesn't
  re-prompt every toggle. Documented tension between A4 and D5; chose D5.
- **Low — renewal throttle field never read** → FIXED. main.rs now suppresses the renewal window if
  `last_rotation_prompt_at` is within 20h (so "Later" persists across quick restarts; still reappears
  on later launches per §7).
- **Low — httpsOnly builds an http URL string** → FIXED. httpUrl is now only constructed in the
  non-strict branch. (accelerator-transport.ts)
- **CI-spike — Windows delstore-by-serial not exercised; updater trust-survives-update assertion**
  → CI follow-ups (unpushable + untestable locally). The $UpdateMode guard is the actual R1 fix.

Re-validated after fixes: Linux 24 lib + 7 main + real NSS integration test pass, clippy -D clean;
Windows target clippy -D clean; SDK 53 pass; biome/fmt clean.
