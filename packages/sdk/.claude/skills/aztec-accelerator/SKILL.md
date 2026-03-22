---
name: aztec-accelerator
description: Integrates the Aztec Accelerator SDK into an Aztec dApp. Covers AcceleratorProver setup, EmbeddedWallet wiring, phase callbacks for UI, Safari HTTPS compatibility, and WASM fallback patterns. Use when adding native-speed proving to an Aztec application.
argument-hint: "[setup | phases | embedded-wallet | troubleshoot]"
---

# Aztec Accelerator SDK Integration

You are helping a developer integrate `@alejoamiras/aztec-accelerator` into their Aztec dApp. The SDK provides **AcceleratorProver** — a drop-in prover that routes private kernel proving to a native desktop accelerator, with automatic WASM fallback.

## Key facts

- Package: `@alejoamiras/aztec-accelerator`
- Peer dependency: `@aztec/aztec.js` (or `@aztec/stdlib` + `@aztec/bb-prover`)
- Accelerator ports: HTTP `127.0.0.1:59833`, HTTPS `127.0.0.1:59834`
- Zero config by default — just `new AcceleratorProver()`
- Transparent fallback: if accelerator is offline, proves via WASM silently (no errors thrown)

## Step-by-step integration

### 1. Install

```bash
npm install @alejoamiras/aztec-accelerator
```

### 2. Create the prover

```typescript
import { AcceleratorProver } from "@alejoamiras/aztec-accelerator";

const prover = new AcceleratorProver();
```

That's the minimal setup. The prover auto-detects the accelerator and falls back to WASM.

### 3. Wire into EmbeddedWallet (browser dApps)

This is the recommended pattern for browser-based Aztec applications:

```typescript
import { AcceleratorProver } from "@alejoamiras/aztec-accelerator";
import { createAztecNodeClient } from "@aztec/aztec.js/node";
import { createPXE, getPXEConfig } from "@aztec/pxe/client/lazy";
import { EmbeddedWallet, WalletDB } from "@aztec/wallets/embedded";
import { createStore } from "@aztec/kv-store/indexeddb";

// 1. Prover
const prover = new AcceleratorProver();

// 2. Aztec node
const node = createAztecNodeClient(aztecNodeUrl);
const l1Contracts = await node.getL1ContractAddresses();

// 3. PXE with accelerated prover
const pxeConfig = getPXEConfig();
pxeConfig.proverEnabled = true;
pxeConfig.l1Contracts = l1Contracts;

const pxe = await createPXE(node, pxeConfig, {
  proverOrOptions: prover,  // <-- inject here
});

// 4. Wallet
const store = await createStore(`wallet-${l1Contracts.rollupAddress}`, {
  dataDirectory: "wallet",
  dataStoreMapSizeKb: 2e10,
});
const walletDB = WalletDB.init(store);
const wallet = new EmbeddedWallet(pxe, node, walletDB);
```

Every transaction through this wallet automatically uses native proving when available.

### 4. Wire into AccountManager (simpler setup)

```typescript
import { getSchnorrAccount } from "@aztec/accounts/schnorr";

const account = getSchnorrAccount(pxe, secretKey, signingKey, Fr.ZERO, prover);
const wallet = await account.getWallet();
```

### 5. Phase callbacks (UI feedback)

Register a callback to show proving progress:

```typescript
import type { AcceleratorPhase, AcceleratorPhaseData } from "@alejoamiras/aztec-accelerator";

const prover = new AcceleratorProver({
  onPhase: (phase: AcceleratorPhase, data?: AcceleratorPhaseData) => {
    updateUI(phase, data?.durationMs);
  },
});
```

**Phase sequence:**

| Phase | Meaning | `data.durationMs` |
|-------|---------|-------------------|
| `detect` | Probing accelerator health | - |
| `downloading` | Accelerator downloading bb binary for this Aztec version | - |
| `serialize` | Serializing execution steps to msgpack | - |
| `transmit` | Sending proof request | - |
| `proving` | Native (or WASM) proving in progress | - |
| `proved` | Proof complete | Server-reported proving time |
| `fallback` | Accelerator unavailable, falling back to WASM | - |
| `receive` | Deserializing proof response | - |

Use `setOnPhase(callback)` to change the callback later, or `setOnPhase(null)` to remove it.

### 6. Health check for status UI

```typescript
const status = await prover.checkAcceleratorStatus();
// status.available       — accelerator is reachable and compatible
// status.needsDownload   — accelerator needs to download bb for this version
// status.protocol        — "http" or "https" (which succeeded)
```

### 7. Force WASM mode (testing/benchmarking)

```typescript
prover.setForceLocal(true);   // bypass accelerator, use WASM
prover.setForceLocal(false);  // re-enable accelerator
```

### 8. Custom ports

```typescript
const prover = new AcceleratorProver({
  accelerator: { port: 51337, httpsPort: 51338 },
});

// Or reconfigure later (clears cached protocol)
prover.setAcceleratorConfig({ port: 51337 });
```

Environment variables also work: `AZTEC_ACCELERATOR_PORT`, `AZTEC_ACCELERATOR_HTTPS_PORT`.

## Safari compatibility

Safari blocks HTTP fetch from HTTPS pages (mixed-content). The SDK handles this automatically:

- Probes both HTTP (`:59833`) and HTTPS (`:59834`) in parallel
- Chrome/Firefox: HTTP responds first (localhost is exempt)
- Safari: HTTP fails silently, HTTPS succeeds
- The accelerator desktop app has an HTTPS toggle (generates a local-only cert)

No code changes needed in the dApp — the SDK handles protocol negotiation.

## Error handling

The SDK is designed to be fail-safe:

- **Accelerator offline**: automatically falls back to WASM (no error thrown)
- **Accelerator returns HTTP error**: falls back to WASM
- **Version mismatch**: modern accelerators auto-download the right bb version

The only cases that throw are corrupted execution steps or simulator unavailability.

## Vite configuration (browser bundling)

Aztec packages need Node.js polyfills in the browser. Use `vite-plugin-node-polyfills`:

```typescript
import { nodePolyfills } from "vite-plugin-node-polyfills";

export default defineConfig({
  plugins: [
    nodePolyfills({ include: ["buffer", "path"], globals: { Buffer: true } }),
  ],
  optimizeDeps: {
    exclude: ["@aztec/noir-acvm_js", "@aztec/noir-noirc_abi"],
  },
  server: {
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "credentialless",
    },
  },
  build: { target: "esnext" },
});
```

COOP/COEP headers are required for `SharedArrayBuffer` (used by WASM proving).

## Checklist

- [ ] `npm install @alejoamiras/aztec-accelerator`
- [ ] Create `new AcceleratorProver()` and pass to PXE or wallet
- [ ] Register `onPhase` callback for UI progress (optional)
- [ ] Use `checkAcceleratorStatus()` for status indicators (optional)
- [ ] Test with accelerator offline to verify WASM fallback works
- [ ] Configure Vite with `nodePolyfills` and COOP/COEP headers (browser apps)
