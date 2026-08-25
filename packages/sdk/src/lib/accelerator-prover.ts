import { BBLazyPrivateKernelProver } from "@aztec/bb-prover/client/lazy";
import type { CircuitSimulator } from "@aztec/simulator/client";
import { type PrivateExecutionStep, serializePrivateExecutionSteps } from "@aztec/stdlib/kernel";
import { ChonkProofWithPublicInputs } from "@aztec/stdlib/proofs";
import sdkPkg from "../../package.json" with { type: "json" };
import {
  AcceleratorTransport,
  isRecognizedHealthBody,
  TransportHttpError,
} from "./accelerator-transport.js";
import { AcceleratorHttpError, parseServerError } from "./errors.js";
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
    let httpsOnly: boolean | undefined;
    let allowInsecureDowngrade: boolean | undefined;

    if (opts.accelerator) {
      if (opts.accelerator.port !== undefined) port = opts.accelerator.port;
      if (opts.accelerator.httpsPort !== undefined) httpsPort = opts.accelerator.httpsPort;
      if (opts.accelerator.host !== undefined) host = opts.accelerator.host;
      if (opts.accelerator.httpsOnly !== undefined) httpsOnly = opts.accelerator.httpsOnly;
      if (opts.accelerator.allowInsecureDowngrade !== undefined)
        allowInsecureDowngrade = opts.accelerator.allowInsecureDowngrade;
    }

    const envPort =
      typeof process !== "undefined" ? process.env?.AZTEC_ACCELERATOR_PORT : undefined;
    const envHttpsPort =
      typeof process !== "undefined" ? process.env?.AZTEC_ACCELERATOR_HTTPS_PORT : undefined;
    const envHttpsOnly =
      typeof process !== "undefined" ? process.env?.AZTEC_ACCELERATOR_HTTPS_ONLY : undefined;
    const envAllowDowngrade =
      typeof process !== "undefined"
        ? process.env?.AZTEC_ACCELERATOR_ALLOW_INSECURE_DOWNGRADE
        : undefined;

    const parsedPort = envPort ? Number.parseInt(envPort, 10) : NaN;
    const parsedHttpsPort = envHttpsPort ? Number.parseInt(envHttpsPort, 10) : NaN;
    const resolvedPort = port ?? (Number.isNaN(parsedPort) ? DEFAULT_ACCELERATOR_PORT : parsedPort);
    const resolvedHttpsPort =
      httpsPort ??
      (Number.isNaN(parsedHttpsPort) ? DEFAULT_ACCELERATOR_HTTPS_PORT : parsedHttpsPort);
    const resolvedHost = host ?? DEFAULT_ACCELERATOR_HOST;
    // Explicit option wins; else the env flag (`1`/`true`) enables strict HTTPS-only; else false.
    const resolvedHttpsOnly =
      httpsOnly ?? (envHttpsOnly === "1" || envHttpsOnly?.toLowerCase() === "true");
    // F-01: off unless explicitly asked for, by option or env, exactly like `httpsOnly`.
    const resolvedAllowDowngrade =
      allowInsecureDowngrade ??
      (envAllowDowngrade === "1" || envAllowDowngrade?.toLowerCase() === "true");
    this.#transport = new AcceleratorTransport(
      resolvedHost,
      resolvedPort,
      resolvedHttpsPort,
      resolvedHttpsOnly,
      resolvedAllowDowngrade,
    );
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
   *
   * Single-flight: concurrent callers share one in-flight probe (per configuration generation), so
   * overlapping health checks can't race each other's pin commits.
   */
  async checkAcceleratorStatus(): Promise<AcceleratorStatus> {
    // Return cached result if still fresh — avoids re-probing on every proof call
    // and eliminates the 1s retry delay when the accelerator is offline.
    const cached = this.#transport.getFreshCachedStatus();
    if (cached) return cached;
    // Reuse an in-flight probe ONLY if it targets the current endpoint configuration.
    const gen = this.#transport.generation;
    if (this.#inflightProbe && this.#inflightProbe.gen === gen) {
      return this.#inflightProbe.promise;
    }
    const promise = this.#probeAndParseHealth(gen).finally(() => {
      if (this.#inflightProbe?.promise === promise) this.#inflightProbe = null;
    });
    this.#inflightProbe = { gen, promise };
    return promise;
  }

  /** The in-flight `/health` probe, keyed to the transport generation it was started against. */
  #inflightProbe: { gen: number; promise: Promise<AcceleratorStatus> } | null = null;

  /**
   * Probe the accelerator's `/health` (dual HTTP/HTTPS, one retry) and parse the result into an
   * {@link AcceleratorStatus}, caching the result. `gen` is the configuration generation this probe
   * was started against — every commit passes it, so a probe that raced a `setAcceleratorConfig`
   * cannot pin/cache against the NEW endpoint (post-impl codex High).
   */
  async #probeAndParseHealth(gen: number): Promise<AcceleratorStatus> {
    const sdkAztecVersion = this.#getAztecVersion();

    try {
      // Probe HTTP + HTTPS (one retry after 1s), preferring HTTPS when it's healthy (ok + the
      // recognized health contract). Chrome/Firefox with HTTPS trusted: HTTPS wins → encrypted
      // channel. HTTPS absent/untrusted: it rejects fast → HTTP wins with no added latency. Safari:
      // HTTP blocked (mixed content) → HTTPS is the only responder. Both offline twice: throws → offline.
      // The transport already read the body ONCE, bounded (deadline + byte cap) — use `body`, never
      // `response.json()`.
      const { response, protocol, body } = await this.#transport.probeHealth();

      if (!response.ok) {
        // q7e3-F-06: non-OK → KEEP any existing pin. A fast error (e.g. an HTTPS cert failure)
        // must not pin the wrong protocol for /prove, nor clear an already-good pin.
        return this.#transport.commitStatus(
          { available: false, reason: "error", sdkAztecVersion, protocol },
          { pin: "keep" },
          gen,
        );
      }

      // Reachable but not the accelerator's health contract (unparseable, stalled-body, or a
      // foreign/malformed JSON shape — enforced in BOTH normal and httpsOnly modes): "error", NOT
      // "offline". q7e3-F-06: CLEAR the pin — a misbehaving responder must not drive /prove.
      if (!isRecognizedHealthBody(body)) {
        return this.#transport.commitStatus(
          { available: false, reason: "error", sdkAztecVersion, protocol },
          { pin: "clear" },
          gen,
        );
      }
      const data = body as {
        aztec_version?: string;
        available_versions?: string[];
        version?: unknown;
        api_version?: unknown;
      };

      // q7e3-F-05: the version-policy decision is a pure function — a reachable, recognized /health
      // always pins the winning protocol (`set`); only the available/needsDownload/mismatch shape varies.
      return this.#transport.commitStatus(
        this.#classifyHealth(data, protocol, sdkAztecVersion),
        { pin: "set", protocol },
        gen,
      );
    } catch {
      // q7e3-F-06: both probes failed → offline; CLEAR the pin.
      return this.#transport.commitStatus(
        { available: false, reason: "offline", sdkAztecVersion },
        { pin: "clear" },
        gen,
      );
    }
  }

  /**
   * q7e3-F-05: pure version-policy. Classify a parsed `/health` body into the available /
   * needs-download / version-mismatch status. No I/O, no caching, no protocol pinning (the caller owns
   * those) — so the policy is isolated and unit-testable. Behavior-identical to the prior inline branches.
   */
  #classifyHealth(
    data: {
      aztec_version?: string;
      available_versions?: string[];
      version?: unknown;
      api_version?: unknown;
    },
    protocol: AcceleratorProtocol,
    sdkAztecVersion: string | undefined,
  ): AcceleratorStatus {
    const acceleratorVersion = data.aztec_version;
    const availableVersions = data.available_versions;
    // B7: surface the accelerator APP version + the negotiated api_version (were parsed then discarded).
    // The body is untrusted wire data, so NARROW at runtime (codex #6: `{version: 42}` must not leak a
    // number through the `appVersion?: string` contract).
    const appVersion = typeof data.version === "string" ? data.version : undefined;
    const apiVersion = typeof data.api_version === "number" ? data.api_version : undefined;

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
        appVersion,
        apiVersion,
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

    return {
      available: true,
      needsDownload: false,
      acceleratorVersion,
      sdkAztecVersion,
      appVersion,
      apiVersion,
      protocol,
    };
  }

  async createChonkProof(
    executionSteps: PrivateExecutionStep[],
  ): Promise<ChonkProofWithPublicInputs> {
    if (this.#forceLocal) {
      logger.info("Force-local mode, using WASM prover");
      return this.#proveLocally(executionSteps, "Local proof completed");
    }

    logger.info("Using accelerated prover");

    // Capture the endpoint generation immediately BEFORE probing — but AFTER the "detect" callback, so
    // a handler that synchronously reconfigures there is honoured (the probe below then targets the
    // NEW endpoint, and rejecting that valid result would force a needless WASM fallback — codex Low).
    // A probe that started against A and completes after `setAcceleratorConfig(B)` has its pin/cache
    // commit discarded but still RETURNS `available: true`; proving on that basis would POST the
    // witness to the never-probed B (codex High), so re-check after the probe and degrade if it moved.
    this.#onPhase?.("detect");
    const detectGen = this.#transport.generation;
    const status = await this.checkAcceleratorStatus();

    if (!status.available) {
      logger.info("Accelerator not available, falling back to WASM");
      return this.#fallbackToWasm(executionSteps, "Local proof completed");
    }

    if (this.#transport.generation !== detectGen) {
      logger.info(
        "Endpoint reconfigured during detection; falling back to WASM (unprobed endpoint)",
      );
      return this.#fallbackToWasm(executionSteps, "Local proof completed after endpoint change");
    }

    if (status.needsDownload) {
      logger.info("Accelerator needs to download bb for this version");
      this.#onPhase?.("downloading");
    }

    return this.#proveRemote(executionSteps, detectGen);
  }

  /**
   * q7e3-F-11: the accelerated proving path — serialize, POST `/prove`, decode. A `403` (origin denied
   * or auth timeout) emits `"denied"` and falls back to WASM. A NETWORK-level failure (no HTTP
   * response at all — TLS/refused/timeout) while pinned to HTTPS demotes the pin and retries once
   * over HTTP, then falls back to WASM (in strict `httpsOnly` mode: straight to WASM — the witness
   * never goes plaintext). Without this, preferring HTTPS at `/health` made a later trust/listener
   * change fail the whole prove despite a healthy HTTP path (post-impl codex High). Other HTTP-level
   * errors (the accelerator answered, e.g. 500) still propagate. Extracted from
   * {@link AcceleratorProver.createChonkProof}; only reached when the accelerator is available.
   */
  async #proveRemote(
    executionSteps: PrivateExecutionStep[],
    attemptGen: number,
  ): Promise<ChonkProofWithPublicInputs> {
    // IMMUTABLE snapshot of the endpoint this attempt targets, taken BEFORE any `onPhase` callback
    // runs. `attemptGen` is the generation the probe validated. The URLs are captured here (not read
    // from the mutable `baseUrl`/host/port at POST time) because a dApp's `onPhase` handler can call
    // `setAcceleratorConfig(B)` between here and the POST — the old code would then have sent the
    // witness to the unprobed B (post-impl codex High). Every POST below uses these snapshots.
    const attemptUrl = `${this.#transport.baseUrl}/prove`;
    const attemptWasHttps = attemptUrl.startsWith("https:");
    // Only snapshot the HTTP retry target when a retry is actually permitted. An `http://` URL is
    // never even CONSTRUCTED when plaintext is off-limits — that's the documented contract, so keep
    // it literally true rather than computing a string we'd never use. Gated on the EFFECTIVE policy,
    // not the raw `httpsOnly` flag: once HTTPS has proven healthy here the downgrade is refused too,
    // and the old check built the URL anyway (codex round 2 — harmless, but it falsified the claim).
    const httpRetryUrl =
      attemptWasHttps && this.#transport.allowsHttpDowngrade
        ? this.#transport.proveUrlFor("http")
        : null;

    logger.info("Accelerator available, proving natively", { url: attemptUrl });

    this.#onPhase?.("serialize");
    const msgpack = serializePrivateExecutionSteps(executionSteps);

    const aztecVersion = this.#getAztecVersion();

    this.#onPhase?.("transmit");
    this.#onPhase?.("proving");

    // A callback above may have reconfigured the endpoint. The snapshot URLs still point at the
    // PROBED endpoint (correct), but that endpoint is no longer the configured one — the caller asked
    // us to talk to B, and A was never re-validated. Degrade to WASM rather than sending the witness
    // to either an abandoned or an unprobed endpoint.
    if (this.#transport.generation !== attemptGen) {
      logger.info("Endpoint reconfigured before transmit; falling back to WASM");
      return this.#fallbackToWasm(executionSteps, "Local proof completed after endpoint change");
    }

    const start = performance.now();
    let res: Response;
    try {
      res = await this.#transport.postProve(new Uint8Array(msgpack), aztecVersion, attemptUrl);
    } catch (err) {
      // Network-level failure: no HTTP response at all (TLS handshake/cert failure, connection
      // refused, timeout). The HTTPS listener/trust may have changed since /health pinned it.
      if (!(err instanceof TransportHttpError)) {
        // The endpoint was reconfigured while this proof was in flight — do NOT touch the new endpoint
        // (don't demote its pin, don't POST the witness to it). Degrade to WASM (codex High).
        if (this.#transport.generation !== attemptGen) {
          logger.warn("Endpoint reconfigured during a failing proof; falling back to WASM", {
            error: String(err),
          });
          return this.#fallbackToWasm(
            executionSteps,
            "Local proof completed after endpoint change",
          );
        }
        // This request went over HTTPS in non-strict mode → the HTTP endpoint may still be healthy.
        // Retry THIS request explicitly over HTTP (independent of the shared pin, so a concurrent
        // failure that already cleared the pin doesn't stop us). `demoteHttpsPin()` only hints FUTURE
        // probes to re-check; the retry itself targets the http URL for this attempt's generation.
        // `httpRetryUrl` is non-null exactly when this attempt went over HTTPS in non-strict mode —
        // guarding on it (rather than re-deriving the condition) also narrows the type.
        if (httpRetryUrl) {
          // F-01: a healthy HTTPS accelerator answered at this endpoint, so a network-layer failure
          // is not a reason to hand the same private witness to whatever is on the plaintext port.
          // The `isProtocolHealthy` check below is the `/health` SHAPE contract, not authentication —
          // any local account can bind 127.0.0.1:59833 and satisfy it. WASM is the safe outcome.
          if (!this.#transport.allowsHttpDowngrade) {
            logger.warn(
              "HTTPS /prove failed, but this accelerator was reachable over HTTPS — refusing to " +
                "retry over plaintext HTTP; falling back to WASM. Set " +
                "`accelerator.allowInsecureDowngrade` (or AZTEC_ACCELERATOR_ALLOW_INSECURE_DOWNGRADE=1) " +
                "to allow it.",
            );
            return this.#fallbackToWasm(
              executionSteps,
              "Local proof completed after transport failure",
            );
          }
          this.#transport.demoteHttpsPin();
          // VALIDATE the HTTP endpoint before sending it the witness. A healthy HTTPS probe says
          // nothing about who is listening on the HTTP port; without this check a foreign responder
          // there receives the serialized witness the instant HTTPS fails (post-impl codex Critical).
          // Re-check the generation too, since this probe is another await point.
          const httpIsOurs = await this.#transport.isProtocolHealthy("http");
          if (!httpIsOurs || this.#transport.generation !== attemptGen) {
            logger.warn(
              "HTTPS /prove failed, but the HTTP endpoint did not answer the accelerator's health " +
                "contract — refusing to downgrade; falling back to WASM",
            );
            return this.#fallbackToWasm(
              executionSteps,
              "Local proof completed after transport failure",
            );
          }
          logger.warn(
            "HTTPS /prove failed at the network layer; retrying once over validated HTTP",
            {
              error: String(err),
            },
          );
          try {
            // The SNAPSHOT http url (captured at attempt entry), not a freshly-derived one — the
            // retry must target the same endpoint this attempt probed, never a reconfigured host/port.
            res = await this.#transport.postProve(
              new Uint8Array(msgpack),
              aztecVersion,
              httpRetryUrl,
            );
            // `return await`, not a bare `return`: without the await the promise escapes this
            // try/catch, so a rejected body read (over-cap, stalled, malformed — F-11) would never
            // reach the fallback handling below and would surface to the dApp instead of degrading
            // to WASM. The sibling call on the non-retry path already awaited; this one did not.
            return await this.#decodeProof(res, start);
          } catch (retryErr) {
            // A network-level retry failure (no HTTP response) still degrades to WASM; an HTTP error the
            // accelerator returned goes through the SAME F14 classifier as the primary path — so a
            // misconfiguration surfaced only on the HTTP retry is not silently masked either.
            if (!(retryErr instanceof TransportHttpError)) {
              logger.warn("HTTP retry failed at the network layer, falling back to WASM", {
                error: String(retryErr),
              });
              return this.#fallbackToWasm(
                executionSteps,
                "Local proof completed after transport failure",
              );
            }
            return this.#fallbackOrThrowHttp(retryErr, executionSteps);
          }
        }
        // Strict httpsOnly (never retry plaintext), OR the attempt was already HTTP (nothing better to
        // try): degrade to WASM rather than failing the dApp.
        logger.warn("Local accelerator /prove failed at the network layer, falling back to WASM", {
          error: String(err),
          httpsOnly: this.#transport.httpsOnly,
        });
        return this.#fallbackToWasm(
          executionSteps,
          "Local proof completed after transport failure",
        );
      }
      // The accelerator ANSWERED with a non-2xx — F14: recognised conditions degrade to WASM, a
      // caller misconfiguration / unrecognised status throws a typed `AcceleratorHttpError` (never the
      // internal transport error, which is not part of the SDK's public surface).
      return this.#fallbackOrThrowHttp(err, executionSteps);
    }

    // Decode OUTSIDE the transport try/catch — a decode failure is not a transport failure and must
    // not be mistaken for one (re-entering that catch would re-run the HTTPS→HTTP demotion logic for
    // a request that actually got a 200). But it still has to degrade rather than escape: the retry
    // path already falls back on a bad body, and it would be incoherent for the SAME malformed
    // response to break the dApp on the primary path and be absorbed on the retry (post-impl codex
    // Medium). Nothing about a 200 with an undecodable body says WASM can't finish the proof.
    try {
      return await this.#decodeProof(res, start);
    } catch (decodeErr) {
      logger.warn("Accelerator returned a proof that could not be decoded, falling back to WASM", {
        error: String(decodeErr),
      });
      return this.#fallbackToWasm(
        executionSteps,
        "Local proof completed after a malformed response",
      );
    }
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

    // F-11 (audit 2026-07-31-9c4cb0c): read under a byte cap AND a body deadline. `res.json()` had
    // neither — the request timeout bounds time-to-headers, so a `200` followed by an endless body hung
    // this promise forever and buffered without limit. An over-cap or stalled body now throws, which
    // the caller already handles as a prove failure (and degrades to WASM).
    const proof = await this.#transport.readProveBody(res);
    if (proof === undefined) {
      throw new Error(
        "accelerator returned an unreadable /prove body (over size cap, stalled, or malformed)",
      );
    }
    this.#onPhase?.("receive");
    const proofBuffer = Buffer.from(proof, "base64");
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
   * B7 (F14): classify an HTTP error the accelerator returned from `/prove` and either degrade to WASM or
   * throw {@link AcceleratorHttpError}. The accelerator is an optimisation, so the degrade set is matched BY
   * STATUS — EVERY `403` (denial / version / cooldown) and EVERY `408`/`413`/`429`/`503`, plus
   * `500 download_failed`/`prove_failed` — all fall back to local proving regardless of `code`. Only a
   * `400` MISCONFIGURATION (`invalid_version` / `invalid_origin`), a `500` with an unrecognised code, or an
   * unexpected status throws — masking those as "slow but working" WASM would hide a real integration bug.
   *
   * The server sends its error body as `text/plain` carrying a JSON string, so the stable `code` is
   * recovered via {@link parseServerError} (a JSON content-type gives an object; both are handled).
   */
  async #fallbackOrThrowHttp(
    err: TransportHttpError,
    executionSteps: PrivateExecutionStep[],
  ): Promise<ChonkProofWithPublicInputs> {
    const status = err.response.status;
    const { code, message } = parseServerError(err.data);

    if (status === 403) {
      // The accelerator refused this SDK's Aztec version — distinct from a user/origin denial, and worth
      // its own phase so the UI can say "update the accelerator" rather than "you denied it".
      if (code === "version_not_allowed") {
        logger.warn("Accelerator refused this SDK's Aztec version, falling back to WASM", { code });
        this.#onPhase?.("version-mismatch");
        return this.#fallbackToWasm(
          executionSteps,
          "Local proof completed after an accelerator version mismatch",
        );
      }
      // A recent denial is still in cooldown (B2 F9). This is NOT a fresh prompt, so do NOT re-emit the
      // "denied" phase — just degrade quietly.
      if (code === "authorization_cooldown") {
        logger.warn("Accelerator is in an authorization cooldown, falling back to WASM", { code });
        return this.#fallbackToWasm(
          executionSteps,
          "Local proof completed during an authorization cooldown",
        );
      }
      // origin_denied / authorization_timeout / authorization_cancelled / a bare "denied" / no code.
      logger.warn("Accelerator denied this origin, falling back to WASM", { code, message });
      this.#onPhase?.("denied");
      return this.#fallbackToWasm(executionSteps, "Local proof completed after denial");
    }

    // Transient/capacity conditions the accelerator itself signalled — degrade rather than fail the dApp.
    // 503 (shutting down / version-evicting), 408 (body_read_timeout), 413 (payload_too_large),
    // 429 (too_many_requests / prove_queue_full), and 500 with a RECOGNISED code (download_failed /
    // prove_failed). A 500 with an unknown code falls through to the throw — an unrecognised server fault.
    if (
      status === 503 ||
      status === 408 ||
      status === 413 ||
      status === 429 ||
      (status === 500 && (code === "download_failed" || code === "prove_failed"))
    ) {
      logger.warn("Accelerator could not serve this proof, falling back to WASM", { status, code });
      return this.#fallbackToWasm(
        executionSteps,
        "Local proof completed while the accelerator was unavailable",
      );
    }

    // 400 invalid_version / invalid_origin (the SDK is misconfigured for this accelerator), or ANY other
    // status/code the SDK does not recognise. Surface it typed — never mask a misconfiguration as WASM.
    throw new AcceleratorHttpError(status, code, message);
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
