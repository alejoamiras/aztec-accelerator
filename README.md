# Aztec Accelerator

Native proving accelerator for Aztec transactions. Bypasses browser WASM throttling by running the `bb` proving binary natively on your machine.

[![SDK](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/sdk.yml/badge.svg)](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/sdk.yml)
[![Accelerator](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/accelerator.yml/badge.svg)](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/accelerator.yml)
[![App](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/app.yml/badge.svg)](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/app.yml)
[![npm version](https://img.shields.io/npm/v/@alejoamiras/aztec-accelerator)](https://www.npmjs.com/package/@alejoamiras/aztec-accelerator)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)

## Packages

| Package | Description | Status |
|---------|-------------|--------|
| [`@alejoamiras/aztec-accelerator`](packages/sdk) | SDK — drop-in `AcceleratorProver` for dApp integration | [![npm](https://img.shields.io/npm/v/@alejoamiras/aztec-accelerator?label=npm)](https://www.npmjs.com/package/@alejoamiras/aztec-accelerator) |
| [`packages/accelerator`](packages/accelerator) | Desktop app — macOS/Linux system tray app | [![Accelerator](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/accelerator.yml/badge.svg)](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/accelerator.yml) |
| [`packages/playground`](packages/playground) | [Live demo](https://playground.aztec-accelerator.dev) — WASM vs accelerated comparison | [![App](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/app.yml/badge.svg)](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/app.yml) |
| [`packages/landing`](packages/landing) | Landing page at [aztec-accelerator.dev](https://aztec-accelerator.dev) | |

## Architecture

```
Browser (dApp)
    │
    │  import { AcceleratorProver } from "@alejoamiras/aztec-accelerator"
    │
    ▼
┌─────────────────────────────────────────────────────────┐
│  SDK (AcceleratorProver)                                │
│  Probes localhost:59833 → accelerator found? ──────┐    │
│                            │ no                    │yes │
│                            ▼                       ▼    │
│                     WASM fallback         POST /prove   │
└─────────────────────────────────────────────────────────┘
                                                │
                                                ▼
                                    ┌───────────────────┐
                                    │  Accelerator App  │
                                    │  (system tray)    │
                                    │       │           │
                                    │       ▼           │
                                    │   bb binary       │
                                    │   (native)        │
                                    │       │           │
                                    │       ▼           │
                                    │     proof         │
                                    └───────────────────┘
```

## Quick Start

### For dApp developers (SDK)

```bash
npm install @alejoamiras/aztec-accelerator
```

```typescript
import { AcceleratorProver } from "@alejoamiras/aztec-accelerator";

// Zero-config — auto-detects accelerator, falls back to WASM
const prover = new AcceleratorProver();
```

See the [SDK README](packages/sdk/README.md) for full API reference.

### For users (Desktop App)

Download the latest release from [GitHub Releases](https://github.com/alejoamiras/aztec-accelerator/releases).

See the [Accelerator README](packages/accelerator/README.md) for installation and configuration.

## Development

```bash
bun install                              # Install dependencies
bun run test                             # Lint + typecheck + unit tests
bun run lint                             # Linting only (biome + pkg + rust)
bun run lint:fix                         # Auto-fix lint/format issues
bun run --cwd packages/playground dev    # Start playground dev server
bun run --cwd packages/sdk build         # Build SDK
```

## Contributing

This project uses [conventional commits](https://www.conventionalcommits.org/) enforced by commitlint. Husky + lint-staged run linting on pre-commit.

```bash
# Before pushing
bun run test             # Lint + typecheck + unit tests
bun run lint:actions     # Lint GitHub Actions workflows
```

| Tool | Purpose |
|------|---------|
| [Biome](https://biomejs.dev) | Linting and formatting (TS/JS/JSON) |
| [commitlint](https://commitlint.js.org) | Conventional commit message enforcement |
| [shellcheck](https://www.shellcheck.net) | Shell script linting |
| [actionlint](https://github.com/rhysd/actionlint) | GitHub Actions workflow linting |
| [cargo fmt](https://github.com/rust-lang/rustfmt) | Rust formatting (accelerator) |

## License

[AGPL-3.0](LICENSE)
