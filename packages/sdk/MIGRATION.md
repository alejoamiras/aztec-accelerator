# Migration guide

## `AcceleratorStatus` is now a discriminated union (Q12)

`AcceleratorStatus` (returned by `AcceleratorProver.checkAcceleratorStatus()`) changed from a flat
interface — where every field was optional and illegal combinations typechecked — to a **discriminated
union on `available`**. The HTTP wire contract is unchanged; this is a TypeScript-only break.

### Before

```ts
interface AcceleratorStatus {
  available: boolean;
  needsDownload: boolean;
  acceleratorVersion?: string;
  availableVersions?: string[];
  sdkAztecVersion?: string;
  protocol?: "http" | "https";
}
```

### After

```ts
type AcceleratorStatus =
  | {
      available: true;
      needsDownload: boolean;
      acceleratorVersion?: string;
      availableVersions?: string[];
      sdkAztecVersion?: string;
      protocol: AcceleratorProtocol;            // "http" | "https"
    }
  | { available: false; reason: "offline"; sdkAztecVersion?: string }
  | { available: false; reason: "error"; protocol: AcceleratorProtocol; sdkAztecVersion?: string }
  | {
      available: false;
      reason: "version-mismatch";
      acceleratorVersion: string;
      protocol: AcceleratorProtocol;
      sdkAztecVersion?: string;
    };
```

### What to change

**Narrow on `available` before reading state-specific fields.** Accessing `needsDownload`,
`availableVersions`, or `acceleratorVersion` without narrowing is now a type error — which is the point:
those fields were never meaningful on an unavailable result.

```ts
// Before — fields read without narrowing
const status = await prover.checkAcceleratorStatus();
if (status.available && !status.needsDownload) {
  /* ... */
}

// After — narrow first; the compiler then exposes exactly the valid fields
const status = await prover.checkAcceleratorStatus();
if (status.available) {
  // status.needsDownload, status.availableVersions, status.protocol available here
  if (!status.needsDownload) {
    /* ... */
  }
} else {
  // status.reason: "offline" | "error" | "version-mismatch"
  switch (status.reason) {
    case "version-mismatch":
      console.warn(`accelerator is on ${status.acceleratorVersion}, SDK wants ${status.sdkAztecVersion}`);
      break;
    case "offline":
    case "error":
      // fall back to WASM
      break;
  }
}
```

Most callers that already wrote `if (status.available) { … }` need **no change** — the narrowing they
already do is exactly what the union requires. Only code that read `needsDownload`/version fields
*without* first checking `available` must add the narrowing.

The new `AcceleratorProtocol` type (`"http" | "https"`) is exported for convenience.

## New: typed `AcceleratorHttpError` + surfaced health fields (B7)

### Prove errors now degrade to WASM or throw a TYPED error — never a raw `ky` error

The accelerator is an optimisation, so `createChonkProof` **falls back to WASM** for every recognised
transient/denial/capacity/version condition (403 denial, `version_not_allowed`, `authorization_cooldown`,
408, 413, 429, 503, and 500 `download_failed`/`prove_failed`). What used to leak a raw `ky` `HTTPError` to
your dApp — a caller **misconfiguration** (`400 invalid_version` / `invalid_origin`) or an unrecognised
status/code — now throws a typed [`AcceleratorHttpError`] (exported from the barrel) with `.status` and
`.code`, so a real integration bug is surfaced instead of masked as "slow but working":

```ts
import { AcceleratorHttpError } from "@alejoamiras/aztec-accelerator";

try {
  await prover.createChonkProof(steps);
} catch (e) {
  if (e instanceof AcceleratorHttpError) {
    // misconfiguration — e.status (e.g. 400), e.code (e.g. "invalid_version")
  }
}
```

If you never inspected the raw thrown error, nothing changes — recognised failures still fall back silently.

### New `"version-mismatch"` phase

`onPhase` may now emit `"version-mismatch"` (distinct from `"denied"`) when the accelerator refuses this
SDK's Aztec version (`403 version_not_allowed`). The proof still degrades to WASM.

### `AcceleratorStatus` gains `appVersion` / `apiVersion`

The `available: true` status now also carries the accelerator app's own `appVersion` and the negotiated
`apiVersion` (both optional — the origin-tiered minimal `/health` may withhold them). Additive; no break.
