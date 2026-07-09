import ky from "ky";
import ms from "ms";
// q7e3-F-02: import shared types from the neutral module, not back from the prover (kills the 2-way edge).
import type { AcceleratorProtocol, AcceleratorStatus } from "./types.js";

/** How long a probed {@link AcceleratorStatus} stays fresh before a re-probe. */
const STATUS_CACHE_TTL_MS = 10_000;
/** Per-attempt timeout for each /health probe (HTTP and HTTPS fired in parallel). */
const HEALTH_PROBE_TIMEOUT_MS = 2_000;
/** Delay before the single /health retry when the first parallel probe fails. */
const PROBE_RETRY_DELAY_MS = 1_000;
/**
 * Max extra wait for a still-pending HTTPS probe once HTTP has already answered OK. Bounds the
 * "prefer HTTPS" preference so a bound-but-stalled HTTPS listener can't delay a healthy HTTP path by
 * more than this. It is only ever paid when HTTPS is *pending* at the moment HTTP settles OK — a
 * refused/rejected HTTPS (nothing on the port) resolves in ~0ms, so the common no-HTTPS path pays
 * nothing (see {@link AcceleratorTransport.probeHealth}).
 */
const HTTPS_GRACE_MS = 250;
/** /prove is long-running (native bb proof) — generous timeout. */
const PROVE_TIMEOUT_MS = ms("10 min");

/** A settled `/health` probe: the {@link Response} and which protocol reached it. */
type ProbeResult = { response: Response; protocol: AcceleratorProtocol };

/** q7e3-F-06: the three protocol-pin transitions {@link AcceleratorTransport.commitStatus} can apply. */
export type ProtocolTransition =
  | { pin: "set"; protocol: AcceleratorProtocol }
  | { pin: "clear" }
  | { pin: "keep" };

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
  /** Strict mode: probe/POST over HTTPS ONLY, never construct an `http://` URL (dApp policy knob). */
  #httpsOnly: boolean;
  /** Protocol that last reached `/health`; pins which endpoint `/prove` uses. `null` = not yet negotiated. */
  #protocol: AcceleratorProtocol | null = null;
  #statusCache: { result: AcceleratorStatus; timestamp: number } | null = null;

  constructor(host: string, port: number, httpsPort: number, httpsOnly = false) {
    this.#host = host;
    this.#port = port;
    this.#httpsPort = httpsPort;
    this.#httpsOnly = httpsOnly;
  }

  /**
   * Update connection settings. Resets BOTH the negotiated protocol and the status
   * cache — each is keyed to the old endpoint, so a stale hit would report the wrong
   * host/port for up to the TTL.
   */
  configure(config: { port?: number; httpsPort?: number; host?: string; httpsOnly?: boolean }) {
    if (config.port !== undefined) this.#port = config.port;
    if (config.httpsPort !== undefined) this.#httpsPort = config.httpsPort;
    if (config.host !== undefined) this.#host = config.host;
    if (config.httpsOnly !== undefined) this.#httpsOnly = config.httpsOnly;
    this.#protocol = null;
    this.#statusCache = null;
  }

  /** Pin (or clear, with `null`) the protocol that `/prove` should use. */
  setProtocol(protocol: AcceleratorProtocol | null) {
    this.#protocol = protocol;
  }

  /**
   * q7e3-F-06: single owner of the protocol-pin transition that the prover's probe previously
   * scattered across three sites. Caches the parsed status AND applies the pin in one place, with the
   * three transitions made explicit so a refactor can't silently flatten them:
   * - `"set"`   — a parseable OK `/health` → pin the winning protocol (drives subsequent `/prove`).
   * - `"clear"` — malformed-JSON or offline → unpin (a misbehaving/absent responder must not drive `/prove`).
   * - `"keep"`  — a non-OK status (`!response.ok`) → leave any EXISTING pin untouched (a fast error,
   *               e.g. an HTTPS cert failure, must not repin and must not clear a good pin).
   */
  commitStatus(status: AcceleratorStatus, transition: ProtocolTransition): AcceleratorStatus {
    if (transition.pin === "set") this.#protocol = transition.protocol;
    else if (transition.pin === "clear") this.#protocol = null;
    // "keep" → #protocol unchanged.
    return this.cacheStatus(status);
  }

  /**
   * Base URL for `/prove`. In strict {@link AcceleratorTransport.#httpsOnly} mode it is ALWAYS the
   * HTTPS endpoint (an `http://` URL is never constructed, even before negotiation). Otherwise it is
   * `https` iff the negotiated protocol is `https`, else the `http` default.
   */
  get baseUrl(): string {
    if (this.#httpsOnly || this.#protocol === "https") {
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
   * Probe `/health`, **preferring HTTPS only when it's healthy**. One retry after
   * {@link PROBE_RETRY_DELAY_MS} if both fail the first time.
   *
   * Selection (see plan §4 / audit R2): HTTPS wins iff it fulfills with `response.ok` AND a
   * parseable JSON body — a fulfilled-but-non-OK or 200-but-malformed HTTPS (possible via a foreign
   * server squatting the fixed HTTPS port, since `throwHttpErrors:false`) does NOT beat a healthy
   * HTTP responder. Otherwise the HTTP result decides. If HTTP answers OK while HTTPS is still
   * pending, HTTPS gets at most {@link HTTPS_GRACE_MS} to preempt; a refused HTTPS resolves in ~0ms
   * so the common no-HTTPS path adds no latency.
   *
   * Resolves with the winning {@link Response} + protocol; rejects only if BOTH probes fail twice
   * (caller maps that to `reason: "offline"`). `throwHttpErrors:false` so a non-2xx still *resolves*
   * (caller maps it to `reason: "error"`); `retry:0` so `ky` doesn't stack its own retries.
   *
   * In strict {@link AcceleratorTransport.#httpsOnly} mode, only the HTTPS endpoint is ever probed
   * (no `http://` URL is constructed); an unreachable HTTPS ⇒ rejects ⇒ caller maps to `offline`.
   */
  async probeHealth(): Promise<ProbeResult> {
    const httpsUrl = `https://${this.#host}:${this.#httpsPort}/health`;

    const fire = (url: string, protocol: AcceleratorProtocol): Promise<ProbeResult> =>
      ky(url, { retry: 0, throwHttpErrors: false, timeout: HEALTH_PROBE_TIMEOUT_MS }).then(
        (response) => ({ response, protocol }),
      );

    const probe = () => {
      // Strict mode: probe HTTPS ONLY — never even *construct* an http URL (contract compliance).
      if (this.#httpsOnly) return fire(httpsUrl, "https");
      const httpUrl = `http://${this.#host}:${this.#port}/health`;
      return this.#probePreferHttps(fire(httpsUrl, "https"), fire(httpUrl, "http"));
    };

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
   * "Healthy" for winner-selection: a 2xx response with a parseable JSON *object* body. Uses
   * `response.clone()` so the caller can still read the original body (audit R2 — never consume the
   * body the prover needs for its own classification). A 200 with a non-JSON / non-object body is
   * NOT healthy, so it can't preempt a healthy HTTP responder.
   */
  async #isHealthy(response: Response): Promise<boolean> {
    if (!response.ok) return false;
    try {
      const body: unknown = await response.clone().json();
      return typeof body === "object" && body !== null;
    } catch {
      return false;
    }
  }

  /**
   * Prefer-HTTPS-when-healthy selection over two in-flight probes. See {@link probeHealth} for the
   * contract. Structured so: (a) a healthy HTTPS wins the instant it appears — even before HTTP
   * settles — but a non-healthy HTTPS never preempts a still-pending HTTP; (b) once HTTP settles OK,
   * HTTPS gets a bounded {@link HTTPS_GRACE_MS} grace, and a HTTPS that already settled (refused /
   * unhealthy) is not waited on; (c) if HTTP isn't OK, a healthy HTTPS is awaited fully, else any
   * fulfilled response is returned for the caller to map, else both-failed throws.
   */
  async #probePreferHttps(
    httpsP: Promise<ProbeResult>,
    httpP: Promise<ProbeResult>,
  ): Promise<ProbeResult> {
    const never = new Promise<never>(() => {});
    const delay = (msTimeout: number) => new Promise((r) => setTimeout(r, msTimeout));

    // Resolves to the HTTPS ProbeResult iff it's healthy, else null (on unhealthy OR rejected).
    const httpsHealthy: Promise<ProbeResult | null> = httpsP.then(
      async (r) => ((await this.#isHealthy(r.response)) ? r : null),
      () => null,
    );
    // Non-throwing views for the fallback decision.
    const httpSettled: Promise<ProbeResult | null> = httpP.then(
      (r) => r,
      () => null,
    );
    const httpsSettled: Promise<ProbeResult | null> = httpsP.then(
      (r) => r,
      () => null,
    );

    // Leading edge: a healthy HTTPS the instant it appears, else whatever HTTP settles to. A
    // non-healthy HTTPS maps to `never` so it can't win the race over a still-pending HTTP.
    type Lead = { kind: "https"; r: ProbeResult } | { kind: "http"; r: ProbeResult | null };
    const first = await Promise.race<Lead>([
      httpsHealthy.then((r) => (r ? { kind: "https" as const, r } : never)),
      httpSettled.then((r) => ({ kind: "http" as const, r })),
    ]);

    if (first.kind === "https") return first.r;

    const httpRes = first.r;
    if (httpRes && httpRes.response.ok) {
      // HTTP answered OK. Prefer HTTPS only if it becomes healthy within the grace window; a HTTPS
      // that already settled (refused/unhealthy → null) short-circuits the wait.
      const graced = await Promise.race<ProbeResult | null | "timeout">([
        httpsHealthy,
        delay(HTTPS_GRACE_MS).then(() => "timeout" as const),
      ]);
      if (graced && graced !== "timeout") return graced;
      return httpRes;
    }

    // HTTP rejected / non-OK. Wait fully for a healthy HTTPS; else return any fulfilled response so
    // the caller maps it (non-OK → "error"); else both failed → throw (caller maps to "offline").
    const healthy = await httpsHealthy;
    if (healthy) return healthy;
    if (httpRes) return httpRes;
    const httpsAny = await httpsSettled;
    if (httpsAny) return httpsAny;
    throw new Error("both /health probes failed");
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
