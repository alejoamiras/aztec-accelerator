# Planner: main — HTTPS by default (3 OSes) + first-run onboarding wizard

One of three independent planning legs (main / codex / fable) for `/blueprint deep`. Consolidation happens in `plan.md`.

## Goal

1. HTTPS between browser and accelerator becomes **default-on** on macOS, Linux, and Windows — consented through a new **first-run onboarding wizard** (user can opt out). HTTP listener stays.
2. SDK **prefers/pins HTTPS** when available (zero added latency when it isn't) + opt-in strict `requireHttps` mode.
3. Rename "Safari Support" → HTTPS/encrypted-connection naming everywhere (config key migration).
4. Wizard covers three choices with plain-language copy: Encrypted Connection (HTTPS) [YES], Start on Login [YES], Auto-Update [YES]; one **Start** CTA executes all; partial-failure UI; shown to new installs AND upgraders once; re-runnable from Settings.
5. Production quality: WebDriver E2E (3 OSes), real cert-trust CI tests, Playwright mocks, fast layers every phase.

## Key design decisions (this leg's positions)

- **D1 — Naming**: UI label **"Encrypted Connection (HTTPS)"**; config key `https_enabled` with `#[serde(alias = "safari_support")]` (old configs load 1:1, next save writes the new key); `config_version` 1→2 per the rename contract in `core/src/config.rs:38-40`. Commands `enable_https` / `disable_https` (frontend is bundled; no external callers — rename outright). macOS Settings/wizard sub-copy keeps the Safari mention ("required for Safari").
- **D2 — "Default-on" is a wizard default, NOT a struct default**: `AcceleratorConfig::default()` keeps `https_enabled: false` (headless shares this struct; silent default-flips would light up TLS surface nobody consented to). The wizard's pre-checked toggle is what makes it "default on".
- **D3 — Trust backend abstraction**: new `src-tauri/src/trust/` with a `TrustBackend` trait (`install`, `remove`, `status` → `TrustReport { stores: Vec<StoreResult> }`):
  - `macos.rs`: move the existing `security` CLI code from `certs.rs` (behavior unchanged).
  - `windows.rs`: shell `%SystemRoot%\System32\certutil.exe -user -addstore Root ca.pem` / `-delstore` / `-user -verifystore Root "Aztec Accelerator Local CA"`. Absolute System32 path (same PATH-hijack defense as `schtasks_exe()`, `crash_recovery.rs:245-253`). Native warning dialog is expected; wizard copy sets expectations. Zero new Rust deps.
  - `linux_nss.rs`: user-level NSS only, no root. Store discovery: `~/.pki/nssdb` (create if absent — Chromium reads it) + every Firefox profile from `~/.mozilla/firefox/profiles.ini`, plus snap (`~/snap/firefox/common/.mozilla/firefox`) and flatpak (`~/.var/app/org.mozilla.firefox/.mozilla/firefox`) locations best-effort. `certutil -A -d sql:<dir> -t "C,," -n "Aztec Accelerator Local CA" -i ca.pem`; binary discovery `/usr/bin/certutil` → `/usr/local/bin/certutil` → PATH fallback. Missing certutil ⇒ honest degraded `TrustReport`, wizard/Settings show per-store status + install hint.
- **D4 — Rotation generalizes**: `rotate()` (certs.rs) swaps its macOS-specific trust/remove steps for `TrustBackend` calls. Keyless CA is kept (every rotation mints a new anchor): re-trust ⇒ macOS password prompt / Windows dialog roughly every ~26 months (leaf 824d − 30d buffer); Linux is silent (user-db writes need no auth). Fail-closed behavior preserved: new-anchor trust failure ⇒ keep old set.
- **D5 — Uninstall story**: (a) NEW Settings action "Remove certificate trust" wired to `TrustBackend::remove` on all OSes — also closes the existing macOS gap (disable leaves the anchor; `README.md:255` documents manual Keychain cleanup). (b) NSIS uninstaller hook runs `certutil -user -delstore Root "Aztec Accelerator Local CA"`. (c) `.deb`/AppImage can't touch `$HOME` at package-remove time — document + rely on (a).
- **D6 — SDK preference via `https_port` advertisement (zero-latency)**: keep today's `Promise.any` race. If the winner is HTTP and its parsed `/health` advertises `https_port` (already only set when the listener actually bound, `core/src/server.rs:300-302`), upgrade the pin to `https` for subsequent requests — no timing hacks, no added latency when HTTPS is absent. If an HTTPS request then fails at the network layer (untrusted cert in this browser), demote to HTTP once and emit a phase event (`https-fallback`) for observability. `requireHttps: true` (constructor + env `AZTEC_ACCELERATOR_REQUIRE_HTTPS`) probes HTTPS only and never demotes.
- **D7 — Wizard architecture**: `onboarding.html` Tauri window, opened at launch when `onboarding_completed: bool` (serde-default `false`) is false; **non-blocking** (tray + HTTP server start regardless). Reuses existing IPC primitives (`get_config`, `get_autostart_enabled`, `set_autostart`, `set_auto_update`, new `enable_https`) plus new `get_onboarding_state` / `complete_onboarding`. Upgrader prefill: HTTPS from migrated `https_enabled`, autostart from `get_autostart_enabled`, auto-update `None`→checked. "Start" applies choices sequentially with per-row ✓/✗/spinner; failures inline with Retry / "Continue without"; completion persists everything + `onboarding_completed=true`. Settings gets "Run setup again".
- **D8 — Headless server stays TLS-free** (unchanged architecture; `server/Cargo.toml` exclusion is deliberate).
- **D9 — AppImage: detect-and-degrade**, don't bundle certutil (NSS tooling drags a shared-lib tree; bundling is a supply-chain + size cost for the least-used packaging).

## Phases

### P1 — Rename + config migration + onboarding field (no behavior change)
`safari_support`→`https_enabled` (alias, version bump 2, migration test: old JSON loads, next save writes new key), `onboarding_completed` field, command renames + settings.html label swap ("Safari Support" → "Encrypted Connection (HTTPS)"), keep row macOS-only for now.
**Gate**: `bun run test` (biome+clippy+Rust/TS unit incl. new migration tests) exit 0; `test:e2e:webdriver` settings spec green locally on dev OS. Layers: fast + targeted E2E.

### P2 — Trust backends + real cert-trust CI
`trust/` module per D3; macOS code moves; Windows + Linux backends new; `TrustReport` surfaced via new `get_trust_status` command. Real-store tests `#[ignore]`d locally, run in CI with `--ignored`: ubuntu (apt libnss3-tools; temp `sql:` NSS dir: add→verify→remove; profiles.ini fixture parsing), windows (real CurrentUser Root add→verify→remove — ephemeral runner), macOS (temp-keychain flow; if trust-settings prove un-headless, fall back to command-construction unit tests + chain verification via rustls — flagged as Inference I3). New reusable `_cert-trust.yml` (3 runners) called from `accelerator.yml`.
**Gate**: `bun run test` + new CI legs green on a PR + `bun run lint:actions`. Layers: fast + real-store integration.

### P3 — HTTPS enable cross-platform + rotation + remove-trust
Un-gate `enable_https` from macOS (`commands.rs:199-211` stubs die); launch-gate logic (`main.rs` classify/try_start) goes cross-platform; `rotate()` uses `TrustBackend`; Settings row visible on all OSes + "Remove certificate trust" action; `/health` unchanged.
**Gate**: `bun run test`; `test:e2e:webdriver` settings spec extended (toggle visible + flips on all 3 OSes vs. mocked trust) green in CI on 3 runners. Layers: fast + E2E (3 OS).

### P4 — Onboarding wizard
`onboarding.html` + IPC per D7; first-launch open logic; upgrader prefill; partial-failure UI; re-run from Settings. Playwright mocks (states: fresh, prefilled-upgrader, cert-install-failed, all-off) + new `onboarding.spec.ts` WebDriver spec (fresh config → wizard appears; choices persist to config.json; opt-out honored; re-run entry). Needs config isolation in the WebDriver harness (temp HOME) — verify existing pattern (Inference I4).
**Gate**: `test:e2e:ui` + `test:e2e:webdriver` (incl. new spec) green on 3 runners; `bun run test`. Layers: fast + UI mocks + E2E (3 OS).

### P5 — SDK preference + strict mode
Pin-upgrade per D6, `requireHttps`, phase event `https-fallback`, demote-once logic; unit tests (upgrade path, no-https path unchanged latency, strict refuses HTTP, demote-once); README + skill doc.
**Gate**: `bun run test`; SDK E2E workflow green (`sdk.yml`). Layers: fast + SDK E2E.

### P6 — Packaging + uninstall
tauri.conf.json: `.deb` depends `libnss3-tools`; NSIS uninstaller hook (delstore); AppImage degrade copy. Verify Windows Build Smoke + Release Smoke still pass; updater-smoke unaffected.
**Gate**: `bun run lint:actions`; PR gates incl. Windows Build/Prebuild Smoke green. Layers: fast + build smokes.

### P7 — Docs + closeout
PLATFORM_SUPPORT.md rewrite (Windows supported; per-OS HTTPS/trust matrix incl. snap/flatpak caveats), accelerator README (Safari section → HTTPS section; uninstall docs), SDK README (preference algorithm, `requireHttps`), CLAUDE.md, index.md close-out.
**Gate**: `bun run test` full + `bun run lint:actions`. Layers: fast.

## Security & Adversarial Considerations

- **Threat model deltas**: new trust-install code paths on 3 OSes = the highest-value target. The keyless-CA property (no mint-any-cert primitive, name-constrained anchor `certs.rs:97-107`) carries over unchanged — enforced by NSS and CryptoAPI too (both honor Name Constraints; flagged I5 to verify for NSS user-added anchors).
- **PATH hijack**: Windows uses absolute System32 certutil.exe (schtasks precedent). Linux certutil can't be absolute-pathed portably — prefer `/usr/bin` then PATH; a same-user attacker who can plant PATH binaries is already past the SEC-04 line, so this is defense-in-depth, not a boundary.
- **Input validation**: profiles.ini parsed defensively (bounded size, ignore malformed sections, no path traversal outside the profiles dir); store paths never shell-interpolated (all `Command::arg`).
- **Wizard IPC**: new commands mutate config + spawn OS prompts — must be callable only from app windows (Tauri capabilities already scope commands to bundled windows; verify capability file covers the new window only).
- **Consent integrity**: upgraders with `safari_support=false` must NOT get HTTPS silently enabled by migration — only the wizard's explicit Start does that. Migration is value-preserving.
- **Elevation**: nothing requests root/admin anywhere (CurrentUser store, user NSS dbs, login Keychain). NSIS hook runs in the per-user uninstaller context.
- **Supply chain**: zero new Rust/TS deps planned; certutil is a system binary (deb dependency, not vendored). CI installs libnss3-tools via apt from ubuntu archives.
- **Lingering anchors**: mitigated by D5; residual risk = AppImage deleted without using Settings remove — documented. Anchor is keyless + name-constrained, so residual risk is cosmetic.

## Assumptions

**Facts** (verified): config schema + version contract `core/src/config.rs:38-62`; trust stubs `src-tauri/src/certs.rs:437-446`; keyless CA + name constraints `certs.rs:86-153`; `/health` advertises `https_port` only when bound `core/src/server.rs:300-302`; SDK race + pin `packages/sdk/src/lib/accelerator-transport.ts:36-138`; autostart plugin + Windows Task Scheduler recovery `crash_recovery.rs`; existing IPC commands `commands.rs:34-225`; WebDriver E2E on 3 OSes (`_e2e-webdriver.yml`, specs in `e2e-webdriver/`); release builds windows-x86_64 NSIS (`release-accelerator.yml:101-103,169`); test commands `packages/accelerator/package.json:10-12`.

**Inferences** (unverified — attack these): I1 NSS user-db writes need no auth prompt on Linux (high confidence, mkcert precedent). I2 `certutil -user -addstore Root` shows exactly one native dialog and works non-elevated (mkcert precedent). I3 macOS CI can install trust to a temp keychain headlessly (fallback in P2 if not). I4 WebDriver harness can isolate config via temp HOME (needs verification in wdio.conf.ts). I5 NSS + CryptoAPI enforce X.509 Name Constraints on user-added anchors. I6 Chromium reads `~/.pki/nssdb` at startup only — trust lands after browser restart (UX copy must say so). I7 Tauri capability scoping covers new window/commands cleanly.

**Asks** (user must decide): A1 final UI naming (present 3-4 candidates in UX mockups). A2 AppImage strategy — this leg recommends detect+degrade (D9). A3 wizard skippable via window-close (recommend: close = "not now", re-shown next launch until completed once? or close = completed-with-defaults-off? This leg: close = not-now, re-show next launch, max friction 1 re-show then auto-complete). A4 should `requireHttps` also gate `/health` probing in playground UI (recommend: SDK-only, playground unchanged).

## Test inventory delta (summary)

Rust unit: +migration, +trust command-construction per OS, +profiles.ini parser, +rotation-via-backend. Real-store CI: +3 legs (NSS / CurrentUser Root / temp keychain). Playwright: +4-6 wizard mock states. WebDriver: +onboarding.spec.ts (~5 tests), settings.spec.ts extended (~2). SDK unit: +6 (upgrade, latency-neutral, strict, demote-once, env vars, phase event). Total new ≈ 30-35 tests, all inside their introducing phase.
