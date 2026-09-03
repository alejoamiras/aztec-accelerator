import type { CircuitSimulator } from "@aztec/simulator/client";

// q7e3-F-02: the SDK's published types live here (a neutral module), not inside the
// `accelerator-prover.ts` hotspot. `index.ts` re-exports them unchanged; `accelerator-transport.ts`
// imports them here instead of back-importing from the prover — killing the former 2-way edge.

/**
 * The `/health` + `/prove` API version this SDK speaks. Bumped in lockstep with the server's own
 * `api_version` when the wire contract changes incompatibly. Kept as a named constant (B7) rather than the
 * `1` literal that was inlined at every check — a per-language constant that documents the negotiated
 * version and gives one place to bump.
 */
export const ACCELERATOR_API_VERSION = 1;

/** Sub-phases emitted during proof generation for UI animation. */
export type AcceleratorPhase =
  | "detect"
  | "secure-connection-unavailable"
  | "serialize"
  | "transmit"
  | "proving"
  | "proved"
  | "receive"
  | "fallback"
  | "downloading"
  | "denied"
  // B7 (F14): the accelerator refused this SDK's Aztec version (`403 version_not_allowed`). Distinct from
  // `"denied"` (a user/origin denial) — the proof still degrades to WASM, but the cause is a version
  // gap the UI should surface differently.
  | "version-mismatch";

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
   * Private proof transport policy. When true, the SDK never sends a `/prove` request or private
   * witness over HTTP. After an HTTPS connection failure it may perform one bounded, witness-free
   * HTTP `GET /health` solely to diagnose whether HTTPS is disabled or untrusted; that diagnostic
   * can never make the accelerator eligible for proving.
   *
   * Defaults to true in browsers and false in Node, Bun, and SSR. An explicit constructor/runtime
   * option wins over `AZTEC_ACCELERATOR_HTTPS_ONLY`, which wins over the runtime default.
   */
  httpsOnly?: boolean;
  /**
   * Allow the SDK to fall back to the plaintext `http://` endpoint **after it has already reached a
   * healthy `https://` accelerator at this address**. Off by default (F-01, audit 2026-07-31).
   *
   * This is not the same knob as {@link AcceleratorConfig.httpsOnly}. It governs the narrower case
   * where HTTPS *was* working and then a `/prove` fails at the network layer. Turn it on only if you
   * explicitly accept retrying the same private witness over plaintext HTTP. Browser dApps that offer
   * a session-only HTTP recovery must set both `httpsOnly: false` and
   * `allowInsecureDowngrade: true`; the SDK never persists that decision.
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

/** Options for {@link AcceleratorProver.checkAcceleratorStatus}. */
export interface AcceleratorStatusCheckOptions {
  /**
   * Ignore a settled status cached within the normal ten-second TTL and start a fresh probe. An
   * already-running probe for the current endpoint is still shared.
   */
  forceRefresh?: boolean;
}

/** Protocol used to reach the accelerator's `/health` + `/prove` endpoints. */
export type AcceleratorProtocol = "http" | "https";

/** Best-effort result of the witness-free HTTP diagnostic after an HTTPS connection failure. */
export type SecureConnectionDiagnosis =
  | "https-disabled"
  | "tls-or-trust-failure"
  | "accelerator-reachable"
  | "unconfirmed";

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
      /**
       * The accelerator app's own version from `/health` (`version`) — the desktop/headless build, NOT the
       * Aztec version. Surfaced (B7) for diagnostics/telemetry; `undefined` when the origin-tiered minimal
       * `/health` withheld it (unapproved cross-origin).
       */
      appVersion?: string;
      /**
       * The accelerator's `/health` `api_version`. The SDK only treats an endpoint as available when this
       * equals {@link ACCELERATOR_API_VERSION}; it is surfaced here for diagnostics.
       */
      apiVersion?: number;
      /** Which protocol reached the accelerator. */
      protocol: AcceleratorProtocol;
    }
  | {
      available: false;
      /**
       * The endpoint did not answer in a runtime/policy that permits normal HTTP probing, and the
       * browser did not expose a conclusive permission denial. Browser HTTPS-only failures instead use
       * `secure-connection-unavailable`, normally with an `unconfirmed` diagnosis when prompts or
       * browser policy obscure both endpoints.
       */
      reason: "offline";
      sdkAztecVersion?: string;
    }
  | {
      available: false;
      /** The browser explicitly denied this origin permission to reach the loopback address space. */
      reason: "permission-blocked";
      sdkAztecVersion?: string;
    }
  | {
      available: false;
      /** HTTPS could not connect and policy forbids using HTTP for private proving. */
      reason: "secure-connection-unavailable";
      /** Best-effort classification from a single witness-free HTTP `GET /health`. */
      diagnosis: SecureConnectionDiagnosis;
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
