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
/**
 * Deadline + byte cap for reading a `/health` BODY. Ky's `timeout` only bounds time-to-headers — a
 * responder that returns `200` and then stalls (or streams forever) would otherwise hang the probe
 * indefinitely and buffer unbounded bytes (post-impl codex High). The real body is <2 KB.
 */
const HEALTH_BODY_TIMEOUT_MS = 2_000;
const HEALTH_BODY_MAX_BYTES = 64 * 1024;
/** /prove is long-running (native bb proof) — generous timeout. */
const PROVE_TIMEOUT_MS = ms("10 min");

/**
 * A settled `/health` probe: the {@link Response}, which protocol reached it, and the response body
 * parsed ONCE under {@link HEALTH_BODY_TIMEOUT_MS}/{@link HEALTH_BODY_MAX_BYTES} (`undefined` =
 * unparseable, over-cap, or stalled). The body stream is consumed here — callers use `body`, never
 * `response.json()`.
 */
type ProbeResult = { response: Response; protocol: AcceleratorProtocol; body: unknown };

/** q7e3-F-06: the three protocol-pin transitions {@link AcceleratorTransport.commitStatus} can apply. */
export type ProtocolTransition =
  | { pin: "set"; protocol: AcceleratorProtocol }
  | { pin: "clear" }
  | { pin: "keep" };

/**
 * The exact health-body contract the accelerator has always served (`{"status":"ok","api_version":1}`
 * on both the minimal and detailed `/health`). Mirrors the app's OWN redundant-instance classifier
 * (`core/src/server/probe.rs::is_healthy_aztec_response`) — anything weaker let a foreign 200-JSON
 * responder on the fixed HTTPS port win the protocol pin by merely *having* a recognizable field
 * (post-impl codex High: pin poisoning → witness exfiltration). Field-presence is NOT identity;
 * this shape check is collision resistance, not authentication — see the `httpsOnly` docs.
 */
export function isRecognizedHealthBody(body: unknown): boolean {
  if (typeof body !== "object" || body === null || Array.isArray(body)) return false;
  const b = body as Record<string, unknown>;
  return b.status === "ok" && b.api_version === 1;
}

/**
 * Read + JSON-parse a response body with a hard deadline and byte cap. Consumes the body (no
 * `clone()` — a clone tees the stream and can buffer an unbounded pending branch). Returns
 * `undefined` on any failure: non-JSON, over-cap, deadline, or stream error.
 */
async function readJsonBounded(response: Response): Promise<unknown> {
  try {
    const stream = response.body;
    if (!stream) {
      // No stream (exotic environments / test doubles): deadline-race a plain text() read, ALWAYS
      // clearing the timer so it can't keep the event loop alive past the read (codex Low).
      let timer: ReturnType<typeof setTimeout> | undefined;
      const text = await Promise.race([
        response.text(),
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error("health body deadline")),
            HEALTH_BODY_TIMEOUT_MS,
          );
        }),
      ]).finally(() => clearTimeout(timer));
      // Measure ENCODED bytes, not UTF-16 code units (codex Low).
      if (new TextEncoder().encode(text).length > HEALTH_BODY_MAX_BYTES) return undefined;
      return JSON.parse(text);
    }
    const reader = stream.getReader();
    const chunks: Uint8Array[] = [];
    let total = 0;
    // A deadline that fires mid-body cancels the reader → the pending read() resolves `done` with a
    // PARTIAL buffer. Track that it fired so a truncated-but-coincidentally-valid body is NOT parsed as
    // healthy (codex Medium). Cancellation is fire-and-forget — a never-settling `cancel()` must not
    // hang us (codex Medium).
    let timedOut = false;
    const deadline = setTimeout(() => {
      timedOut = true;
      void reader.cancel().catch(() => {});
    }, HEALTH_BODY_TIMEOUT_MS);
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        total += value.byteLength;
        if (total > HEALTH_BODY_MAX_BYTES) {
          void reader.cancel().catch(() => {}); // fire-and-forget — do not await a possibly-stuck cancel
          return undefined;
        }
        chunks.push(value);
      }
    } finally {
      clearTimeout(deadline);
    }
    if (timedOut) return undefined; // partial body from a deadline cancel — never "healthy"
    const merged = new Uint8Array(total);
    let offset = 0;
    for (const c of chunks) {
      merged.set(c, offset);
      offset += c.byteLength;
    }
    return JSON.parse(new TextDecoder().decode(merged));
  } catch {
    return undefined;
  }
}

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
  /** Cache timestamps use performance.now() — a wall-clock (Date.now) step backwards must not extend the TTL. */
  #statusCache: { result: AcceleratorStatus; timestamp: number } | null = null;
  /**
   * Endpoint-configuration generation. Bumped by {@link configure} so a probe that was in flight
   * against the OLD endpoint cannot commit its result (pin + cache) against the NEW one — a stale
   * commit would route `/prove` (and the witness in it) to an endpoint that was never probed
   * (post-impl codex High).
   */
  #generation = 0;

  constructor(host: string, port: number, httpsPort: number, httpsOnly = false) {
    this.#host = host;
    this.#port = port;
    this.#httpsPort = httpsPort;
    this.#httpsOnly = httpsOnly;
  }

  /**
   * Update connection settings. Resets the negotiated protocol and the status cache — each is keyed
   * to the old endpoint — and bumps the generation so any in-flight probe's commit is discarded.
   */
  configure(config: { port?: number; httpsPort?: number; host?: string; httpsOnly?: boolean }) {
    if (config.port !== undefined) this.#port = config.port;
    if (config.httpsPort !== undefined) this.#httpsPort = config.httpsPort;
    if (config.host !== undefined) this.#host = config.host;
    if (config.httpsOnly !== undefined) this.#httpsOnly = config.httpsOnly;
    this.#protocol = null;
    this.#statusCache = null;
    this.#generation++;
  }

  /** The current configuration generation — capture before a probe, pass to {@link commitStatus}. */
  get generation(): number {
    return this.#generation;
  }

  /** Whether strict HTTPS-only mode is on (no HTTP URL is ever constructed). */
  get httpsOnly(): boolean {
    return this.#httpsOnly;
  }

  /** Pin (or clear, with `null`) the protocol that `/prove` should use. */
  setProtocol(protocol: AcceleratorProtocol | null) {
    this.#protocol = protocol;
  }

  /**
   * Drop an HTTPS pin after a network-level `/prove` failure so the retry (and the next probe) can
   * use HTTP. No-op in strict mode or when the pin isn't `https`. Also invalidates the status cache —
   * it described an endpoint that just failed at the transport layer. Returns whether a demotion
   * happened (the caller uses this to decide on an HTTP retry).
   */
  demoteHttpsPin(): boolean {
    if (this.#httpsOnly || this.#protocol !== "https") return false;
    this.#protocol = null;
    this.#statusCache = null;
    return true;
  }

  /**
   * q7e3-F-06: single owner of the protocol-pin transition that the prover's probe previously
   * scattered across three sites. Caches the parsed status AND applies the pin in one place, with the
   * three transitions made explicit so a refactor can't silently flatten them:
   * - `"set"`   — a parseable OK `/health` → pin the winning protocol (drives subsequent `/prove`).
   * - `"clear"` — malformed-JSON or offline → unpin (a misbehaving/absent responder must not drive `/prove`).
   * - `"keep"`  — a non-OK status (`!response.ok`) → leave any EXISTING pin untouched (a fast error,
   *               e.g. an HTTPS cert failure, must not repin and must not clear a good pin).
   *
   * When `generation` is given and no longer current (the endpoint was reconfigured while this
   * probe was in flight), the commit is DISCARDED — neither pin nor cache mutates — and the stale
   * status is returned to its caller only.
   */
  commitStatus(
    status: AcceleratorStatus,
    transition: ProtocolTransition,
    generation?: number,
  ): AcceleratorStatus {
    if (generation !== undefined && generation !== this.#generation) return status;
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
    if (
      this.#statusCache &&
      performance.now() - this.#statusCache.timestamp < STATUS_CACHE_TTL_MS
    ) {
      return this.#statusCache.result;
    }
    return null;
  }

  /** Store a freshly-computed status and return it (call-site convenience). */
  cacheStatus(status: AcceleratorStatus): AcceleratorStatus {
    this.#statusCache = { result: status, timestamp: performance.now() };
    return status;
  }

  /**
   * Probe `/health`, **preferring HTTPS only when it's healthy**. One retry after
   * {@link PROBE_RETRY_DELAY_MS} if both fail the first time.
   *
   * Selection (plan §4 / audit R2, hardened post-impl): HTTPS wins iff it fulfills with
   * `response.ok` AND a body matching the accelerator's own health contract
   * ({@link isRecognizedHealthBody}: `status:"ok"`, `api_version:1`) — a fulfilled-but-non-OK,
   * 200-but-malformed, or 200-but-foreign-JSON HTTPS (possible via a server squatting the fixed
   * HTTPS port, since `throwHttpErrors:false`) does NOT beat a healthy HTTP responder. Otherwise the
   * HTTP result decides. If HTTP answers OK while HTTPS is still pending, HTTPS gets at most
   * {@link HTTPS_GRACE_MS} to preempt; a refused HTTPS resolves in ~0ms so the common no-HTTPS path
   * adds no latency.
   *
   * Every settled probe's body is read exactly once under a deadline + byte cap
   * ({@link readJsonBounded}) and returned as `body` — a `200` that stalls its body can neither hang
   * the probe nor buffer unbounded memory.
   *
   * Resolves with the winning {@link ProbeResult}; rejects only if BOTH probes fail twice (caller
   * maps that to `reason: "offline"`). `throwHttpErrors:false` so a non-2xx still *resolves* (caller
   * maps it to `reason: "error"`); `retry:0` so `ky` doesn't stack its own retries.
   *
   * In strict {@link AcceleratorTransport.#httpsOnly} mode, only the HTTPS endpoint is ever probed
   * (no `http://` URL is constructed); an unreachable HTTPS ⇒ rejects ⇒ caller maps to `offline`.
   */
  async probeHealth(): Promise<ProbeResult> {
    const httpsUrl = `https://${this.#host}:${this.#httpsPort}/health`;

    const fire = (url: string, protocol: AcceleratorProtocol): Promise<ProbeResult> =>
      ky(url, { retry: 0, throwHttpErrors: false, timeout: HEALTH_PROBE_TIMEOUT_MS }).then(
        async (response) => ({ response, protocol, body: await readJsonBounded(response) }),
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

  /** "Healthy" for winner-selection: 2xx + the recognized accelerator health-body contract. */
  #isHealthy(r: ProbeResult): boolean {
    return r.response.ok && isRecognizedHealthBody(r.body);
  }

  /**
   * Prefer-HTTPS-when-healthy selection over two in-flight probes. See {@link probeHealth} for the
   * contract. Structured so: (a) a healthy HTTPS wins the instant it appears — even before HTTP
   * settles — but a non-healthy HTTPS never preempts a still-pending HTTP; (b) once HTTP settles OK,
   * HTTPS gets a bounded {@link HTTPS_GRACE_MS} grace, and a HTTPS that already settled (refused /
   * unhealthy) is not waited on; (c) if HTTP isn't OK, a healthy HTTPS is awaited fully — safe now
   * that the body read is deadline-bounded, so `httpsHealthy` always settles — else any fulfilled
   * response is returned for the caller to map, else both-failed throws.
   */
  async #probePreferHttps(
    httpsP: Promise<ProbeResult>,
    httpP: Promise<ProbeResult>,
  ): Promise<ProbeResult> {
    const never = new Promise<never>(() => {});
    const delay = (msTimeout: number) => new Promise((r) => setTimeout(r, msTimeout));

    // Resolves to the HTTPS ProbeResult iff it's healthy, else null (on unhealthy OR rejected).
    // Settlement is guaranteed: fire() bounds both headers (ky timeout) and body (readJsonBounded).
    const httpsHealthy: Promise<ProbeResult | null> = httpsP.then(
      (r) => (this.#isHealthy(r) ? r : null),
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
    // Only a HEALTHY HTTP (recognized contract) gets to win via the short grace — a foreign 2xx that
    // merely settled first must NOT beat a healthy-but-slightly-slower HTTPS (codex Medium: the old
    // check was just `response.ok`). An unhealthy/foreign HTTP falls through to await HTTPS fully.
    if (httpRes && this.#isHealthy(httpRes)) {
      // Prefer HTTPS only if it becomes healthy within the grace window; a HTTPS that already settled
      // (refused/unhealthy → null) short-circuits the wait.
      const graced = await Promise.race<ProbeResult | null | "timeout">([
        httpsHealthy,
        delay(HTTPS_GRACE_MS).then(() => "timeout" as const),
      ]);
      if (graced && graced !== "timeout") return graced;
      return httpRes;
    }

    // HTTP rejected, non-OK, OR ok-but-not-the-health-contract. Wait FULLY for a healthy HTTPS (the
    // body read is bounded, so it always settles) — a healthy HTTPS must win here; else return any
    // fulfilled response so the caller maps it (unrecognized → "error"); else both failed → throw
    // (caller maps to "offline").
    const healthy = await httpsHealthy;
    if (healthy) return healthy;
    if (httpRes) return httpRes;
    const httpsAny = await httpsSettled;
    if (httpsAny) return httpsAny;
    throw new Error("both /health probes failed");
  }

  /** The `/prove` URL for an EXPLICIT protocol, independent of the current pin — used by the demotion
   * retry so a mid-proof `configure()` (generation change) can't redirect the retried witness. */
  proveUrlFor(protocol: AcceleratorProtocol): string {
    return protocol === "https"
      ? `https://${this.#host}:${this.#httpsPort}/prove`
      : `http://${this.#host}:${this.#port}/prove`;
  }

  /**
   * POST serialized execution steps to `/prove` on the negotiated endpoint (`baseUrl`), or — when
   * `url` is given — to that EXACT url (the demotion retry passes an explicit `http://` url so it
   * targets the endpoint THIS attempt was made against, not a since-reconfigured pin). Throws `ky`'s
   * `HTTPError` on a non-2xx response (the caller maps `403` → origin denial).
   */
  async postProve(
    body: Uint8Array<ArrayBuffer>,
    aztecVersion: string | undefined,
    url?: string,
  ): Promise<Response> {
    return ky.post(url ?? `${this.baseUrl}/prove`, {
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
