# Aztec Accelerator

Native proving accelerator for Aztec transactions. Bypasses browser WASM throttling by running the `bb` proving binary natively on your machine.

## Packages

| Package | Description |
|---------|-------------|
| [`@alejoamiras/aztec-accelerator`](packages/sdk) | SDK — `AcceleratorProver` class for dApp integration |
| [`packages/accelerator`](packages/accelerator) | Desktop app — macOS/Linux system tray app |
| [`packages/playground`](packages/playground) | Web UI — local vs accelerated proving comparison |
| [`packages/landing`](packages/landing) | Landing page at `aztec-accelerator.dev` |

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

### For users (Desktop App)

Download the latest release from [GitHub Releases](https://github.com/alejoamiras/aztec-accelerator/releases).

## Development

```bash
bun install          # Install dependencies
bun run test         # Lint + typecheck + unit tests
bun run playground   # Start playground dev server
```

## License

[AGPL-3.0](LICENSE)
