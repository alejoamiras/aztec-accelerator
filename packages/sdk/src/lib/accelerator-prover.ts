import { BBLazyPrivateKernelProver } from "@aztec/bb-prover/client/lazy";
import type { CircuitSimulator } from "@aztec/simulator/client";
import { type PrivateExecutionStep, serializePrivateExecutionSteps } from "@aztec/stdlib/kernel";
import { ChonkProofWithPublicInputs } from "@aztec/stdlib/proofs";
import ky, { HTTPError } from "ky";
import ms from "ms";
import sdkPkg from "../../package.json" with { type: "json" };
import { logger } from "./logger.js";

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

/** Status of the local native accelerator, returned by {@link AcceleratorProver.checkAcceleratorStatus}. */
export interface AcceleratorStatus {
  /** Whether the accelerator is reachable and compatible. */
  available: boolean;
  /** Whether the accelerator needs to download `bb` for the SDK's Aztec version. */
  needsDownload: boolean;
  /** Accelerator version string from the `/health` endpoint (legacy `aztec_version` field). */
  acceleratorVersion?: string;
  /** Aztec versions the accelerator already has cached. */
  availableVersions?: string[];
  /** The Aztec version this SDK expects (from its `@aztec/stdlib` dependency). */
  sdkAztecVersion?: string;
  /** Which protocol was used to reach the accelerator (`"http"` or `"https"`). */
  protocol?: "http" | "https";
}

/**
 * Create a lazy-loading proxy for CircuitSimulator that dynamically imports
 * `@aztec/simulator/client` on first method call. This avoids adding
 * `@aztec/simulator` as a runtime dependency of the SDK.
 */
function createLazySimulator(): CircuitSimulator {
  let instance: CircuitSimulator | null = null;
  let loading: Promise<CircuitSimulator> | null = null;

  async function getInstance(): Promise<CircuitSimulator> {
    if (instance) return instance;
    if (!loading) {
      loading = import("@aztec/simulator/client")
        .then((mod) => {
          instance = new mod.WASMSimulator();
          return instance;
        })
        .catch(() => {
          loading = null;
          throw new Error(
            "No simulator provided and @aztec/simulator/client could not be loaded. " +
              "Install @aztec/simulator or pass a simulator in the constructor options.",
          );
        });
    }
    return loading;
  }

  // Return a proxy that forwards all property access to the lazy-loaded instance.
  return new Proxy({} as CircuitSimulator, {
    get(_target, prop) {
      // Return an async function that loads the simulator then delegates.
      return async (...args: unknown[]) => {
        const sim = await getInstance();
        return (sim as any)[prop](...args);
      };
    },
  });
}

const DEFAULT_ACCELERATOR_PORT = 59833;
const DEFAULT_ACCELERATOR_HTTPS_PORT = 59834;
const DEFAULT_ACCELERATOR_HOST = "127.0.0.1";

/**
 * Aztec private kernel prover that routes proving to a local native accelerator
 * running `bb` on the user's machine via `http://127.0.0.1:59833`.
 *
 * Falls back to WASM proving if the accelerator is unavailable.
 *
 * @example
 * ```ts
 * // Zero-config — auto-detects accelerator on default port
 * const prover = new AcceleratorProver();
 *
 * // Custom port
 * const prover = new AcceleratorProver({ accelerator: { port: 51337 } });
 *
 * // Phase callback for UI animation
 * const prover = new AcceleratorProver({ onPhase: (p) => console.log(p) });
 * ```
 */
export class AcceleratorProver extends BBLazyPrivateKernelProver {
  #onPhase: ((phase: AcceleratorPhase, data?: AcceleratorPhaseData) => void) | null = null;
  #acceleratorPort: number;
  #acceleratorHttpsPort: number;
  #acceleratorHost: string;
  #acceleratorProtocol: "http" | "https" | null = null;
  #forceLocal = false;

  constructor(options?: AcceleratorProverOptions) {
    const opts = options ?? {};
    super(opts.simulator ?? createLazySimulator());

    if (opts.onPhase) this.#onPhase = opts.onPhase;

    // Initialize with undefined to defer to env/defaults below
    let port: number | undefined;
    let httpsPort: number | undefined;
    let host: string | undefined;

    if (opts.accelerator) {
      if (opts.accelerator.port !== undefined) port = opts.accelerator.port;
      if (opts.accelerator.httpsPort !== undefined) httpsPort = opts.accelerator.httpsPort;
      if (opts.accelerator.host !== undefined) host = opts.accelerator.host;
    }

    const envPort =
      typeof process !== "undefined" ? process.env?.AZTEC_ACCELERATOR_PORT : undefined;
    const envHttpsPort =
      typeof process !== "undefined" ? process.env?.AZTEC_ACCELERATOR_HTTPS_PORT : undefined;

    this.#acceleratorPort =
      port ?? (envPort ? Number.parseInt(envPort, 10) : DEFAULT_ACCELERATOR_PORT);
    this.#acceleratorHttpsPort =
      httpsPort ??
      (envHttpsPort ? Number.parseInt(envHttpsPort, 10) : DEFAULT_ACCELERATOR_HTTPS_PORT);
    this.#acceleratorHost = host ?? DEFAULT_ACCELERATOR_HOST;
  }

  /** Configure the local accelerator connection (port, host). Resets cached protocol. */
  setAcceleratorConfig(config: AcceleratorConfig) {
    if (config.port !== undefined) this.#acceleratorPort = config.port;
    if (config.httpsPort !== undefined) this.#acceleratorHttpsPort = config.httpsPort;
    if (config.host !== undefined) this.#acceleratorHost = config.host;
    this.#acceleratorProtocol = null;
  }

  /** Register a callback for proof generation sub-phase transitions (for UI animation). */
  setOnPhase(callback: ((phase: AcceleratorPhase, data?: AcceleratorPhaseData) => void) | null) {
    this.#onPhase = callback;
  }

  /** Force WASM proving, bypassing accelerator detection. */
  setForceLocal(force: boolean) {
    this.#forceLocal = force;
  }

  get #acceleratorBaseUrl(): string {
    if (this.#acceleratorProtocol === "https") {
      return `https://${this.#acceleratorHost}:${this.#acceleratorHttpsPort}`;
    }
    return `http://${this.#acceleratorHost}:${this.#acceleratorPort}`;
  }

  /**
   * Probe the local accelerator's `/health` endpoint and return its status.
   * Use it to show "Accelerator connected" / "Offline" in your UI before a prove call.
   */
  async checkAcceleratorStatus(): Promise<AcceleratorStatus> {
    const sdkAztecVersion = this.#getAztecVersion();
    const httpUrl = `http://${this.#acceleratorHost}:${this.#acceleratorPort}/health`;
    const httpsUrl = `https://${this.#acceleratorHost}:${this.#acceleratorHttpsPort}/health`;

    try {
      // Probe both HTTP and HTTPS in parallel — whichever responds first wins.
      // Chrome/Firefox: HTTP responds (~1ms), HTTPS rejection silently ignored.
      // Safari with HTTPS enabled: HTTP blocked (mixed content), HTTPS responds.
      // Both offline: AggregateError → { available: false }.
      const { res: response, protocol } = await Promise.any([
        fetch(httpUrl, { signal: AbortSignal.timeout(2000) }).then((res) => ({
          res,
          protocol: "http" as const,
        })),
        fetch(httpsUrl, { signal: AbortSignal.timeout(2000) }).then((res) => ({
          res,
          protocol: "https" as const,
        })),
      ]);

      this.#acceleratorProtocol = protocol;

      if (!response.ok)
        return { available: false, needsDownload: false, sdkAztecVersion, protocol };

      const data = (await response.json()) as {
        aztec_version?: string;
        available_versions?: string[];
      };

      const acceleratorVersion = data.aztec_version;
      const availableVersions = data.available_versions;

      // New multi-version protocol: check available_versions array
      if (availableVersions) {
        const needsDownload = sdkAztecVersion
          ? !availableVersions.includes(sdkAztecVersion)
          : false;
        logger.info("Multi-version health check", {
          sdkAztecVersion,
          availableVersions,
          needsDownload,
          protocol,
        });
        return {
          available: true,
          needsDownload,
          acceleratorVersion,
          availableVersions,
          sdkAztecVersion,
          protocol,
        };
      }

      // Legacy protocol: exact version match
      if (acceleratorVersion && acceleratorVersion !== "unknown") {
        if (sdkAztecVersion && acceleratorVersion !== sdkAztecVersion) {
          logger.warn("Accelerator Aztec version mismatch", {
            accelerator: acceleratorVersion,
            sdk: sdkAztecVersion,
          });
          return {
            available: false,
            needsDownload: false,
            acceleratorVersion,
            sdkAztecVersion,
            protocol,
          };
        }
      }
      return {
        available: true,
        needsDownload: false,
        acceleratorVersion,
        sdkAztecVersion,
        protocol,
      };
    } catch {
      this.#acceleratorProtocol = null;
      return { available: false, needsDownload: false, sdkAztecVersion };
    }
  }

  async createChonkProof(
    executionSteps: PrivateExecutionStep[],
  ): Promise<ChonkProofWithPublicInputs> {
    if (this.#forceLocal) {
      logger.info("Force-local mode, using WASM prover");
      this.#onPhase?.("proving");
      const start = performance.now();
      const proof = await super.createChonkProof(executionSteps);
      const durationMs = Math.round(performance.now() - start);
      logger.info("Local proof completed", { durationMs });
      this.#onPhase?.("proved", { durationMs });
      return proof;
    }

    logger.info("Using accelerated prover");

    this.#onPhase?.("detect");
    const { available, needsDownload } = await this.checkAcceleratorStatus();

    if (!available) {
      logger.info("Accelerator not available, falling back to WASM");
      this.#onPhase?.("fallback");
      this.#onPhase?.("proving");
      const start = performance.now();
      const proof = await super.createChonkProof(executionSteps);
      const durationMs = Math.round(performance.now() - start);
      logger.info("Local proof completed", { durationMs });
      this.#onPhase?.("proved", { durationMs });
      this.#onPhase?.("receive");
      return proof;
    }

    if (needsDownload) {
      logger.info("Accelerator needs to download bb for this version");
      this.#onPhase?.("downloading");
    }

    logger.info("Accelerator available, proving natively", {
      url: this.#acceleratorBaseUrl,
    });

    this.#onPhase?.("serialize");
    const msgpack = serializePrivateExecutionSteps(executionSteps);

    const aztecVersion = this.#getAztecVersion();

    this.#onPhase?.("transmit");
    this.#onPhase?.("proving");

    let res: Awaited<ReturnType<typeof ky.post>>;
    try {
      res = await ky.post(`${this.#acceleratorBaseUrl}/prove`, {
        body: new Uint8Array(msgpack),
        timeout: ms("10 min"),
        retry: 0,
        headers: {
          "content-type": "application/octet-stream",
          ...(aztecVersion ? { "x-aztec-version": aztecVersion } : {}),
        },
      });
    } catch (err) {
      // 403: user denied this site, or authorization timed out — fall back to WASM
      if (err instanceof HTTPError && err.response.status === 403) {
        const body = await err.response
          .json<{ error?: string; message?: string }>()
          .catch(() => null);
        logger.warn("Accelerator denied this origin, falling back to WASM", {
          error: body?.error,
          message: body?.message,
        });
        this.#onPhase?.("denied");
        this.#onPhase?.("fallback");
        this.#onPhase?.("proving");
        const start = performance.now();
        const proof = await super.createChonkProof(executionSteps);
        const durationMs = Math.round(performance.now() - start);
        logger.info("Local proof completed after denial", { durationMs });
        this.#onPhase?.("proved", { durationMs });
        this.#onPhase?.("receive");
        return proof;
      }
      throw err;
    }

    const proveDurationMs = res.headers.get("x-prove-duration-ms");
    if (proveDurationMs) {
      logger.info("Accelerator server-side timing", { proveDurationMs: Number(proveDurationMs) });
      this.#onPhase?.("proved", { durationMs: Number(proveDurationMs) });
    }
    const response = await res.json<{ proof: string }>();

    this.#onPhase?.("receive");
    const proofBuffer = Buffer.from(response.proof, "base64");
    return ChonkProofWithPublicInputs.fromBuffer(proofBuffer);
  }

  #getAztecVersion(): string | undefined {
    return (sdkPkg.dependencies as Record<string, string | undefined>)["@aztec/stdlib"];
  }
}
