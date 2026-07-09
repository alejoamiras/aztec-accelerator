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

> The bare install resolves npm `latest`, which tracks the last **stable** Aztec line. For the current **Aztec 5.0** line (`5.0.0-rc.x`), install the `testnet` dist-tag: `npm install @alejoamiras/aztec-accelerator@testnet`.

Peer dependency: your project must already have `@aztec/aztec.js` (or the individual `@aztec/stdlib`, `@aztec/bb-prover` packages) installed.

## Quick Start

```typescript
import { AcceleratorProver } from "@alejoamiras/aztec-accelerator";

// Zero-config — auto-detects the accelerator, falls back to WASM.
const prover = new AcceleratorProver();
```

Inject `prover` into your wallet through the PXE `proverOrOptions` option — see [Embedded Wallet](#embedded-wallet-browser-dapps) below. Every transaction then proves natively when the [Aztec Accelerator](https://github.com/alejoamiras/aztec-accelerator/releases) desktop app is running, and falls back to in-browser WASM automatically. No other code changes.

### Embedded Wallet (Browser dApps)

For browser-based dApps using Aztec's embedded wallet, inject the prover via the unified `pxe` option:

```typescript
import { AcceleratorProver } from "@alejoamiras/aztec-accelerator";
import { EmbeddedWallet } from "@aztec/wallets/embedded";

const wallet = await EmbeddedWallet.create("http://localhost:8080", {
  pxe: {
    proverEnabled: true,
    proverOrOptions: new AcceleratorProver(),
  },
});
```

Every transaction sent through this wallet will automatically use native proving when the accelerator is available, and fall back to WASM otherwise.

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
| `setForceLocal(force)` | `void` | Force WASM proving, bypassing accelerator detection (testing). |

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

Returned by `checkAcceleratorStatus()`. A **discriminated union** on `available` — narrow on `available`
first (and on `reason` for the unavailable cases) so you only access fields valid for that state. (The
prior flat interface let illegal field combinations typecheck; this is the post-Q12 shape — see
[MIGRATION.md](./MIGRATION.md).)

```typescript
type AcceleratorStatus =
  | {
      available: true;
      needsDownload: boolean;        // must download bb for the SDK's Aztec version before proving
      acceleratorVersion?: string;   // from /health (single-version protocol)
      availableVersions?: string[];  // cached versions (multi-version protocol)
      sdkAztecVersion?: string;
      protocol: AcceleratorProtocol; // "http" | "https"
    }
  | { available: false; reason: "offline"; sdkAztecVersion?: string }
  | { available: false; reason: "error"; sdkAztecVersion?: string; protocol: AcceleratorProtocol }
  | {
      available: false;
      reason: "version-mismatch";
      acceleratorVersion: string;
      sdkAztecVersion?: string;
      protocol: AcceleratorProtocol;
    };
```

> **Origin approval affects `/health` detail.** Before the user approves your dApp's origin in the
> accelerator popup, `/health` returns a *minimal* body, so `needsDownload` / `availableVersions` /
> `acceleratorVersion` may be absent (and `needsDownload` can read `false` even though `bb` will
> download on first use). After the user clicks **Allow**, the full status is reported. Proving works
> in both cases — an unapproved-origin proof triggers an on-demand `bb` download rather than surfacing
> the hint up front. (Applies to accelerators from the 2026-06 security-hardening release onward.)

### `AcceleratorProtocol`

```typescript
type AcceleratorProtocol = "http" | "https";
```

### `AcceleratorPhase`

```typescript
type AcceleratorPhase =
  | "detect" | "serialize" | "transmit" | "proving"
  | "proved" | "receive" | "fallback" | "downloading" | "denied";
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

If the user denies your site at step 3 (or authorization times out), the SDK emits `"denied"` → `"fallback"` and falls back to WASM automatically. Use the `onPhase` callback to show a hint like "Approve in the Accelerator app for faster proving".

## Configuration

### Default Ports

| Protocol | Port | Use Case |
|----------|------|----------|
| HTTP | 59833 | Chrome, Firefox (fallback / when HTTPS isn't trusted) |
| HTTPS | 59834 | Preferred when the accelerator's certificate is trusted; required for Safari |

### Protocol preference (HTTP vs HTTPS)

The SDK probes both endpoints in parallel and **prefers HTTPS when it's healthy** — a `/health` that
responds `200` with a parseable body. Concretely:

- If HTTPS answers healthy, it wins (encrypted channel) — even if HTTP answered first.
- If HTTPS is absent or its certificate isn't trusted in this browser, its probe fails fast and
  **HTTP wins with no added latency** (the common Chrome/Firefox path is unchanged).
- A HTTPS endpoint that answers non-`2xx` or with a malformed body does **not** win over a healthy
  HTTP endpoint (guards against another process answering on the HTTPS port).
- Safari blocks HTTP-from-HTTPS, so HTTPS is the only responder there.

The pinned protocol also drives the subsequent `/prove` request.

**Strict mode (`httpsOnly`).** dApps that require an encrypted, authenticated channel can force it:
the SDK then probes and POSTs over HTTPS **only**, never constructing an `http://` URL and never
falling back — an unreachable/untrusted HTTPS accelerator simply reports offline (→ WASM fallback).

```typescript
const prover = new AcceleratorProver({ accelerator: { httpsOnly: true } });
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `AZTEC_ACCELERATOR_PORT` | `59833` | Override the HTTP port |
| `AZTEC_ACCELERATOR_HTTPS_PORT` | `59834` | Override the HTTPS port |
| `AZTEC_ACCELERATOR_HTTPS_ONLY` | `false` | `1`/`true` → strict HTTPS-only transport (no HTTP fallback) |

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
| `denied` | User denied this site access to the accelerator (403) — falling back to WASM |

## Browser Compatibility

| Browser | Works | Notes |
|---------|-------|-------|
| Chrome | Yes* | HTTP localhost exempt from mixed-content restrictions; Chrome 142+ shows a Local Network Access permission prompt (see below) |
| Firefox | Yes | HTTP localhost exempt from mixed-content restrictions |
| Safari | Yes* | Requires HTTPS mode enabled in the accelerator app |

Safari blocks `fetch()` from HTTPS pages to `http://127.0.0.1`. The SDK works around this by probing both HTTP and HTTPS in parallel — Chrome/Firefox use HTTP, Safari uses HTTPS. See the [accelerator README](../../packages/accelerator/README.md#safari-support-macos-only) for setup instructions.

### Chrome Local Network Access (Chrome 142+)

Starting with Chrome 142 (October 2025), requests from a public website to loopback addresses are gated behind a **Local Network Access permission prompt** ("… wants to access devices on your local network"). Chrome 145 splits this into separate `local-network` and `loopback-network` permissions. This applies to the SDK's health probe and prove requests:

- If the user **allows**, everything works as before.
- If the user **blocks** (or dismisses) the prompt, the probe fails and the SDK reports the accelerator as unavailable — proving silently falls back to WASM (`fallback` phase), which is indistinguishable from the accelerator not running. To recover, the user must re-allow the permission via the icon in Chrome's address bar (Site settings).

Note this is about the destination address space, not the scheme — enabling HTTPS mode on the accelerator does **not** bypass the prompt.

## Version Compatibility

The SDK auto-detects its Aztec version from `@aztec/stdlib` in its dependencies and sends it as the `x-aztec-version` header on prove requests. The accelerator uses this to select (or download) the correct `bb` binary — no manual version matching needed.

## Claude Code Skill

This SDK ships with a [Claude Code](https://claude.com/claude-code) skill at `.claude/skills/aztec-accelerator/`. If you're using Claude Code in a project that depends on this SDK, the `/aztec-accelerator` slash command gives Claude full context on the integration patterns — EmbeddedWallet wiring, phase callbacks, Safari compatibility, Vite config, and more.

To use it in your own project, copy the skill directory:

```bash
mkdir -p .claude/skills
cp -r node_modules/@alejoamiras/aztec-accelerator/.claude/skills/aztec-accelerator .claude/skills/
```

Then use `/aztec-accelerator` in Claude Code for guided integration.

## Development

```bash
bun run --cwd packages/sdk build   # Build the SDK
bun run --cwd packages/sdk test:unit   # Run unit tests
bun run --cwd packages/sdk test:lint   # Typecheck
bun run --cwd packages/sdk test:e2e    # Run e2e tests (requires local Aztec sandbox)
```

## License

[AGPL-3.0](../../LICENSE)
