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

> The SDK's version mirrors the Aztec line it targets — SDK `X.Y.Z` is built against `@aztec/*` `X.Y.Z`. A bare install resolves npm `latest`, the release promoted as current-stable; the `testnet` dist-tag points at the newest release on that track (`npm install @alejoamiras/aztec-accelerator@testnet`). The two are usually the same version, and differ while a newer line is being validated or if `latest` has been rolled back.

The SDK ships its `@aztec/*` packages as exact-pinned **dependencies** (not peer dependencies), so it installs standalone. When your project already depends on the same exact `@aztec` version — the normal case for an Aztec dApp — npm/Bun dedupe them to a single `@aztec` graph.

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
| `checkAcceleratorStatus(options?)` | `Promise<AcceleratorStatus>` | Probe the accelerator's health endpoint. `{ forceRefresh: true }` bypasses a settled cached status but still joins a current probe. |
| `setAcceleratorConfig(config)` | `void` | Update connection and transport policy. Resets cached protocol/status. |
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

interface AcceleratorStatusCheckOptions {
  forceRefresh?: boolean;
}
```

### `AcceleratorConfig`

```typescript
interface AcceleratorConfig {
  port?: number;       // HTTP port. Default: 59833
  httpsPort?: number;  // HTTPS port. Default: 59834
  host?: string;       // Host — must be loopback. Default: "127.0.0.1"
  httpsOnly?: boolean; // Never send /prove or a witness over HTTP. Browser default: true; server default: false
  allowInsecureDowngrade?: boolean; // Permit later plaintext retry when httpsOnly is false. Default: false
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
      appVersion?: string;
      apiVersion?: number;
      protocol: AcceleratorProtocol; // "http" | "https"
    }
  | { available: false; reason: "offline"; sdkAztecVersion?: string }
  | { available: false; reason: "permission-blocked"; sdkAztecVersion?: string }
  | {
      available: false;
      reason: "secure-connection-unavailable";
      diagnosis:
        | "https-disabled"
        | "tls-or-trust-failure"
        | "accelerator-reachable"
        | "unconfirmed";
      sdkAztecVersion?: string;
    }
  | { available: false; reason: "error"; sdkAztecVersion?: string; protocol: AcceleratorProtocol }
  | {
      available: false;
      reason: "version-mismatch";
      acceleratorVersion: string;
      sdkAztecVersion?: string;
      protocol: AcceleratorProtocol;
    };
```

`permission-blocked` means the browser explicitly reports that this origin's loopback-network permission
is denied; it does **not** mean the accelerator is installed or healthy. It has no `protocol` because
neither endpoint answered. Use a forced refresh after the user changes the site permission:

```typescript
const status = await prover.checkAcceleratorStatus({ forceRefresh: true });
```

The refresh preserves the endpoint configuration, protocol pin, HTTPS history, and an existing
same-generation probe. Results, including `permission-blocked`, otherwise use the normal ten-second
status cache.

`secure-connection-unavailable` means HTTPS could not establish a connection and the current policy
will not send private proving data over HTTP. The SDK then performs at most one bounded HTTP
`GET /health` diagnostic. It sends no witness, never calls HTTP `/prove`, never pins HTTP, and never
makes that endpoint eligible for proving. The diagnosis is best-effort:

| Diagnosis | Meaning | Suggested recovery |
|-----------|---------|--------------------|
| `https-disabled` | A detailed Accelerator health response says HTTPS is disabled | Open Accelerator from the tray, open Settings, and enable **Encrypted Connection** |
| `tls-or-trust-failure` | Accelerator advertises HTTPS, but the browser could not connect securely | Re-run certificate setup in Accelerator Settings, then restart the affected browser if required |
| `accelerator-reachable` | A privacy-limited health response confirms Accelerator is reachable | Enable/check **Encrypted Connection**; approval gating prevents a more exact diagnosis |
| `unconfirmed` | The diagnostic failed, was blocked, or did not match Accelerator's health contract | Ensure Accelerator is installed and running, then check Encrypted Connection and browser permissions |

Keep Local Network Access recovery separate: an explicit permission denial is
`permission-blocked`, not a secure-connection diagnosis.

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
  | "detect" | "secure-connection-unavailable" | "serialize" | "transmit" | "proving"
  | "proved" | "receive" | "fallback" | "downloading" | "denied" | "version-mismatch";
```

### `AcceleratorPhaseData`

```typescript
interface AcceleratorPhaseData {
  durationMs: number;  // Actual proving duration in milliseconds
}
```

## How It Works

```
1. detect       SDK probes the configured loopback health endpoint
2. serialize    Execution steps serialized to msgpack
3. transmit     POST /prove with x-aztec-version header
4. proving      Accelerator runs bb binary natively
5. proved       Proof returned with x-prove-duration-ms header
6. receive      SDK deserializes proof buffer
```

If browser HTTPS cannot connect at step 1, the SDK reports
`secure-connection-unavailable`, emits `"secure-connection-unavailable"` immediately before
`"fallback"`, and proves via WASM instead. This condition does not throw and HTTP proving is not
activated automatically. Other unreachable cases continue to emit `"fallback"` and use WASM.

If the user denies your site at step 3 (or authorization times out), the SDK emits `"denied"` → `"fallback"` and falls back to WASM automatically. Use the `onPhase` callback to show a hint like "Approve in the Accelerator app for faster proving". If the accelerator refuses this SDK's Aztec version (`403 version_not_allowed`), you get `"version-mismatch"` → `"fallback"` instead.

**When proving throws.** The accelerator is an optimisation, so recognised failures — a denial, a version mismatch, an authorization cooldown, and capacity/transient errors (408/413/429/503, and `500 download_failed`/`prove_failed`) — all degrade to WASM silently. What DOES throw — surfaced as a typed [`AcceleratorHttpError`](https://github.com/alejoamiras/aztec-accelerator) (`.status`, `.code`) instead of masked as "slow but working" — is a `400` misconfiguration (`invalid_version`/`invalid_origin`), a `500` with an **unrecognised** code, or any other unexpected status. Note the degrade set is matched by status, not exhaustively by code: **every** `403` and **every** `408`/`413`/`429`/`503` falls back regardless of its `code`, so only genuinely unexpected responses reach your `catch`. Catch it if you integrate against a specific accelerator version:

```ts
import { AcceleratorHttpError } from "@alejoamiras/aztec-accelerator";
try { await prover.createChonkProof(steps); }
catch (e) { if (e instanceof AcceleratorHttpError) { /* e.status, e.code */ } }
```

`checkAcceleratorStatus()` additionally surfaces the accelerator's `appVersion` and `apiVersion` on an available result.

## Configuration

### Default Ports

| Protocol | Port | Use Case |
|----------|------|----------|
| HTTP | 59833 | Server-side clients by default; witness-free browser diagnosis; explicit browser session fallback |
| HTTPS | 59834 | Default private proving transport in browsers |

### Protocol policy (HTTP vs HTTPS)

Browser page and Web Worker instances default to `httpsOnly: true`. They probe HTTPS normally,
including one bounded startup retry. A healthy response pins HTTPS for `/prove`. An HTTPS response
that is non-`2xx`, malformed, foreign, or version-incompatible retains its existing `error` or
`version-mismatch` classification; only a connection failure triggers the witness-free HTTP
diagnostic described above.

Node, Bun, and SSR instances default to `httpsOnly: false` for compatibility with the TLS-free
headless CI server. In that mode the existing dual probe prefers a healthy HTTPS endpoint and may
select HTTP when HTTPS has never worked. An explicit option overrides the environment, and
`AZTEC_ACCELERATOR_HTTPS_ONLY` overrides the runtime default.

Once HTTPS has answered successfully at an endpoint, the preference becomes a commitment: the SDK
does not quietly re-pin HTTP if HTTPS later disappears. Changing the configured address resets that
history; changing only a policy flag does not.

The pinned protocol also drives the subsequent `/prove` request. If a pinned-HTTPS `/prove` later
fails at the network layer (trust removed, listener stopped), the SDK falls back to WASM rather than
retrying the same private witness over plaintext HTTP — **once HTTPS has worked at an endpoint, the
SDK will not downgrade it.** Any local account can bind `127.0.0.1:59833`, and the health-contract
check is collision resistance rather than authentication, so that plaintext retry was reachable by a
different user on the same machine. A deliberate HTTP opt-in must set both policy flags as described
below.

**Every connection setting is validated at runtime, not just by its type.** TypeScript types are
erased at runtime, so a value that arrives from JSON, an env var, or plain JavaScript is whatever it
is — and `/prove` carries the private witness, so a setting that re-points the URL sends it off the
machine.

- `host` is parsed with `URL` and rejected if it carries a port, path, query, fragment, or
  credentials, or resolves anywhere but `127.0.0.0/8`, `::1`, or `localhost`. The accelerator's CA is
  name-constrained to loopback, so a remote host could not present a certificate this SDK's trust
  model would accept anyway.
- `port` and `httpsPort` must be integers in 1–65535. A string here is not harmless: `"80@evil.com"`
  turns `http://127.0.0.1:80@evil.com/prove` into an authority of `evil.com`, with `127.0.0.1`
  demoted to a username — past the host check entirely.
- `httpsOnly` and `allowInsecureDowngrade` must be actual booleans. `"false"` is truthy, so coercing
  it would switch the opt-out **on** via a value that reads as off.

A rejected setting throws and changes nothing at all — the transport is left exactly as it was,
including its cached status and negotiated protocol.

**Private transport policy (`httpsOnly`).** Browser dApps get this policy by default. An
unreachable/untrusted HTTPS accelerator (or a mid-proof network failure) degrades to WASM rather
than sending the witness over plaintext.

```typescript
const prover = new AcceleratorProver({ accelerator: { httpsOnly: true } });
```

> **What `httpsOnly` does and doesn't guarantee.** It guarantees no private proving payload and no
> `/prove` request is ever sent over HTTP. After an HTTPS connection failure, the SDK may send one
> witness-free HTTP `GET /health` diagnostic under the same timeout, response-size, redirect, host,
> CORS, and health-shape protections. That response can only improve the diagnosis; it cannot select
> HTTP for proving. TLS + the name-constrained local CA authenticate that the endpoint presented a
> certificate your browser trusts for `127.0.0.1`. It is **not** cryptographic pairing with a
> specific accelerator install: any same-machine process that obtained a browser-trusted certificate
> for localhost and squats the HTTPS port is past this line. The health-contract check is collision
> resistance against accidental squatters, not authentication of a deliberate one.

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `AZTEC_ACCELERATOR_PORT` | `59833` | Override the HTTP port |
| `AZTEC_ACCELERATOR_HTTPS_PORT` | `59834` | Override the HTTPS port |
| `AZTEC_ACCELERATOR_HTTPS_ONLY` | Browser: `true`; Node/Bun/SSR: `false` | `1`/`true` or `0`/`false` overrides the runtime default |
| `AZTEC_ACCELERATOR_ALLOW_INSECURE_DOWNGRADE` | `false` | `1`/`true` → after a healthy HTTPS endpoint fails mid-proof, allow the plaintext HTTP retry |

### Programmatic Configuration

```typescript
const prover = new AcceleratorProver({
  accelerator: { port: 51337, host: "127.0.0.1" },
});

// Or update later
prover.setAcceleratorConfig({ port: 51337 });
```

### Explicit HTTP fallback for the current session

A browser dApp may offer HTTP only after an informed user confirmation. Apply the choice to the
current prover instance, then force-refresh status:

```typescript
prover.setAcceleratorConfig({
  httpsOnly: false,
  allowInsecureDowngrade: true,
});
await prover.checkAcceleratorStatus({ forceRefresh: true });
```

Use a warning such as: “HTTP can expose private proving data to another local user or process. Use
it only if you accept this risk for the current tab.” Do not store the decision in local storage,
cookies, URL parameters, or desktop configuration. A reload or new `AcceleratorProver` restores the
browser HTTPS-only default. There is intentionally no production `?httpsOnly=false` switch.

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
| `secure-connection-unavailable` | HTTPS could not connect; emitted immediately before WASM `fallback` |
| `serialize` | Serializing execution steps to msgpack |
| `transmit` | Sending proof request to accelerator |
| `proving` | Accelerator (or WASM fallback) is proving |
| `proved` | Proof complete — `data.durationMs` has the timing |
| `downloading` | Accelerator is downloading `bb` for this Aztec version |
| `receive` | Deserializing proof from response |
| `fallback` | Accelerator unavailable, falling back to WASM |
| `denied` | User denied this site access to the accelerator (403) — falling back to WASM |
| `version-mismatch` | Accelerator refused this SDK's Aztec version (403) — falling back to WASM |

## Browser Compatibility

| Browser | Works | Notes |
|---------|-------|-------|
| Chrome | Yes* | Uses trusted HTTPS by default; Chrome 142+ shows a Local Network Access permission prompt (see below) |
| Firefox | Yes* | Uses trusted HTTPS by default; Firefox 153+ enables Local Network Access prompting by default (see below) |
| Safari | Yes* | Requires Encrypted Connection (HTTPS) in the accelerator app |

All browsers require a trusted [Encrypted Connection
(HTTPS)](../accelerator/README.md#encrypted-connection-https) for native proving by default. Chrome
and Firefox can use HTTP only after the dApp obtains explicit, session-scoped consent. Safari may
block the HTTP diagnostic and fallback entirely when the dApp itself is served over HTTPS.

### Browser Local Network Access (Chrome 142+, Firefox 153+)

Current Chrome and Firefox gate requests from a public website to loopback addresses behind a **Local Network Access permission prompt**. Chrome introduced the prompt in 142 and split it into `local-network` and `loopback-network` permissions in 145; Firefox enables its corresponding protection by default in 153. This applies to the SDK's health probe and prove requests:

- If the user **allows**, the HTTPS path can proceed.
- If the browser's permission state is explicitly **denied**, status is `{ available: false, reason: "permission-blocked" }`; proving still falls back to WASM. Under the browser HTTPS-only default, a prompt that remains open, is dismissed without a persisted denial, or cannot be queried is inconclusive and normally appears as `secure-connection-unavailable` with `diagnosis: "unconfirmed"`.
- The usual recovery is to open the site's permissions beside the address bar, allow local network or device access, then call `checkAcceleratorStatus({ forceRefresh: true })`. This is not guaranteed: managed policy may require an administrator, and an iframe may need top-level access or an appropriate Permissions Policy delegation.

The SDK adds `targetAddressSpace: "loopback"` to supported plaintext Fetch requests. That declares
intent so supporting browsers can apply their mixed-content/LNA flow; it does **not** grant or bypass permission.
The gate is about the destination address space, not the scheme, so HTTPS is not an escape hatch.

## Security: local service discovery is shape-matched, not authenticated

The SDK discovers the accelerator by probing fixed loopback ports and accepting any `/health`
response matching `{"status":"ok","api_version":1}`. That is a **shape check, not server
authentication**: while the accelerator app is *not running*, any local process can bind the port,
answer the probe, and receive whatever your dApp sends to `/prove` (the witness data). This
inherits from the unauthenticated-localhost-service model every browser extension and dev tool
shares; it is documented as an accepted trust boundary in the project's
[security report](../../audit/security/2026-08-21-independent-hardening/report.md).

Practical guidance for integrators:

- **Run the accelerator** — and keep Encrypted Connection enabled and trusted.
- **Prefer the encrypted path**: when HTTPS mode is enabled in the accelerator app, its TLS
  leaf certificate chains to a name-constrained local CA and its private key is stored owner-only,
  so another *user* on a shared machine cannot impersonate the HTTPS endpoint by merely squatting
  the port. Same-user malware is out of scope for any localhost service.
- Browser SDK instances are HTTPS-only by default. The only automatic HTTP request after an HTTPS
  connection failure is the witness-free liveness diagnostic; it can never lead to HTTP `/prove`.
- If a dApp offers plaintext proving, require informed consent and set both `httpsOnly: false` and
  `allowInsecureDowngrade: true` only on the current prover instance.

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
