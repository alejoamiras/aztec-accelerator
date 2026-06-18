import { BBLazyPrivateKernelProver } from "@aztec/bb-prover/client/lazy";
import type { CircuitSimulator } from "@aztec/simulator/client";
import { type PrivateExecutionStep, serializePrivateExecutionSteps } from "@aztec/stdlib/kernel";
import { ChonkProofWithPublicInputs } from "@aztec/stdlib/proofs";
import { HTTPError } from "ky";
import sdkPkg from "../../package.json" with { type: "json" };
import { AcceleratorTransport } from "./accelerator-transport.js";
import { logger } from "./logger.js";
// q7e3-F-02: published types now live in ./types.ts (a neutral module); index.ts re-exports them.
import type {
  AcceleratorConfig,
  AcceleratorPhase,
  AcceleratorPhaseData,
  AcceleratorProtocol,
  AcceleratorProverOptions,
  AcceleratorStatus,
} from "./types.js";

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
  /** Owns endpoint URLs, protocol negotiation, the status cache, and `/health` + `/prove` I/O. */
  #transport: AcceleratorTransport;
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
    const resolvedPort = port ?? (Number.isNaN(parsedPort) ? DEFAULT_ACCELERATOR_PORT : parsedPort);
    const resolvedHttpsPort =
      httpsPort ??
      (Number.isNaN(parsedHttpsPort) ? DEFAULT_ACCELERATOR_HTTPS_PORT : parsedHttpsPort);
    const resolvedHost = host ?? DEFAULT_ACCELERATOR_HOST;
    this.#transport = new AcceleratorTransport(resolvedHost, resolvedPort, resolvedHttpsPort);
  }

  /** Configure the local accelerator connection (port, host). Resets cached protocol + status. */
  setAcceleratorConfig(config: AcceleratorConfig) {
    // The transport resets BOTH the cached protocol and the status cache (each is keyed to
    // the old endpoint, so a stale hit would report the wrong host/port for up to the TTL).
    this.#transport.configure(config);
  }

  /** Register a callback for proof generation sub-phase transitions (for UI animation). */
  setOnPhase(callback: ((phase: AcceleratorPhase, data?: AcceleratorPhaseData) => void) | null) {
    this.#onPhase = callback;
  }

  /** Force WASM proving, bypassing accelerator detection. */
  setForceLocal(force: boolean) {
    this.#forceLocal = force;
  }

  /**
   * Probe the local accelerator's `/health` endpoint and return its status.
   * Use it to show "Accelerator connected" / "Offline" in your UI before a prove call.
   */
  async checkAcceleratorStatus(): Promise<AcceleratorStatus> {
    // Return cached result if still fresh — avoids re-probing on every proof call
    // and eliminates the 1s retry delay when the accelerator is offline.
    const cached = this.#transport.getFreshCachedStatus();
    if (cached) return cached;
    return this.#probeAndParseHealth();
  }

  /**
   * Probe the accelerator's `/health` (dual HTTP/HTTPS, one retry) and parse the response into an
   * {@link AcceleratorStatus}, caching the result. Extracted from {@link AcceleratorProver.checkAcceleratorStatus}
   * (Q5) — behavior-identical; the status-cache fast-path stays in the caller.
   */
  async #probeAndParseHealth(): Promise<AcceleratorStatus> {
    const sdkAztecVersion = this.#getAztecVersion();

    try {
      // Probe both HTTP and HTTPS in parallel (one retry after 1s) — whichever responds
      // first wins. Chrome/Firefox: HTTP responds (~1ms), HTTPS rejection silently ignored.
      // Safari with HTTPS enabled: HTTP blocked (mixed content), HTTPS responds.
      // Both offline twice: probeHealth throws → caught below → { available: false }.
      const { response, protocol } = await this.#transport.probeHealth();

      if (!response.ok) {
        // q7e3-F-06: non-OK → KEEP any existing pin. A fast error (e.g. an HTTPS cert failure)
        // must not pin the wrong protocol for /prove, nor clear an already-good pin.
        return this.#transport.commitStatus(
          { available: false, reason: "error", sdkAztecVersion, protocol },
          { pin: "keep" },
        );
      }

      let data: { aztec_version?: string; available_versions?: string[] };
      try {
        data = (await response.json()) as {
          aztec_version?: string;
          available_versions?: string[];
        };
      } catch {
        // Reachable but unparseable JSON — "error" (the host answered), NOT "offline" (both probes
        // failed). q7e3-F-06: CLEAR the pin — a misbehaving responder shouldn't drive /prove.
        return this.#transport.commitStatus(
          { available: false, reason: "error", sdkAztecVersion, protocol },
          { pin: "clear" },
        );
      }

      // q7e3-F-05: the version-policy decision is a pure function — a reachable, parsed /health
      // always pins the winning protocol (`set`); only the available/needsDownload/mismatch shape varies.
      return this.#transport.commitStatus(this.#classifyHealth(data, protocol, sdkAztecVersion), {
        pin: "set",
        protocol,
      });
    } catch {
      // q7e3-F-06: both probes failed → offline; CLEAR the pin.
      return this.#transport.commitStatus(
        { available: false, reason: "offline", sdkAztecVersion },
        { pin: "clear" },
      );
    }
  }

  /**
   * q7e3-F-05: pure version-policy. Classify a parsed `/health` body into the available /
   * needs-download / version-mismatch status. No I/O, no caching, no protocol pinning (the caller owns
   * those) — so the policy is isolated and unit-testable. Behavior-identical to the prior inline branches.
   */
  #classifyHealth(
    data: { aztec_version?: string; available_versions?: string[] },
    protocol: AcceleratorProtocol,
    sdkAztecVersion: string | undefined,
  ): AcceleratorStatus {
    const acceleratorVersion = data.aztec_version;
    const availableVersions = data.available_versions;

    // New multi-version protocol: the SDK's version just needs to be in the cached set.
    if (availableVersions) {
      const needsDownload = sdkAztecVersion ? !availableVersions.includes(sdkAztecVersion) : false;
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

    // Legacy single-version protocol: exact match required (a known accelerator version that differs).
    if (
      acceleratorVersion &&
      acceleratorVersion !== "unknown" &&
      sdkAztecVersion &&
      acceleratorVersion !== sdkAztecVersion
    ) {
      logger.warn("Accelerator Aztec version mismatch", {
        accelerator: acceleratorVersion,
        sdk: sdkAztecVersion,
      });
      return {
        available: false,
        reason: "version-mismatch",
        acceleratorVersion,
        sdkAztecVersion,
        protocol,
      };
    }

    return { available: true, needsDownload: false, acceleratorVersion, sdkAztecVersion, protocol };
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
      return this.#fallbackToWasm(executionSteps, "Local proof completed");
    }

    if (status.needsDownload) {
      logger.info("Accelerator needs to download bb for this version");
      this.#onPhase?.("downloading");
    }

    return this.#proveRemote(executionSteps);
  }

  /**
   * q7e3-F-11: the accelerated proving path — serialize, POST `/prove`, decode. A `403` (origin denied
   * or auth timeout) emits `"denied"` and falls back to WASM; other errors propagate. Extracted from
   * {@link AcceleratorProver.createChonkProof}; only reached when the accelerator is available.
   */
  async #proveRemote(executionSteps: PrivateExecutionStep[]): Promise<ChonkProofWithPublicInputs> {
    logger.info("Accelerator available, proving natively", {
      url: this.#transport.baseUrl,
    });

    this.#onPhase?.("serialize");
    const msgpack = serializePrivateExecutionSteps(executionSteps);

    const aztecVersion = this.#getAztecVersion();

    this.#onPhase?.("transmit");
    this.#onPhase?.("proving");

    const start = performance.now();
    let res: Response;
    try {
      res = await this.#transport.postProve(new Uint8Array(msgpack), aztecVersion);
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
        return this.#fallbackToWasm(executionSteps, "Local proof completed after denial");
      }
      throw err;
    }

    return this.#decodeProof(res, start);
  }

  /**
   * q7e3-F-11: emit `"proved"` (the server's authoritative `x-prove-duration-ms` if present, else the
   * client-measured round-trip — so the UI never hangs on `"proving"`), then `"receive"` + decode the
   * base64 proof buffer.
   */
  async #decodeProof(res: Response, start: number): Promise<ChonkProofWithPublicInputs> {
    const serverMs = Number(res.headers.get("x-prove-duration-ms"));
    const durationMs =
      Number.isFinite(serverMs) && serverMs > 0 ? serverMs : Math.round(performance.now() - start);
    logger.info("Accelerator proof completed", { durationMs });
    this.#onPhase?.("proved", { durationMs });

    const response = (await res.json()) as { proof: string };
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

  /**
   * WASM fallback wrapper: emit "fallback" → run the local prover → emit "receive". Shared by the
   * accelerator-unavailable and 403-denied paths (the "denied" phase stays at its call site).
   */
  async #fallbackToWasm(
    executionSteps: PrivateExecutionStep[],
    logLabel: string,
  ): Promise<ChonkProofWithPublicInputs> {
    this.#onPhase?.("fallback");
    const proof = await this.#proveLocally(executionSteps, logLabel);
    this.#onPhase?.("receive");
    return proof;
  }

  #getAztecVersion(): string | undefined {
    // Strip leading semver range prefixes (^, ~, >=) in case the dependency isn't pinned.
    // We only strip the LEADING non-digits: the server's is_valid_version accepts the inner
    // `.`/`-`/`_` of a version like `5.0.0-rc.1` (see core version_policy.rs `is_valid_version`),
    // so the prerelease suffix must be preserved for the /health version handshake.
    return (sdkPkg.dependencies as Record<string, string | undefined>)["@aztec/stdlib"]?.replace(
      /^[^0-9]*/,
      "",
    );
  }
}
