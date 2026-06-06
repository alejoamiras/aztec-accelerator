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
      // Do NOT make the proxy thenable or hijack symbol-keyed protocols: if `then`
      // (or any symbol like Symbol.iterator/toPrimitive) resolved to a forwarding
      // function, `await proxy` or a promise-probe would treat the proxy as a broken
      // thenable and could hang. Methods are string-keyed, so this is safe.
      if (prop === "then" || typeof prop === "symbol") return undefined;
      // Otherwise return an async function that loads the simulator then delegates.
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
  #statusCache: { result: AcceleratorStatus; timestamp: number } | null = null;
  static readonly #STATUS_CACHE_TTL = 10_000; // 10 seconds
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

    const parsedPort = envPort ? Number.parseInt(envPort, 10) : NaN;
    const parsedHttpsPort = envHttpsPort ? Number.parseInt(envHttpsPort, 10) : NaN;
    this.#acceleratorPort =
      port ?? (Number.isNaN(parsedPort) ? DEFAULT_ACCELERATOR_PORT : parsedPort);
    this.#acceleratorHttpsPort =
      httpsPort ??
      (Number.isNaN(parsedHttpsPort) ? DEFAULT_ACCELERATOR_HTTPS_PORT : parsedHttpsPort);
    this.#acceleratorHost = host ?? DEFAULT_ACCELERATOR_HOST;
  }

  /** Configure the local accelerator connection (port, host). Resets cached protocol. */
  setAcceleratorConfig(config: AcceleratorConfig) {
    if (config.port !== undefined) this.#acceleratorPort = config.port;
    if (config.httpsPort !== undefined) this.#acceleratorHttpsPort = config.httpsPort;
    if (config.host !== undefined) this.#acceleratorHost = config.host;
    // Reset BOTH the cached protocol and the status cache — both are keyed to the old
    // endpoint, so a stale cache hit would report the wrong host/port for up to the TTL.
    this.#acceleratorProtocol = null;
    this.#statusCache = null;
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
    // Return cached result if still fresh — avoids re-probing on every proof call
    // and eliminates the 1s retry delay when the accelerator is offline.
    if (
      this.#statusCache &&
      Date.now() - this.#statusCache.timestamp < AcceleratorProver.#STATUS_CACHE_TTL
    ) {
      return this.#statusCache.result;
    }
    return this.#probeAndParseHealth();
  }

  /**
   * Probe the accelerator's `/health` (dual HTTP/HTTPS, one retry) and parse the response into an
   * {@link AcceleratorStatus}, caching the result. Extracted from {@link AcceleratorProver.checkAcceleratorStatus}
   * (Q5) — behavior-identical; the status-cache fast-path stays in the caller.
   */
  async #probeAndParseHealth(): Promise<AcceleratorStatus> {
    const sdkAztecVersion = this.#getAztecVersion();
    const httpUrl = `http://${this.#acceleratorHost}:${this.#acceleratorPort}/health`;
    const httpsUrl = `https://${this.#acceleratorHost}:${this.#acceleratorHttpsPort}/health`;

    // Probe with a single retry — the accelerator may be slow to start on first
    // launch or after an update. Without retry, the SDK falls back to WASM unnecessarily.
    const probe = () =>
      Promise.any([
        fetch(httpUrl, { signal: AbortSignal.timeout(2000) }).then((res) => ({
          res,
          protocol: "http" as const,
        })),
        fetch(httpsUrl, { signal: AbortSignal.timeout(2000) }).then((res) => ({
          res,
          protocol: "https" as const,
        })),
      ]);

    const cacheAndReturn = (status: AcceleratorStatus): AcceleratorStatus => {
      this.#statusCache = { result: status, timestamp: Date.now() };
      return status;
    };

    try {
      // Probe both HTTP and HTTPS in parallel — whichever responds first wins.
      // Chrome/Firefox: HTTP responds (~1ms), HTTPS rejection silently ignored.
      // Safari with HTTPS enabled: HTTP blocked (mixed content), HTTPS responds.
      // Both offline: AggregateError → retry once after 1s, then { available: false }.
      let result: { res: Response; protocol: "http" | "https" };
      try {
        result = await probe();
      } catch {
        // First probe failed — retry once after 1s
        await new Promise((r) => setTimeout(r, 1000));
        result = await probe();
      }
      const { res: response, protocol } = result;

      if (!response.ok) {
        // Don't cache protocol on error — a fast error (e.g. HTTPS cert failure)
        // would permanently set the wrong protocol for subsequent /prove calls.
        return cacheAndReturn({
          available: false,
          reason: "error",
          sdkAztecVersion,
          protocol,
        });
      }

      this.#acceleratorProtocol = protocol;

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
        return cacheAndReturn({
          available: true,
          needsDownload,
          acceleratorVersion,
          availableVersions,
          sdkAztecVersion,
          protocol,
        });
      }

      // Legacy protocol: exact version match
      if (acceleratorVersion && acceleratorVersion !== "unknown") {
        if (sdkAztecVersion && acceleratorVersion !== sdkAztecVersion) {
          logger.warn("Accelerator Aztec version mismatch", {
            accelerator: acceleratorVersion,
            sdk: sdkAztecVersion,
          });
          return cacheAndReturn({
            available: false,
            reason: "version-mismatch",
            acceleratorVersion,
            sdkAztecVersion,
            protocol,
          });
        }
      }
      return cacheAndReturn({
        available: true,
        needsDownload: false,
        acceleratorVersion,
        sdkAztecVersion,
        protocol,
      });
    } catch {
      this.#acceleratorProtocol = null;
      return cacheAndReturn({ available: false, reason: "offline", sdkAztecVersion });
    }
  }

  async createChonkProof(
    executionSteps: PrivateExecutionStep[],
  ): Promise<ChonkProofWithPublicInputs> {
    if (this.#forceLocal) {
      logger.info("Force-local mode, using WASM prover");
      return this.#proveLocally(executionSteps, "Local proof completed");
    }

    logger.info("Using accelerated prover");

    this.#onPhase?.("detect");
    const status = await this.checkAcceleratorStatus();

    if (!status.available) {
      logger.info("Accelerator not available, falling back to WASM");
      this.#onPhase?.("fallback");
      const proof = await this.#proveLocally(executionSteps, "Local proof completed");
      this.#onPhase?.("receive");
      return proof;
    }

    if (status.needsDownload) {
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
    const start = performance.now();
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
        // ky 2.x pre-parses the error body into err.data (response body is already consumed).
        const body =
          err.data && typeof err.data === "object"
            ? (err.data as { error?: string; message?: string })
            : undefined;
        logger.warn("Accelerator denied this origin, falling back to WASM", {
          error: body?.error,
          message: body?.message,
        });
        this.#onPhase?.("denied");
        this.#onPhase?.("fallback");
        const proof = await this.#proveLocally(
          executionSteps,
          "Local proof completed after denial",
        );
        this.#onPhase?.("receive");
        return proof;
      }
      throw err;
    }

    // Always emit "proved" so the UI never hangs on "proving": prefer the server's
    // authoritative duration (x-prove-duration-ms), else the client-measured round-trip.
    const serverMs = Number(res.headers.get("x-prove-duration-ms"));
    const durationMs =
      Number.isFinite(serverMs) && serverMs > 0 ? serverMs : Math.round(performance.now() - start);
    logger.info("Accelerator proof completed", { durationMs });
    this.#onPhase?.("proved", { durationMs });

    const response = await res.json<{ proof: string }>();
    this.#onPhase?.("receive");
    const proofBuffer = Buffer.from(response.proof, "base64");
    return ChonkProofWithPublicInputs.fromBuffer(proofBuffer);
  }

  /**
   * Run the WASM (super) prover with phase + timing instrumentation. Emits "proving"
   * then "proved"; callers add any surrounding phases (e.g. "fallback" / "receive").
   */
  async #proveLocally(
    executionSteps: PrivateExecutionStep[],
    logLabel: string,
  ): Promise<ChonkProofWithPublicInputs> {
    this.#onPhase?.("proving");
    const start = performance.now();
    const proof = await super.createChonkProof(executionSteps);
    const durationMs = Math.round(performance.now() - start);
    logger.info(logLabel, { durationMs });
    this.#onPhase?.("proved", { durationMs });
    return proof;
  }

  #getAztecVersion(): string | undefined {
    // Strip semver range prefixes (^, ~, >=) in case the dependency isn't pinned.
    // The server's is_valid_version rejects non-alphanumeric characters.
    return (sdkPkg.dependencies as Record<string, string | undefined>)["@aztec/stdlib"]?.replace(
      /^[^0-9]*/,
      "",
    );
  }
}
