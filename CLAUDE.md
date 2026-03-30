# Aztec Accelerator

## Current State

- **Repo**: `alejoamiras/aztec-accelerator` (GitHub)
- **SDK** (`/packages/sdk`): TypeScript package `@alejoamiras/aztec-accelerator` — AcceleratorProver for native proving via localhost bb binary. Extends `BBLazyPrivateKernelProver`, auto-detects accelerator on port 59833, falls back to WASM if unavailable. Dual HTTP/HTTPS probe for Safari compatibility.
- **Accelerator** (`/packages/accelerator`): Tauri desktop app — system tray, multi-version bb cache, optional HTTPS (Safari), crash recovery, site authorization (MetaMask-style origin approval), Settings window, speed control, signed auto-update (Ed25519 via tauri-plugin-updater).
- **Playground** (`/packages/playground`): Vite + vanilla TS frontend — local WASM vs accelerated mode comparison, embedded wallet, ASCII animation. Deployed at `playground.aztec-accelerator.dev`.
- **Landing** (`/packages/landing`): Static landing page at `aztec-accelerator.dev`.
- **Build system**: Bun workspaces (`packages/sdk`, `packages/accelerator`, `packages/playground`, `packages/landing`)
- **Linting/Formatting**: Biome (lint + format), shellcheck, actionlint, sort-package-json, OpenTofu fmt, cargo fmt (Rust)
- **Commit hygiene**: Husky + lint-staged + commitlint (conventional commits)
- **CI**: GitHub Actions (PR gates: `accelerator.yml`, `sdk.yml`, `app.yml`, `actionlint.yml`; deploy: `deploy-landing.yml`, `publish-testnet.yml`, `publish-nightlies.yml`; reusable: `_e2e.yml`, `_e2e-app.yml`, `_e2e-webdriver.yml`, `_publish-sdk.yml`, `_aztec-update.yml`; automation: `aztec-nightlies.yml`, `aztec-stable.yml`; release: `release-accelerator.yml`)
- **Testing**: 9 WebDriver E2E tests (macOS + Linux) via `tauri-plugin-webdriver` + WebdriverIO, 28 Playwright UI mock tests, ~90 Rust unit tests, ~96 TS unit tests. WebDriver tests run as PR gate and pre-release gate.
- **TypeScript**: 6.0 with ES2025 target. Biome for lint/format.
- **Release pipeline**: `validate → e2e-webdriver gate → tag → build (3 platforms) → post-build DMG smoke → release → bump-source`
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
- **Workflow changes**: `bun run lint:actions`
- **New tests**: Run the specific test file first
- **Before pushing**: Run full `bun run test` + `bun run lint:actions`
