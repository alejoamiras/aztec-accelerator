import type { CircuitSimulator } from "@aztec/simulator/client";

// q7e3-F-02: the SDK's published types live here (a neutral module), not inside the
// `accelerator-prover.ts` hotspot. `index.ts` re-exports them unchanged; `accelerator-transport.ts`
// imports them here instead of back-importing from the prover — killing the former 2-way edge.

/** Sub-phases emitted during proof generation for UI animation. */
export type AcceleratorPhase =
  | "detect"
  | "serialize"
  | "transmit"
  | "proving"
  | "proved"
  | "receive"
  | "fallback"
  | "downloading"
  | "denied";

/** Data payload for the `"proved"` phase — carries the actual proving duration. */
export interface AcceleratorPhaseData {
  durationMs: number;
}

export interface AcceleratorConfig {
  /** Port the accelerator listens on (HTTP). Default: 59833. */
  port?: number;
  /** Port the accelerator listens on (HTTPS — required for Safari; preferred elsewhere when trusted). Default: 59834. */
  httpsPort?: number;
  /** Host the accelerator binds to. Default: "127.0.0.1". */
  host?: string;
  /**
   * Strict transport policy: talk to the accelerator over HTTPS ONLY. The SDK never probes or POSTs
   * to the `http://` endpoint and never falls back to it — an unreachable HTTPS accelerator reports
   * as offline (→ WASM fallback). Off by default (the SDK prefers HTTPS when healthy but still uses
   * HTTP otherwise). Also settable via `AZTEC_ACCELERATOR_HTTPS_ONLY=1`. Default: false.
   */
  httpsOnly?: boolean;
  /**
   * Allow the SDK to fall back to the plaintext `http://` endpoint **after it has already reached a
   * healthy `https://` accelerator at this address**. Off by default (F-01, audit 2026-07-31).
   *
   * This is not the same knob as {@link AcceleratorConfig.httpsOnly}. An accelerator with HTTPS
   * disabled — the user's own choice in the onboarding wizard, and the permanent state of the
   * TLS-free headless server — is unaffected: the SDK never saw a healthy HTTPS endpoint, so there is
   * nothing to downgrade FROM and it uses HTTP exactly as before. What this governs is the narrower
   * case where HTTPS *was* working and then a `/prove` fails at the network layer: without it the SDK
   * used to retry the same private witness over plaintext HTTP, and any local account can bind
   * 127.0.0.1:59833 to receive it. Turn it on only if you would rather have the proof than the
   * confidentiality. Also settable via `AZTEC_ACCELERATOR_ALLOW_INSECURE_DOWNGRADE=1`.
   *
   * Default: false.
   */
  allowInsecureDowngrade?: boolean;
}

export interface AcceleratorProverOptions {
  /** Circuit simulator. Defaults to WASMSimulator (lazy-loaded from @aztec/simulator/client). */
  simulator?: CircuitSimulator;
  /** Accelerator connection config (port, host). */
  accelerator?: AcceleratorConfig;
  /** Phase transition callback for UI animation. */
  onPhase?: (phase: AcceleratorPhase, data?: AcceleratorPhaseData) => void;
}

/** Protocol used to reach the accelerator's `/health` + `/prove` endpoints. */
export type AcceleratorProtocol = "http" | "https";

/**
 * Status of the local native accelerator, returned by {@link AcceleratorProver.checkAcceleratorStatus}.
 *
 * A discriminated union on `available` (Q12). The prior flat interface let illegal field combinations
 * typecheck (e.g. `available: false` carrying `availableVersions`, or `needsDownload` on an offline
 * result). Narrow on `available` first — and on `reason` for the unavailable cases — to access only the
 * fields valid for that state.
 */
export type AcceleratorStatus =
  | {
      /** The accelerator is reachable and version-compatible. */
      available: true;
      /** Whether it must download `bb` for the SDK's Aztec version before it can prove. */
      needsDownload: boolean;
      /** Accelerator version from `/health` (`aztec_version`); absent on the multi-version protocol. */
      acceleratorVersion?: string;
      /** Aztec versions the accelerator already has cached (multi-version protocol). */
      availableVersions?: string[];
      /** The Aztec version this SDK expects (from its `@aztec/stdlib` dependency). */
      sdkAztecVersion?: string;
      /** Which protocol reached the accelerator. */
      protocol: AcceleratorProtocol;
    }
  | {
      available: false;
      /** Both the HTTP and HTTPS probes failed — the accelerator isn't running. */
      reason: "offline";
      sdkAztecVersion?: string;
    }
  | {
      available: false;
      /** Reachable, but `/health` returned a non-OK HTTP status. */
      reason: "error";
      sdkAztecVersion?: string;
      protocol: AcceleratorProtocol;
    }
  | {
      available: false;
      /** Reachable, but its Aztec version doesn't match the SDK's (legacy single-version protocol). */
      reason: "version-mismatch";
      /** The mismatched accelerator version. */
      acceleratorVersion: string;
      sdkAztecVersion?: string;
      protocol: AcceleratorProtocol;
    };
