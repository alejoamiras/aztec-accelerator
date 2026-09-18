# Phases 2–6 — Composite extension + workflow refactors + per-job perms

Implemented as a single feature branch `ci/dedup-setup-accelerator-composite`.

## Phase 2 — Composite extended

`.github/actions/setup-accelerator/action.yml`:
- Added 3 inputs (all optional): `rust-target`, `rust-cache-key`, `rust-components` (default `clippy,rustfmt`).
- Added `Assert host matches rust-target` fail-fast guard when `rust-target != ""` — derives host triple from `runner.os` + `runner.arch` and exits non-zero on mismatch. (Codex's final-pass should-fix.)
- Gated the Linux `apt-get install` on `runner.os == 'Linux'`. Defense-in-depth for future macOS callers — no current macOS caller exists (release-smoke is ubuntu-latest, not macOS as v1 plan inaccurately claimed).
- Step order: host check → apt (Linux) → bun setup + cache + install → rust-toolchain → rust-cache → prebuild. Rust-toolchain + rust-cache stay BEFORE the prebuild and BEFORE any Cargo.toml mutation in callers. Cache auto-key derives from un-mutated `Cargo.toml` — preserves cache hit rate across release versions. (Codex's first-pass must-fix.)
- Composite has no `secrets.*` references — signing secrets remain caller-scoped.

## Phase 3 — `build` job refactored

`.github/workflows/release-accelerator.yml`:
- Replaced inline `dtolnay/rust-toolchain` + `Swatinem/rust-cache` + `oven-sh/setup-bun` + `actions/cache` + `bun install --frozen-lockfile` + Linux apt + `Copy bb sidecar` steps with a single `uses: ./.github/actions/setup-accelerator` block.
- Preserved version-patching steps (`Patch version in Cargo.toml` + `Patch version in tauri.conf.json`) AFTER the composite call — keeps rust-cache auto-key stable.
- `Build Tauri bundle` step unchanged (signing secrets still job-scoped).
- Added `permissions: contents: read` to the job (Phase 6).

## Phase 4 — `build-headless` job refactored

Same pattern. No tauri.conf.json patch. Job already had `permissions: contents: read`.

## Phase 5 — `_e2e-webdriver.yml` refactored

- Replaced inline env steps with composite call.
- Kept WebDriver-specific extras inline: `xvfb stalonetray dbus-x11` apt install + Xvfb/dbus/tray startup. Removed the duplicate Tauri Linux deps (`libwebkit2gtk-4.1-dev` etc.) — composite installs them.
- The `working-directory: .` override on the apt install step is required to escape the workflow-level `defaults.run.working-directory: packages/accelerator`.
- Cache key: `e2e-webdriver-${{ inputs.mode }}` (preserves existing isolation).
- Added `permissions: contents: read` to the job.

## Phase 6 — Per-job permission tightening

`release-accelerator.yml`:
- Dropped workflow-level `id-token: write`, `contents: write`, `pull-requests: write` → `contents: read`.
- Per-job grants:
  - `validate`: `contents: read`
  - `build`: `contents: read` (added)
  - `build-headless`: `contents: read` (kept)
  - `smoke`: `contents: read`
  - `tag`: `contents: write` (needs push)
  - `release`: `contents: write` (gh release create) + `id-token: write` (OIDC for AWS)
  - `bump-source`: `contents: write` (commit + push) + `pull-requests: write` (gh pr create)
- `e2e-webdriver` job (in `_e2e-webdriver.yml`) gets `contents: read`.

Caller-of-reusable-workflow inheritance: `release-accelerator.yml`'s `e2e-webdriver:` step calls `_e2e-webdriver.yml` which now declares its own `permissions:` — supersedes inheritance.

## Validation

- `actionlint` passes for all 3 modified files.
- Diff stats: composite +63 / -22, release-accelerator -52, _e2e-webdriver -32 / +6. Net workflow lines: -91 added, +94 removed (clean dedup).
- PR-gate compatibility: composite defaults (`rust-components: clippy,rustfmt`, empty `rust-target`, empty `rust-cache-key`) preserve current PR-gate behavior for all 6 consumers in `accelerator.yml`.

## Skipped from initial v2 plan

- Phase 9 (SHA-pinning + CODEOWNERS) intentionally split into a separate follow-up PR per codex's recommendation — keeps this refactor PR reviewable.

## Next: open PR → Phase 7 (PR-gate validation).
