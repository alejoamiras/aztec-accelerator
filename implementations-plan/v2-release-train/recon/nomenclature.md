# Recon: v1→v2 nomenclature sweep (1.0.8-rc.1 → 2.0.0)

Agent: sonnet Explore, 2026-08-16, tree @ 0c351bc.

**Bottom line: version machinery is genuine SemVer everywhere; zero major-digit special-casing in
the whole tree (explicit digit-extraction grep → zero hits). The bump itself needs no code change.**

## Declarations — all SAFE

- tauri.conf.json:4 is THE declaration; CFBundleVersion / exe VERSIONINFO / NSIS ${VERSION} /
  deb/AppImage fields are all bundler-templated from it (no committed plists/control files).
  release-accelerator.yml:175-187 patches it generically.
- src-tauri/Cargo.toml:3 + server/Cargo.toml:3 patched in lockstep (:164-173, :326-335, :1133-1145;
  headless asserts --version == RELEASE_VERSION :349-358).
- core/Cargo.toml:3 = "1.0.5-rc.1" — NEVER patched, already 3 patches stale, verified INERT
  (core's env!(CARGO_PKG_VERSION) only reaches HeadlessState::default(), test-only; prod paths
  inject src-tauri's/server's own version: main.rs:505-506, server/src/main.rs:33,89). Cosmetic
  cleanup only.
- sdk/accelerator/playground/landing/root package.json versions: placeholders/private — SAFE.
- NSIS hooks.nsi: zero version literals (CN-based cert logic).
- Workflow regexes (:52-53 validator, :62 tag, :1099-1118 next-RC computation) generic; hand-checked
  2.0.0→2.0.1-rc.1, 2.0.0-rc.1→rc.2.

## Comparisons — all SAFE

- updater.rs: semver::Version::parse everywhere; F-004 Layer A = exact string-equality binding
  (value-agnostic); Layer B monotonic floor cmp_precedence (updater_state.rs:187-189) with tests
  covering 1.0.0→2.0.0→3.0.0 (:454-491) and rc.2 < rc.10 < stable (:719-724).
- x-aztec-version: documented orthogonal axis (version_policy.rs:240-249,277-297 explicitly REJECTS
  a newer-is-safer floor); no app-major coupling. /health api_version = constant 1 (asserted in
  release smoke :438, webdriver smoke.spec.ts:37, owner.rs:469-485 — F-03 identity checks never
  read the release version).
- config.rs config_version: written, never read — ALREADY B4 (the one NEEDS-CHANGE, tracked).

## Assumptions — none harmful

- All 1.0.x hits = test fixtures (updater_state, update_manifest, update_marker, server/tests
  HeadlessState("1.0.0"), version_policy av("2.0.0"), update-prompt.spec.ts URL params) or
  historical docs (1.0.1 amfid incident in README:23-33; README:172 example pins 1.0.6 — cosmetic).
- Tag naming accelerator-v<version>: defined once (:62), consumed prefix-match-only everywhere
  (_e2e-updater*.yml, update-feed-health.yml:70-72, landing/src/main.ts:66). SDK tag namespace
  separate. `--exclude-pre-releases` used consistently for N-1 stable lookups.

## RC vs stable — SAFE, 2.0.0-rc.N flows exactly like 1.0.8-rc.1

is_prerelease = "contains -" (:56-60); all feed-touching jobs gate on it; single feed URL; RCs get
GitHub prerelease only.

## Newly surfaced (non-blocking) — for the plan

1. **_e2e-updater-windows.yml:28-32 `n1-version` default "1.0.7"** — CORRECT for this release
   (real N-1), and the 1.0.7→2.0.0 jump is thereby exercised on Windows runners. POST-2.0.0 backlog:
   bump default to "2.0.0" + drop -N1BinaryName override (steps documented in-file; pinned by
   tauri-identity.test.ts:145-154).
2. **Landing download button surfaces RCs**: landing/src/main.ts:57-72 fetchLatestAcceleratorTag()
   uses /releases (includes prereleases), no !r.prerelease filter → during the ≥2h 2.0.0-rc.N soak
   (D-R4) real visitors get pointed at the RC installer. Pre-existing behavior, newly consequential.
   DECISION for plan: filter prereleases (small change) vs accept early exposure. Recommend filter.
3. core/Cargo.toml stale version — cosmetic bump opportunity.
4. README version examples (:23-33 archive candidate, :172 stale example) — docs pass.

## Identity/task/marker/cache keys — all product-name-keyed, never version-keyed

crash_recovery.rs:129,133,383, autostart.rs:54, update_marker.rs:43-57; drift-guards pin APP_NAME/
productName/identifier (autostart.rs:2310-2319, tauri-identity.test.ts:33-36) — none change in this
bump. rust-cache keys crate+target only (_e2e-webdriver.yml:41 "-v2" is a cache-schema salt, NOT the
product version). S3 key landing/releases/latest.json version-free (the B6 overwrite target). bb/
AZTEC_BB_VERSION fully decoupled (build.rs:113-124; aztec automation touches sdk/playground only).
Schema constants (CONFIG_VERSION, ONBOARDING_VERSION, ENVELOPE_SCHEMA, updater_state SCHEMA,
update_marker SCHEMA) version their own file formats — none change for 2.0.0.
