# B4 — shipped-product + migration gate (LAST): implementation plan

Executes `plan.md:222-...` (root-plan B4 design, double-audited). Three items; item 1 is bounded and
implementable now, items 2-3 are the 3-OS packaged-E2E infra + an owner decision.

## Item 1 — config migration + type-bound persist gate (DO FIRST)

**Design (root plan §B4.1), confirmed against `packages/accelerator/core/src/config.rs`:**
- Today: `config_version` exists (default 1) but NOTHING reads it; the HTTPS-by-default feature was a
  CLEAN-INSTALL (dropped the `safari_support` serde alias → an old key is ignored, `https_enabled` defaults
  off, wizard re-enables). No migration, no version gate.
- B4 adds:
  1. **Two-stage load.** Stage 1: lenient probe `{ config_version: u32 }` (unknown fields ignored). If
     `probe.config_version > CONFIG_VERSION` (=2) → the load returns the config (best-effort) but **NO
     `PersistCapability`** — an older app must never overwrite a newer-schema config.
  2. **Value-pass migration** (stage 2): parse the raw `serde_json::Value`; if it has `safari_support` and
     no `https_enabled`, set `https_enabled` from it (NEW key wins if both present — no duplicate-field
     error, unlike the old alias). Bump `CONFIG_VERSION = 2`; a migrated/current load writes v2.
  3. **`PersistCapability`** — a private, non-forgeable token: no `Default`, no public constructor, minted
     ONLY by a successful current-or-migratable load. **Every save path takes `&PersistCapability`**, so a
     future-schema load (which yields none) makes a save fail to COMPILE — structural, not convention
     (same newtype discipline as B2's `PendingUpdate` / F8's slot).

**Save-path scope (centralized — verified):** the ONLY writers are `config::save` / `save_to` and the
mutate chokepoint `config::lock_mutate_save`. Callers go through those:
- `save`/`save_to`/`lock_mutate_save` gain a `&PersistCapability` param.
- `load()`/`load_from()` return `(AcceleratorConfig, Option<PersistCapability>)` (or a `LoadedConfig`).
- `main.rs` startup mints the cap at load and stores it in AppState next to `ConfigState`.
- `commands.rs::mutate_config` takes the cap from AppState and passes it down.
- The headless `server/` config load/save threads it the same way.
- `config.rs` internal `approve_origin` persist helper + any startup-reset path thread it.

**Tests (root plan): each mutation-provable.**
- migration trio: (a) `safari_support:true` → `https_enabled:true` + v2 written; (b) both keys → new wins;
  (c) already-v2 untouched.
- `future_config_never_persisted_over`: a fixture with `config_version: 999` that is NOT v2-parseable →
  load yields NO capability → **the decisive mutation is REMOVING the capability arg from a save → the test
  file won't compile** (structural). Plus each save path mutation-tested to require the cap.
- v2-untouched: a current v2 config round-trips unchanged.

**Windows-cfg note:** none here (pure core). Validate: `cargo test` in core + `bun run test`.

## Items 2–3 — GROUNDED DESIGN (recon: Explore map 2026-08-17, cited below)

**Evidence bar (per OS):** the app's own generated CA is trusted in that OS's real store → the app's launch
gate passes (`is_ca_trusted`, `trust/mod.rs:186`, = `any_installed` over ≥1 store) so it serves TLS on
`:59834` → a REAL browser loading the PACKED SDK completes a native `bb` proof over HTTPS, with HTTP-downgrade
disabled — and the accelerated path is POSITIVELY asserted (not merely "no error").

### The two non-obvious constraints the recon surfaced
1. **No WASM-disable flag exists.** The accelerator is a by-design always-degrading optimization: under strict
   `httpsOnly`, a failed HTTPS `/prove` falls straight to WASM (never plaintext) rather than throwing
   (`accelerator-prover.ts:350`,`457`,`519`,`664`). So a silent WASM fallback would pass a naive gate GREEN.
   **The harness MUST positively assert the accelerated path** — via the `onPhase` callbacks
   (`detect→serialize→transmit→proving→proved`) or the server's `x-prove-duration-ms` response header
   (`accelerator-prover.ts:554`). Force HTTPS-only with `AZTEC_ACCELERATOR_HTTPS_ONLY=1` +
   `allowInsecureDowngrade=false` (no `http://` URL is ever constructed, probe or `/prove` retry).
2. **Chicken-and-egg trust bootstrap.** The app SKIPS HTTPS until its CA is trusted (`LaunchHttpsGate::
   UntrustedSkip`, `main.rs:81`), but the app's own trust-INSTALL is the interactive path (hangs headless).
   So the harness bootstraps out-of-band: launch once so the app generates `~/.aztec-accelerator/certs/ca.pem`
   (cert GEN is non-interactive; only the STORE WRITE prompts) → seed that CA into the OS store out-of-band →
   relaunch → the read-only predicate now passes → HTTPS serves. The same seeded store makes the browser
   trust the fetch (Chromium reads NSS on Linux / the OS store on mac+win).

### OWNER DECISION — RESOLVED by recon: all 3 OSes automate out-of-band; NO production trust-seam needed
The predicate is a pure READ on every OS (`security verify-cert` / `certutil -store <serial>` / `certutil -V`,
`trust/{macos:62,windows:147,linux:307}`). Only the WRITE prompts. Each OS has a non-interactive seeding lever
that satisfies BOTH the app predicate and the browser:
- **Linux**: `certutil -A -t C,, -d sql:~/.pki/nssdb` (exactly `install_ca_trust`'s own store; Chromium reads
  it). Fully proven headless already (`tests/trust_linux.rs`).
- **Windows**: PowerShell **`Import-Certificate -CertStoreLocation Cert:\CurrentUser\Root`** — the in-repo
  documented CI lever (`trust/windows.rs:15`); raises NO dialog (only raw `certutil -addstore Root` /
  `X509Store.Add` do). Predicate matches by serial in `Root`.
- **macOS** (the one unproven leg): **`sudo security add-trusted-cert -d -r trustRoot -k
  /Library/Keychains/System.keychain ca.pem`** — GH `macos-latest` has passwordless sudo; the predicate
  `security verify-cert -l -L` reads the default search list (incl. System keychain); Playwright/Chromium on
  macOS uses the Security framework, so it trusts it too. Repo currently seeds mac trust only via the manual
  runbook — this SYSTEM-domain CI seed is the new, to-be-proven bit.

⇒ **Decision (pending codex design review + owner PushNotification): (a) seed trust out-of-band per-OS in CI
(certutil / Import-Certificate / sudo system-domain), NO production code change, all 3 OSes AUTOMATED.**
Rejected: the `e2e-trust` cargo-feature trust-seam (adds a bypass to security-critical prod code for no gain
now that out-of-band seeding works) and the self-hosted runner (infra spend, owner-only). If the macOS
system-domain seed proves flaky on GH runners, the fallback for macOS ONLY is documented manual pre-GA
verification (Linux+Windows stay automated regardless).

### Item 2 — the combined gate (build Linux leg FIRST as the reference, then replicate mac/win)
A new `packaged-e2e-on-draft` job. The packed-SDK→installed-app→HTTPS-proof flow is GREENFIELD — the
ingredients exist SEPARATELY and none combine HTTPS+trust+proof: the real-proof harness
(`packages/sdk/e2e/proving.test.ts`, but over HTTP against the SERVER crate), the `npm pack` step
(`sdk.yml`), and the installed-app launch (`_e2e-webdriver.yml`). Compose them:
1. Download the per-OS installer artifact (`accelerator-<platform>`; `.dmg`/`.deb`/`.AppImage`/`-setup.exe`) —
   `release-accelerator.yml:301` publishes them; the `smoke` job (`:416`) is the download/mount template.
2. Install it; launch once to generate the CA; seed trust out-of-band (per-OS lever above); relaunch.
3. `npm pack` the SDK; a tiny browser harness page imports the PACKED tarball, points at `:59834` HTTPS with
   `AZTEC_ACCELERATOR_HTTPS_ONLY=1`, runs a real proof, and asserts the accelerated path via `onPhase` /
   `x-prove-duration-ms`.
4. **Stateful `1.0.7→2.0.0` upgrade** leg: install 1.0.7, set origins/config/HTTPS, upgrade in place, assert
   preservation + the config MIGRATION (item-1's `safari_support→https_enabled`, `config_version` bump) +
   CA-trust survives; **+ B5 FULL uninstall** (package removal) asserting app-owned stores are cleaned.

### Item 3 — wiring into B6's deferred draft-gate slot
Per `release-accelerator.yml:18` ("stable DRAFT-gate — draft → packaged-E2E-on-draft → finalize — lands in
B4") + `:1123`. Two options (pick in codex review): (a) add `packaged-e2e-on-draft` to `tag.needs` (`:638`)
alongside `smoke`, mirroring the existing gates (simplest; keeps the transitive `needs: e2e-webdriver`
invariant, `:136`); or (b) the literal draft model — `release` creates `--draft`, the new job `needs:
[release]` downloads the draft's OWN assets, a `finalize` job (`gh release edit --draft=false`) `needs` it.
Prefer (b) only if "prove the real published assets" is worth the extra release-graph complexity; else (a).

## Codex design-review R1 (2026-08-17, session `01a00fb3` / `codex-DNGjdn6U`) — corrections that OVERRIDE the above
The recon's "all 3 OSes automate out-of-band" was too optimistic. Codex (read-only, cited the code) corrected:

1. **Windows is the real blocker (recon was wrong).** `windows.rs:16` *claims* CI seeds `CurrentUser\Root` via
   PowerShell `Import-Certificate` non-interactively, but the actual P4 spike outcome (`tests/trust_windows.rs:3`)
   states plainly: adding to `CurrentUser\Root` via `certutil -addstore` / `X509Store.Add` / Import-Certificate
   all raise the root-CA "Security Warning" dialog and **hang headless**. `LocalMachine\Root` is silent (admin)
   but the app's predicate queries only `-user` (`windows.rs` `live_present`, `-user -store Root <serial>`), so
   an LM seed won't satisfy it. ⇒ **Windows CANNOT run the composed proof headless as-is.** OWNER DECISION
   (Windows only) — options: **(a)** a small PRODUCTION predicate change: accept OUR-serial CA in
   `LocalMachine\Root` *OR* `CurrentUser\Root` (NOT a bypass — still our specific serial; LM\Root is admin-gated
   so it's a *higher* trust bar, and it's a real enterprise-IT-deploys-the-CA feature; then CI seeds LM\Root
   silently + Chromium-on-Windows trusts LM\Root too); **(b)** a test-only `e2e-trust` bypass seam (rejected —
   trust bypass in security code); **(c)** managed/pretrusted runner (infra, owner-only); **(d)** documented
   manual pre-GA Windows verification (Linux+macOS stay automated). **Recommend (a); fallback (d).** → codex
   confirm + owner PushNotification.
2. **macOS is fine (recon's fear was misplaced).** `verify-cert` runs WITHOUT `-k`, so it's not login-keychain
   restricted; a `sudo security add-trusted-cert -d` System/Admin anchor satisfies the predicate, and
   Chromium-on-macOS consumes System-keychain roots. Bound the seed with a timeout; prove `dump-trust-settings
   -d` + `verify-cert` + a browser fetch (sudo ≠ guaranteed on newer TCC-tightened runners).
3. **Linux serves HTTPS decoupled from trust** (`main.rs:126` `ca_trusted = || true`) — so the Linux app needs
   only certs-present; only the BROWSER's NSS (`~/.pki/nssdb` via `certutil`, preserving the content-hash
   nickname) needs the seed. Use `.deb` (or `install libfuse2t64` for the AppImage — GH runners lack FUSE).
4. **Bootstrap fix = a narrow `--generate-certs-only` CLI/command** (NOT a trust bypass): startup returns early
   with HTTPS off and never generates (`main.rs:174`); generation lives only inside `enable_https_inner` right
   before the interactive install (`commands.rs:618`). So add a headless "generate certs, don't install trust"
   entry; the harness then seeds trust out-of-band + writes a config with `https_enabled:true` AND the harness
   **approved origin pre-added** (else the `/prove` auth popup blocks headless). Do NOT reimplement the
   keyless-CA profile in the harness. The `1.0.7` upgrade leg has the SAME passive-generation gap (1.0.7 also
   won't self-generate headless) — the leg installs 1.0.7 then must bootstrap its certs the same way.
5. **Positive-proof (proved ≠ enough — WASM emits `proved` too, `accelerator-prover.ts:580`):** require ALL of
   — the exact `onPhase` sequence WITHOUT `fallback`, a successful returned proof, AND a Playwright-observed
   `200 https://127.0.0.1:59834/prove` carrying `x-prove-duration-ms` (the server adds that header ONLY after a
   successful `bb::prove`, `server/prove.rs:347`). Pass BOTH strict flags explicitly to the page (browser
   `process.env` is unreliable): `httpsOnly:true` + `allowInsecureDowngrade:false`.
6. **Wiring = literal draft → gate → finalize, and MOVE tag creation to finalize** (else a failed draft-gate
   burns the version tag against a failed SHA). Adding the job to `tag.needs` (option a) does NOT test the
   uploaded release assets — rejected.
7. **Uninstall leg:** product uninstall removes only its LOGIN-keychain (mac) / `-user` (win) anchor — it does
   NOT remove a harness-seeded System/LM anchor, so the harness must clean up its OWN out-of-band seed
   (don't credit product-uninstall for it).

## Harness build recipe (EXECUTE NEXT — grounded, zero re-research needed)
Branch `worktree-b4-e2e` (off `db8a373`). Landed: `--generate-certs-only` (`289181e`). Build order:

**A. Browser proof harness — REUSE THE PLAYGROUND (decided; NOT a greenfield Vite app).** The playground
(`packages/playground`) already is a browser dApp that proves via `AcceleratorProver` + `EmbeddedWallet`,
with `onPhase` tracking, a `?forceProofs=true` param, and the hard browser-bundling already solved
(`bbWorkerPlugin` redirecting bb.js/kv-store web-workers, nodePolyfills, COOP/COEP, pinned `@aztec` 5.0.1, CRS
cache-busting). A greenfield app would re-solve all of that + drift. So the harness = a SMALL playground
extension + a new Playwright project + the CI job.
- **DONE:** `aztec.ts:initializeNode` now honours **`?httpsOnly=true`** → `new AcceleratorProver({ accelerator:
  { httpsOnly:true, allowInsecureDowngrade:false } })` (both flags via opts, codex: env unreliable); default
  keeps the dual-probe for real users. Typechecks. The proving tx already exists: `deployTestAccount`
  (`createSchnorrAccount → getDeployMethod → sendWithRetry`) with `onPhase` + a `proveTracker` that captures
  `proved.durationMs`.
- **DONE (A2):** `e2e/accelerator.packaged-e2e.spec.ts` + a `packaged-e2e` Playwright project (`playwright.
  config.ts`) + `test:e2e:packaged` script. Loads the playground from **localhost** (Chrome 142+ LNA exempts
  localhost) with `?forceProofs=true&httpsOnly=true`, drives deploy-account via the existing `deployAndAssert`,
  and asserts the decisive witness — an intercepted **`200 https://127.0.0.1:59834/prove` carrying
  `x-prove-duration-ms`** (`server/prove.rs` adds it ONLY after a real `bb::prove`; WASM/HTTP emit no such
  request, so this alone discriminates native-over-HTTPS from a silent fallback). No `ignoreHTTPSErrors` (the
  TLS handshake against the seeded CA must really succeed). Typecheck + biome green. **⇒ Recipe A COMPLETE.**
  (If codex's harness review wants the explicit `onPhase` no-`fallback` trail too, add a `window.__ACCEL_PHASES__`
  export in `deployTestAccount` — deferred as belt-and-suspenders; the HTTPS-header witness is sufficient.)
  Note for B: `bun run dev` serves HTTP on :5173, so the HTTP page fetching `https://:59834` is an UPGRADE (fine,
  not mixed-content) and same-address-space as loopback (LNA-exempt). Pre-warm or allow a `downloading` phase.

**B. Linux CI leg (reference — fully automated) — a new reusable `_e2e-packaged.yml`.** Concrete structure
(grounded in `_e2e-app.yml` + `_e2e-webdriver.yml` + `release-accelerator.yml:smoke`):
- `runs-on: ubuntu-latest`, `timeout-minutes: 55`. `workflow_call` inputs: `ref`, `app_artifact`
  (default `accelerator-linux-x86_64`), `aztec_node_url` (default `http://localhost:8080`).
- Steps: `checkout` → `./.github/actions/setup-aztec` (`skip_cli` per node url) → `./.github/actions/
  playwright-cache` → `./.github/actions/start-services` (the sandbox node). Then:
- **GUI-app-on-headless-Linux needs a virtual display + tray** (from `_e2e-webdriver.yml`): `sudo apt-get
  install -y xvfb stalonetray dbus-x11 libnss3-tools`; `Xvfb :99 -screen 0 1280x1024x24 &`; `echo
  "DISPLAY=:99" >> $GITHUB_ENV`; start `stalonetray` + a dbus session (the app is a system-tray app). Prefer
  the **`.deb`** (`sudo apt-get install -y ./artifact/*.deb` pulls its GTK deps) over the AppImage (needs
  `libfuse2t64`).
- **Bootstrap + seed:** `AztecAccelerator --generate-certs-only` (writes `~/.aztec-accelerator/certs/ca.pem`)
  → seed the browser store: `certutil -A -t C,, -n aztec-accel-e2e-ca -d sql:$HOME/.pki/nssdb -i
  ~/.aztec-accelerator/certs/ca.pem` (create the DB first if absent, `certutil -N --empty-password`). On Linux
  the APP serves HTTPS trust-decoupled (`main.rs:126`), so this NSS seed is purely for the BROWSER.
- **Config:** write `~/.aztec-accelerator/config.json` = `{"config_version":2,"https_enabled":true,
  "auto_approve_localhost":true}` — `auto_approve_localhost:true` auto-approves the `localhost:5173` page so no
  `/prove` auth popup is needed (simpler than listing the origin; the headless GUI has no popup surface anyway).
- **Launch + wait:** `AztecAccelerator >/tmp/accel.log 2>&1 &`; poll `https://127.0.0.1:59834/health` (curl
  `-k` for the poll only — the app's own health, not the trust proof).
- **Packed SDK:** `npm pack` the SDK → install the tarball into the playground (override `workspace:*` for this
  run, e.g. a `bun add ./aztec-accelerator-*.tgz` in a throwaway step or a `resolutions`/`overrides` swap) so
  the proof exercises the PUBLISHED artifact, not workspace source (brief requirement). Then `bun run --cwd
  packages/playground test:e2e:packaged` with `AZTEC_NODE_URL` set. Playwright auto-starts the Vite dev server
  (`webServer` in the config) on :5173.
- **EMPIRICAL UNCERTAINTIES to validate on the FIRST real CI run** (codex flagged; can't verify locally):
  (1) does Playwright's bundled Chromium read `~/.pki/nssdb` so the seeded CA is trusted for the
  `https://:59834` fetch? If not, the fallback is a custom `NSS`/`SSL_CERT` env or a Chromium policy — NOT
  `ignoreHTTPSErrors` (that voids the trust proof). (2) does the tray app come up + serve HTTPS under Xvfb?
  (3) first-proof bb download latency (the `downloading` phase) vs the 15-min test timeout — pre-warm the bb
  cache dir if needed.

**C. macOS leg.** Same, but seed via `sudo security add-trusted-cert -d -r trustRoot -k
/Library/Keychains/System.keychain ca.pem` (bound with a timeout; on failure fall to the manual-gate residual);
verify with `security dump-trust-settings -d` + `security verify-cert -c ca.pem` before launch. The app's launch
predicate must pass (macOS gates on trust).

**D. Windows leg — BLOCKED on the owner decision** (LocalMachine\Root predicate extension vs manual gate). Do
NOT build until resolved.

**E. Upgrade + uninstall legs.** Install `1.0.7`, set origins/config, upgrade to `2.0.0` in place, assert the
config migration (item-1: `safari_support→https_enabled`, `config_version`→2) + origins/autostart/HTTPS survive
+ CA-trust survives; then B5 FULL uninstall asserts app-owned stores cleaned. NOTE the 1.0.7 passive-generation
gap (bootstrap its certs the same way) and that product-uninstall does NOT remove the harness's System/LM seed
(the harness cleans its own).

**F. Wiring (`release-accelerator.yml`).** LITERAL draft→gate→finalize: `release` creates `--draft`; the
packaged-e2e job `needs:[release]` downloads the DRAFT's own assets; a `finalize` job (`gh release edit
--draft=false`) `needs` it; MOVE tag creation to finalize (a failed draft-gate must not burn the version tag
against a failed SHA). Keep every side-effecting job transitively `needs: e2e-webdriver` (`:136` invariant).

**G. Codex review the harness (fresh session) → PR → CI → merge.** Then the release sequence.

## Nomenclature checklist (root plan owns; verify ALL before RC)
Updater/promote semver comparisons (real semver); version fields (tauri.conf.json / Cargo.toml / package.json
/ NSIS / CFBundleVersion / deb/AppImage); identity-guard/marker/scheduled-task names; docs/landing/runbook/
README version strings; confirm SDK `x-aztec-version` isn't coupled to app major.

## Item 1 — DONE (config migration + type-bound persist gate + cross-process write lock)

Commits on `worktree-b4-product-gate`: `95d94af` (migration + capability + full caller threading through
core / src-tauri / server) → `933f70d` (fail-closed load + save-time re-check) → `42f6615` (cross-process
write lock + fresh-probe preflight + object-probe) → `adbdd5d` (duplicate-key fail-closed) → `eac7de8`
(end-of-input fail-closed). 263 core tests, clippy, `x86_64-pc-windows-gnu` cross-check, and full `bun run
test` all green.

**Shape:** `CONFIG_VERSION = 2`; two-stage `load_with_cap[_from]` mints a non-forgeable `PersistCapability`
ONLY on a fresh install or a fully-read, object-probed (`<= 2`), migrated+deserialized config — every
uncertainty fails closed (`cap: None`, read-only). Every save path requires `&PersistCapability` (compile
gate). `save_to` re-checks the on-disk version right before the rename, under a cross-process **exclusive
advisory lock** (`config.json.lock`, `flock`/`LockFileEx`, no new crate) held across stage→recheck→rename, so
the recheck→rename is atomic w.r.t. any other v2+ writer. `lock_mutate_save_to` commits to the in-memory lock
only after a successful save (no divergence). Trust-mutating commands preflight persistence (startup cap AND a
fresh on-disk re-probe) before any cert/trust side effect. `probe_config_version` is a streaming map visitor
requiring a JSON object, rejecting duplicate `config_version`, and enforcing end-of-input.

**Codex loop (session `01a00f45`, 6 rounds, converged):** R1 found fail-open probing + unbound/stale caps →
fail-closed load + save re-check. R2: still fail-open on unreadable destination + memory divergence →
fail-closed `version_is_overwritable` + clone-before-commit. R3 arbitrated the cross-process lock as
load-bearing (app is not single-instance; descheduling + inode/pathname race + shared-temp) → added the lock +
fresh-probe. R4: duplicate-key last-wins fail-open → streaming visitor. R5: `deserialize_map` didn't enforce
end-of-input → `de.end()`; **and conceded the concurrent trust-side-effect race as an acceptable documented
residual** (no non-loopback exploit; a dialog-spanning cross-process lock would be materially worse). R6:
confirm clean. Every security-relevant guard mutation-proved (revert → named test fails → restore).

**Documented, codex-accepted residuals** (NOT blockers): (a) the already-shipped 1.0.7 has no version gate, so
a downgrade to it can overwrite v2 (cannot retrofit a released binary; the gate protects v2.0.0+→future); (b)
additive fields don't bump `CONFIG_VERSION`, so a same-version older build re-defaults a newer additive field
(reset via serde default, not corruption); (c) the concurrent trust-side-effect race — a trust op (install/
remove a loopback-name-constrained CA) may not be reflected in a config a concurrently-running future build
owns. It cannot overwrite that config (the locked save refuses); it is availability-only, surfaced to the UI,
and reconciled on next enable/launch.

## Sequencing
1. Item 1 (config migration) — implement core-first, thread callers, mutation tests, codex loop → PR → merge.
2. Item 2+3 packaged-E2E infra (the bulk) — wire into B6's draft-gate slot; Linux composed proof automated;
   surface the mac/win owner decision.
3. Nomenclature sweep. Then the release sequence (rc → gates → ≥2h soak → promote → SDK publish → close-out).
