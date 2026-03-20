# @alejoamiras/aztec-accelerator

TypeScript SDK that routes Aztec private kernel proving to a local native accelerator, bypassing browser WASM throttling. Zero-config — auto-detects the [Aztec Accelerator](../../packages/accelerator) desktop app on localhost, falls back to WASM if unavailable.

[![SDK](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/sdk.yml/badge.svg)](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/sdk.yml)
[![npm version](https://img.shields.io/npm/v/@alejoamiras/aztec-accelerator)](https://www.npmjs.com/package/@alejoamiras/aztec-accelerator)
[![npm downloads](https://img.shields.io/npm/dm/@alejoamiras/aztec-accelerator)](https://www.npmjs.com/package/@alejoamiras/aztec-accelerator)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](../../LICENSE)

## Installation

```bash
npm install @alejoamiras/aztec-accelerator
# or
bun add @alejoamiras/aztec-accelerator
```

Peer dependency: your project must already have `@aztec/aztec.js` (or the individual `@aztec/stdlib`, `@aztec/bb-prover` packages) installed.

## Quick Start

```typescript
import { AcceleratorProver } from "@alejoamiras/aztec-accelerator";

const prover = new AcceleratorProver();

// Use as the prover when creating a wallet or sending transactions
const wallet = await getSchnorrAccount(pxe, secretKey, signingKey, Fr.ZERO, prover).getWallet();
```

That's it. If the user has the [Aztec Accelerator](https://github.com/alejoamiras/aztec-accelerator/releases) desktop app running, proving happens natively at full speed. If not, it falls back to in-browser WASM proving automatically.

### Embedded Wallet (Browser dApps)

For browser-based dApps using Aztec's embedded wallet, inject the prover when creating the PXE:

```typescript
import { AcceleratorProver } from "@alejoamiras/aztec-accelerator";
import { createAztecNodeClient } from "@aztec/aztec.js/node";
import { createPXE, getPXEConfig } from "@aztec/pxe/client/lazy";
import { EmbeddedWallet, WalletDB } from "@aztec/wallets/embedded";
import { createStore } from "@aztec/kv-store/indexeddb";

// 1. Create the prover
const prover = new AcceleratorProver();

// 2. Connect to an Aztec node
const node = createAztecNodeClient("http://localhost:8080");
const l1Contracts = await node.getL1ContractAddresses();
const rollupAddress = l1Contracts.rollupAddress;

// 3. Initialize PXE with the accelerated prover
const pxeConfig = getPXEConfig();
pxeConfig.dataDirectory = `pxe-${rollupAddress}`;
pxeConfig.proverEnabled = true;
pxeConfig.l1Contracts = l1Contracts;

const pxe = await createPXE(node, pxeConfig, {
  proverOrOptions: prover, // <-- AcceleratorProver injected here
});

// 4. Create the wallet
const store = await createStore(`wallet-${rollupAddress}`, {
  dataDirectory: "wallet",
  dataStoreMapSizeKb: 2e10,
});
const walletDB = WalletDB.init(store);
const wallet = new EmbeddedWallet(pxe, node, walletDB);
```

Every transaction sent through this wallet will automatically use native proving when the accelerator is available.

## API Reference

### `AcceleratorProver`

The main class. Extends `BBLazyPrivateKernelProver` from `@aztec/bb-prover`.

```typescript
const prover = new AcceleratorProver(options?: AcceleratorProverOptions);
```

| Method | Returns | Description |
|--------|---------|-------------|
| `checkAcceleratorStatus()` | `Promise<AcceleratorStatus>` | Probe the accelerator's health endpoint. Use for UI status indicators. |
| `setAcceleratorConfig(config)` | `void` | Update connection settings (port, host). Resets cached protocol. |
| `setOnPhase(callback)` | `void` | Register a phase transition callback for UI animation. |
| `createChonkProof(steps)` | `Promise<ChonkProofWithPublicInputs>` | Generate a proof — routes to accelerator or falls back to WASM. |

### `AcceleratorProverOptions`

```typescript
interface AcceleratorProverOptions {
  simulator?: CircuitSimulator;  // Defaults to lazy-loaded WASMSimulator
  accelerator?: AcceleratorConfig;
  onPhase?: (phase: AcceleratorPhase, data?: AcceleratorPhaseData) => void;
}
```

### `AcceleratorConfig`

```typescript
interface AcceleratorConfig {
  port?: number;      // HTTP port. Default: 59833
  httpsPort?: number;  // HTTPS port (Safari). Default: 59834
  host?: string;       // Host. Default: "127.0.0.1"
}
```

### `AcceleratorStatus`

Returned by `checkAcceleratorStatus()`.

```typescript
interface AcceleratorStatus {
  available: boolean;
  needsDownload: boolean;
  acceleratorVersion?: string;
  availableVersions?: string[];
  sdkAztecVersion?: string;
  protocol?: "http" | "https";
}
```

### `AcceleratorPhase`

```typescript
type AcceleratorPhase =
  | "detect" | "serialize" | "transmit" | "proving"
  | "proved" | "receive" | "fallback" | "downloading";
```

### `AcceleratorPhaseData`

```typescript
interface AcceleratorPhaseData {
  durationMs: number;  // Actual proving duration in milliseconds
}
```

## How It Works

```
1. detect       SDK probes localhost:59833/health (HTTP + HTTPS in parallel)
2. serialize    Execution steps serialized to msgpack
3. transmit     POST /prove with x-aztec-version header
4. proving      Accelerator runs bb binary natively
5. proved       Proof returned with x-prove-duration-ms header
6. receive      SDK deserializes proof buffer
```

If the accelerator is unreachable at step 1, the SDK emits a `"fallback"` phase and proves via WASM instead — no error, no user action required.

## Configuration

### Default Ports

| Protocol | Port | Use Case |
|----------|------|----------|
| HTTP | 59833 | Chrome, Firefox (default) |
| HTTPS | 59834 | Safari (requires accelerator HTTPS mode) |

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `AZTEC_ACCELERATOR_PORT` | `59833` | Override the HTTP port |
| `AZTEC_ACCELERATOR_HTTPS_PORT` | `59834` | Override the HTTPS port |

### Programmatic Configuration

```typescript
const prover = new AcceleratorProver({
  accelerator: { port: 51337, host: "127.0.0.1" },
});

// Or update later
prover.setAcceleratorConfig({ port: 51337 });
```

## Phase Callbacks

Register a callback to animate proof progress in your UI:

```typescript
const prover = new AcceleratorProver({
  onPhase: (phase, data) => {
    console.log(`Phase: ${phase}`, data);
  },
});
```

| Phase | Meaning |
|-------|---------|
| `detect` | Probing accelerator health endpoint |
| `serialize` | Serializing execution steps to msgpack |
| `transmit` | Sending proof request to accelerator |
| `proving` | Accelerator (or WASM fallback) is proving |
| `proved` | Proof complete — `data.durationMs` has the timing |
| `downloading` | Accelerator is downloading `bb` for this Aztec version |
| `receive` | Deserializing proof from response |
| `fallback` | Accelerator unavailable, falling back to WASM |

## Browser Compatibility

| Browser | Works | Notes |
|---------|-------|-------|
| Chrome | Yes | HTTP localhost exempt from mixed-content restrictions |
| Firefox | Yes | HTTP localhost exempt from mixed-content restrictions |
| Safari | Yes* | Requires HTTPS mode enabled in the accelerator app |

Safari blocks `fetch()` from HTTPS pages to `http://127.0.0.1`. The SDK works around this by probing both HTTP and HTTPS in parallel — Chrome/Firefox use HTTP, Safari uses HTTPS. See the [accelerator README](../../packages/accelerator/README.md#safari-support-macos-only) for setup instructions.

## Version Compatibility

The SDK auto-detects its Aztec version from `@aztec/stdlib` in its dependencies and sends it as the `x-aztec-version` header on prove requests. The accelerator uses this to select (or download) the correct `bb` binary — no manual version matching needed.

## Development

```bash
bun run sdk:build     # Build the SDK
bun run test:unit     # Run unit tests (from packages/sdk)
bun run test:lint     # Typecheck
bun run test:e2e      # Run e2e tests (requires local Aztec sandbox)
```

## License

[AGPL-3.0](../../LICENSE)
