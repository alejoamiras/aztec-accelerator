# Aztec Accelerator

## Current State

- **Repo**: `alejoamiras/aztec-accelerator` (GitHub)
- **SDK** (`/packages/sdk`): TypeScript package `@alejoamiras/aztec-accelerator` — AcceleratorProver for native proving via localhost bb binary. Extends `BBLazyPrivateKernelProver`, auto-detects accelerator on port 59833, falls back to WASM if unavailable. Dual HTTP/HTTPS probe that **prefers HTTPS when healthy** (ok + parseable `/health`) with a 250 ms stall bound and zero added latency when HTTPS is absent; opt-in strict `httpsOnly` mode (`AZTEC_ACCELERATOR_HTTPS_ONLY`).
- **Accelerator** (`/packages/accelerator`): Tauri desktop app (macOS + Linux + Windows) — executable named `AztecAccelerator` (renamed at the CARGO layer, `[[bin]]` + `default-run`, so plain-cargo and tauri builds agree; `productName`/identifier/install dir unchanged; `tauri-identity.test.ts` pins the tuple and `bundle.publisher` stays absent until it ships with an NSIS registry-migration story), system tray, multi-version bb cache, **HTTPS default-on** consented via a first-run onboarding wizard (Encrypted Connection / Start-on-Login / Auto-Update), crash recovery, site authorization (MetaMask-style origin approval, **deny-by-default**; **desktop prompts-once for localhost, headless auto-approves it**; `url::Url`-based canonicalization at server ingress + curated verified-sites registry rendering friendly name + ✓ in the popup; see `VERIFIED_SITES.md`), a **loopback `Host`-allowlist guard at ingress** (anti-DNS-rebinding; exact host+port, both HTTP/HTTPS listeners), **Origin-tiered `/health`** (minimal body for unapproved origins), Settings window, speed control, signed auto-update (Ed25519 via tauri-plugin-updater, with a pre-flight artifact size cap). **Cross-OS browser trust** (`src-tauri/src/trust/`): keyless local CA (name-constrained to loopback) installed into the macOS Keychain / Windows CurrentUser Root / Linux user NSS DBs (`certutil`, no root); rename `safari_support`→`https_enabled`; renewal consent window (macOS/Windows) vs silent rotation (Linux); NSIS uninstall hook (`$UpdateMode`-guarded). Headless server (`/packages/accelerator/server`) is deny-by-default with `ALLOWED_ORIGINS` / `--allow-all` opt-in, and stays TLS-free.
- **Playground** (`/packages/playground`): Vite + vanilla TS frontend — local WASM vs accelerated mode comparison, embedded wallet, ASCII animation. Deployed at `playground.aztec-accelerator.dev`.
- **Landing** (`/packages/landing`): Static landing page at `aztec-accelerator.dev`.
- **Build system**: Bun workspaces (`packages/sdk`, `packages/accelerator`, `packages/playground`, `packages/landing`)
- **Linting/Formatting**: Biome (lint + format), shellcheck, actionlint, sort-package-json, OpenTofu fmt, cargo fmt (Rust)
- **Commit hygiene**: Husky + lint-staged + commitlint (conventional commits)
- **CI**: GitHub Actions (required PR gates: `accelerator.yml`, `sdk.yml`, `app.yml`, `actionlint.yml`; accelerator jobs use the central `.github/filters/accelerator.yml` routing contract; dependency audits are PR-path-filtered but remain unconditional for schedules, manual runs, and release calls; PR Rust caches are restore-only). Reusable workflows include `_e2e.yml`, `_e2e-app.yml`, `_e2e-webdriver.yml`, and the macOS/Linux/Windows updater smokes, which use the greatest complete published lower same-key baseline (including prereleases) and fail closed if none exists. `release-accelerator.yml` has no evergreen key-rotation override; any future key migration requires a reviewed workflow change. `smoke-updater-windows.yml` remains the secretless ephemeral L8 barrier smoke.
- **Testing**: 17 WebDriver E2E tests (macOS + Linux + Windows) via `tauri-plugin-webdriver` + WebdriverIO, 66 Playwright UI mock tests, 435 Rust tests (417 non-ignored; incl. `win_acl` inline DACL tests on the windows-build lane), ~280 TS unit tests (SDK 107 + accelerator scripts 53 + root scripts 64 + playground 55), plus `#[ignore]`d real-OS integration suites run per-OS in CI (`trust_*`, `autostart_heal`, `uninstall_ownership`). WebDriver tests run as PR gate and pre-release gate.
- **TypeScript**: 6.0 with ES2025 target. Biome for lint/format.
- **Release pipeline (B6 publish/promote split)**: two dispatches. `mode=publish` (default) runs `validate → e2e-webdriver gate → build (3 Tauri + 4 headless platforms) → smoke → tag → sign-update-feed (stable) → release` and publishes the GitHub release **without touching the live feed** — stable is `--latest=false`, with the signed `latest.json` shipped as a release asset. A separate `mode=promote-only` dispatch re-verifies the published release (published/non-draft/non-prerelease, full asset set, production Ed25519 verifier over the release's own feed) then flips the S3 `latest.json` (`promote → verify-live-feed → bump-source` on organic GA). `promote-only <prev>` is the rollback lever; `dry_run` runs the pre-flight with no feed write. Prerelease `X.Y.Z-rc.N` publishes as a public prerelease and is never promoted. **Append-only**: no release/tag deletion (fix-forward on a colliding tag). The landing download version is derived from the signed feed, not the GitHub Latest badge.
- **Infrastructure** (`/infra/tofu`): S3 + CloudFront for static site hosting. CloudFront function routes by Host header: `aztec-accelerator.dev` → `/landing/`, `playground.aztec-accelerator.dev` → `/playground/`

## Quick Start

```bash
bun install              # Install dependencies
bun run test             # Full checks (lint + typecheck + unit tests)
bun run lint             # Linting only (biome + pkg + rust)
bun run lint:actions     # Lint GitHub Actions workflows
bun run lint:fix         # Auto-fix lint/format issues
bun run --cwd packages/sdk build          # Build SDK
bun run --cwd packages/playground dev     # Playground (default)
bun run --cwd packages/playground dev:localhost  # Playground -> localhost
bun run --cwd packages/playground dev:testnet    # Playground -> testnet
```

## Development Principles

1. **Iterative implementation**: Break into small, testable steps
2. **Research first**: Understand the current system before changing it
3. **Test at each step**: Verify before moving on
4. **Prefer Bun native APIs**: Use Bun APIs over Node.js compat or third-party packages

## Workflow

Before writing any code:
1. Read relevant source files and existing tests
2. Create a task list breaking work into incremental steps
3. Work through the list one step at a time, validating after each

### Validation

- **Code changes**: `bun run lint` and `bun run test`
- **Platform-gated Rust** (`#[cfg(windows)]` / `target_os` branches): also
  `cargo check --target x86_64-pc-windows-gnu --lib` from `src-tauri` — Linux-only checks cannot see
  a Windows compile break (twice bitten: a `pub(crate)` visibility error, and a `cfg` attribute
  detached from its function). Needs a one-off empty, gitignored
  `binaries/bb-x86_64-pc-windows-gnu.exe` placeholder for tauri-build's sidecar check; ~5s
  incremental. Clippy warnings on that target are test-only noise (CI's clippy gate is ubuntu-only).
- **Workflow changes**: `bun run lint:actions`
- **New tests**: Run the specific test file first
- **Before pushing**: Run full `bun run test` + `bun run lint:actions`
