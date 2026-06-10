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
  /** Port the accelerator listens on (HTTPS, for Safari). Default: 59834. */
  httpsPort?: number;
  /** Host the accelerator binds to. Default: "127.0.0.1". */
  host?: string;
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
