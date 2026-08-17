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

## Item 2 — per-OS combined gate (fresh-install; upgrade→proof→FULL uninstall)
`release-accelerator.yml:392-445` smoke stops at `/health`. B4 extends the packaged-app E2E to: packed SDK
→ real browser → INSTALLED desktop → native bb proof over HTTPS, on all 3 OSes; a stateful
`latest-stable(1.0.7)→2.0.0` upgrade preserving origins/config/autostart/HTTPS/CA-trust (CA-trust-present
assertion where seedable — Linux now; mac/win residual ledgered); + B5's FULL uninstall (incl. package
removal) in the same matrix, assertions scoped to app-owned stores. **This is the draft-gate slot B6
deferred** — the packaged-E2E runs against the DRAFT's assets (download from the draft → install → prove),
which B6's release job creates; on PASS the draft is finalized.

## Item 3 — HTTPS proof matrix + OWNER DECISION (surface BEFORE RC dispatch; does NOT block the RC build)
Evidence bar per OS: app CA present in its OWN production trust store (not distrusted) → app logs `Ready` →
:59834 serves TLS → a real browser with the packed SDK completes a native-bb proof, HTTP-downgrade AND
WASM-fallback DISABLED.
- **Linux**: full composed proof AUTOMATED (certutil → user NSS; the app's predicate reads NSS).
- **macOS / Windows**: composed proof likely NOT non-interactively automatable (macOS `add-trusted-cert`
  needs interactive trust-setting auth + the app reads the LOGIN keychain; Windows CurrentUser\Root
  protected-root filtering ignores non-interactive raw writes). **STOP-and-surface OWNER DECISION**, three
  options: (a) a TEST-ONLY trust seam behind a new `e2e-trust` cargo feature (production predicate unchanged
  + separately unit-tested); (b) a self-hosted/pre-trusted runner; (c) documented manual pre-GA verification
  for those two OSes. Linux automated proof stands regardless; `tls_handshake.rs`/`UntrustedSkip` is
  supplementary only.

## Nomenclature checklist (root plan owns; verify ALL before RC)
Updater/promote semver comparisons (real semver); version fields (tauri.conf.json / Cargo.toml / package.json
/ NSIS / CFBundleVersion / deb/AppImage); identity-guard/marker/scheduled-task names; docs/landing/runbook/
README version strings; confirm SDK `x-aztec-version` isn't coupled to app major.

## Sequencing
1. Item 1 (config migration) — implement core-first, thread callers, mutation tests, codex loop → PR → merge.
2. Item 2+3 packaged-E2E infra (the bulk) — wire into B6's draft-gate slot; Linux composed proof automated;
   surface the mac/win owner decision.
3. Nomenclature sweep. Then the release sequence (rc → gates → ≥2h soak → promote → SDK publish → close-out).
