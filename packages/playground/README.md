# Aztec Accelerator Playground

Interactive web app for comparing in-browser WASM proving against native accelerated proving on Aztec. Deploy a token contract, transfer tokens, and see the speed difference side by side.

[![App](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/app.yml/badge.svg)](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/app.yml)

## Live Demo

[playground.aztec-accelerator.dev](https://playground.aztec-accelerator.dev)

## Features

- Side-by-side comparison of WASM vs accelerated proving
- Embedded wallet with in-browser PXE — no extensions required
- Token deploy and private transfer flow
- ASCII terminal animation showing proof phases in real time
- HTTPS recovery with diagnosis-specific guidance and a confirmed, current-tab-only HTTP escape hatch
- Diagnostics export for debugging

Browser proving is HTTPS-only by default. When HTTPS cannot connect, the playground keeps Local
Network Access permission guidance separate from secure-connection recovery, explains the SDK's
best available diagnosis, and offers **Retry secure connection**. **Use HTTP for this session** first
warns that private proving data may be exposed to another local user or process. On confirmation it
sets `httpsOnly: false` and `allowInsecureDowngrade: true` only on the in-memory prover and
force-refreshes status. The choice is not written to local storage, cookies, URL parameters, or
desktop configuration and resets on reload. There is no production `?httpsOnly=false` switch.

## Development

### Prerequisites

- [Bun](https://bun.sh)
- An Aztec node URL (testnet or local sandbox)

### Dev Server

```bash
bun run playground   # from repo root
# or
cd packages/playground && bun run dev
```

### Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `AZTEC_NODE_URL` | Yes | Aztec node RPC endpoint |

Injected at build time via Vite. The playground always pays fees through the **canonical salt=0 Sponsored FPC** (auto-deployed on the local sandbox; deployed + funded on v5 testnet) — no salt configuration needed.

## Testing

```bash
bun run test:unit              # Unit tests
bun run test:e2e               # E2E tests (mocked project)
bun run test:e2e:local-network # E2E tests against local Aztec sandbox
bun run test:e2e:smoke         # Smoke tests against deployed environment
```

E2E tests use [Playwright](https://playwright.dev).

## Build and Deployment

```bash
bun run build   # Output: dist/
```

Deployed with Cloudflare Workers Static Assets at `playground.aztec-accelerator.dev`. `app.yml` is the PR gate (lint, typecheck, unit, e2e). The live deploy is a manual `release-sdk.yml` dispatch: choose `sdk-and-playground` for a candidate SDK release plus deploy, or `playground-only` to deploy without publishing npm.

## License

[AGPL-3.0](../../LICENSE)
