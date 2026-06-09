import ky from "ky";
import ms from "ms";
import type { AcceleratorProtocol, AcceleratorStatus } from "./accelerator-prover.js";

/** How long a probed {@link AcceleratorStatus} stays fresh before a re-probe. */
const STATUS_CACHE_TTL_MS = 10_000;
/** Per-attempt timeout for each /health probe (HTTP and HTTPS fired in parallel). */
const HEALTH_PROBE_TIMEOUT_MS = 2_000;
/** Delay before the single /health retry when the first parallel probe fails. */
const PROBE_RETRY_DELAY_MS = 1_000;
/** /prove is long-running (native bb proof) — generous timeout. */
const PROVE_TIMEOUT_MS = ms("10 min");

/**
 * Owns all network I/O to the local accelerator: endpoint/URL construction, the
 * dual HTTP/HTTPS `/health` probe + protocol negotiation, the short-lived status
 * cache, and the `/prove` POST. One HTTP client (`ky`) for both endpoints, so the
 * thrown-error surface is uniform.
 *
 * The {@link AcceleratorProver} keeps the *domain* logic: parsing a `/health`
 * response into the {@link AcceleratorStatus} discriminated union, and reading a
 * `403` as an origin denial. This class is internal — it is **not** exported from
 * the package barrel.
 */
export class AcceleratorTransport {
  #host: string;
  #port: number;
  #httpsPort: number;
  /** Protocol that last reached `/health`; pins which endpoint `/prove` uses. `null` = not yet negotiated. */
  #protocol: AcceleratorProtocol | null = null;
  #statusCache: { result: AcceleratorStatus; timestamp: number } | null = null;

  constructor(host: string, port: number, httpsPort: number) {
    this.#host = host;
    this.#port = port;
    this.#httpsPort = httpsPort;
  }

  /**
   * Update connection settings. Resets BOTH the negotiated protocol and the status
   * cache — each is keyed to the old endpoint, so a stale hit would report the wrong
   * host/port for up to the TTL.
   */
  configure(config: { port?: number; httpsPort?: number; host?: string }) {
    if (config.port !== undefined) this.#port = config.port;
    if (config.httpsPort !== undefined) this.#httpsPort = config.httpsPort;
    if (config.host !== undefined) this.#host = config.host;
    this.#protocol = null;
    this.#statusCache = null;
  }

  /** Pin (or clear, with `null`) the protocol that `/prove` should use. */
  setProtocol(protocol: AcceleratorProtocol | null) {
    this.#protocol = protocol;
  }

  /** Base URL for `/prove` — `https` iff the negotiated protocol is `https`, else `http`. */
  get baseUrl(): string {
    if (this.#protocol === "https") {
      return `https://${this.#host}:${this.#httpsPort}`;
    }
    return `http://${this.#host}:${this.#port}`;
  }

  /** The cached status if still within the TTL, else `null`. */
  getFreshCachedStatus(): AcceleratorStatus | null {
    if (this.#statusCache && Date.now() - this.#statusCache.timestamp < STATUS_CACHE_TTL_MS) {
      return this.#statusCache.result;
    }
    return null;
  }

  /** Store a freshly-computed status and return it (call-site convenience). */
  cacheStatus(status: AcceleratorStatus): AcceleratorStatus {
    this.#statusCache = { result: status, timestamp: Date.now() };
    return status;
  }

  /**
   * Probe `/health` over HTTP and HTTPS in parallel; whichever responds first wins.
   * One retry after {@link PROBE_RETRY_DELAY_MS} if both fail the first time.
   *
   * Resolves with the winning {@link Response} + which protocol reached it; rejects
   * only if BOTH probes fail twice (the caller maps that to `reason: "offline"`).
   * `throwHttpErrors: false` so a non-2xx still *resolves* (caller maps it to
   * `reason: "error"`), and `retry: 0` so `ky` doesn't stack its own retries on top
   * of the single explicit one here.
   */
  async probeHealth(): Promise<{ response: Response; protocol: AcceleratorProtocol }> {
    const httpUrl = `http://${this.#host}:${this.#port}/health`;
    const httpsUrl = `https://${this.#host}:${this.#httpsPort}/health`;

    const probe = () =>
      Promise.any([
        ky(httpUrl, {
          retry: 0,
          throwHttpErrors: false,
          timeout: HEALTH_PROBE_TIMEOUT_MS,
        }).then((response) => ({ response, protocol: "http" as const })),
        ky(httpsUrl, {
          retry: 0,
          throwHttpErrors: false,
          timeout: HEALTH_PROBE_TIMEOUT_MS,
        }).then((response) => ({ response, protocol: "https" as const })),
      ]);

    try {
      return await probe();
    } catch {
      // Both probes failed — retry once (the accelerator may be slow to start on
      // first launch or just after an update). Then let a second failure propagate.
      await new Promise((resolve) => setTimeout(resolve, PROBE_RETRY_DELAY_MS));
      return probe();
    }
  }

  /**
   * POST serialized execution steps to `/prove` on the negotiated endpoint. Throws
   * `ky`'s `HTTPError` on a non-2xx response (the caller maps `403` → origin denial).
   */
  async postProve(
    body: Uint8Array<ArrayBuffer>,
    aztecVersion: string | undefined,
  ): Promise<Response> {
    return ky.post(`${this.baseUrl}/prove`, {
      body,
      timeout: PROVE_TIMEOUT_MS,
      retry: 0,
      headers: {
        "content-type": "application/octet-stream",
        ...(aztecVersion ? { "x-aztec-version": aztecVersion } : {}),
      },
    });
  }
}
