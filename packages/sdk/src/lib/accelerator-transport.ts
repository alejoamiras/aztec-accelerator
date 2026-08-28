import ms from "ms";
import type { AcceleratorProtocol, AcceleratorStatus } from "./types.js";
// q7e3-F-02: import shared types from the neutral module, not back from the prover (kills the 2-way edge).
import { ACCELERATOR_API_VERSION } from "./types.js";

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
 * Deadline + byte cap for reading a `/health` BODY. The request timeout only bounds time-to-headers
 * — a responder that returns `200` and then stalls (or streams forever) would otherwise hang the
 * probe indefinitely and buffer unbounded bytes (post-impl codex High). The real body is <2 KB.
 */
const HEALTH_BODY_TIMEOUT_MS = 2_000;
const HEALTH_BODY_MAX_BYTES = 64 * 1024;
/** /prove is long-running (native bb proof) — generous timeout. */
const PROVE_TIMEOUT_MS = ms("10 min");

/**
 * Deadline + byte cap for reading a `/prove` BODY (F-11, audit 2026-07-31-9c4cb0c). `PROVE_TIMEOUT_MS`
 * above bounds time-to-headers — so before this, an endpoint could
 * answer `200` and then stream forever into an unbounded buffer.
 *
 * **The cap is derived, not sampled.** A single observed proof is a lower bound, not a protocol
 * maximum, so it is computed from the protocol's own declared sizes:
 *
 * - `@aztec/stdlib`'s `chonk_proof.ts` documents the proof data itself as **≈52 KB** uncompressed
 *   ("always >= 40KB") and ≈35 KB compressed.
 * - Public inputs ride along on top: `numPublicInputs = fields.length - CHONK_PROOF_LENGTH`, and the
 *   widest relevant kernel output is `PRIVATE_TO_ROLLUP_KERNEL_CIRCUIT_PUBLIC_INPUTS_LENGTH` = 1281
 *   fields × 32 B ≈ 41 KB.
 * - So ≈93 KB of raw proof, ≈124 KB once base64'd, plus a negligible JSON envelope.
 *
 * 8 MiB is therefore ~65× the largest response the protocol can currently produce — room for the
 * scheme to grow an order of magnitude without a code change — while still far too small to exhaust a
 * browser tab. Sizing this too LOW breaks proving for every user, which is why the headroom is large
 * and the derivation is written down rather than left as a magic number.
 */
const PROVE_BODY_TIMEOUT_MS = ms("60 sec");
const PROVE_BODY_MAX_BYTES = 8 * 1024 * 1024;

/**
 * Validate a configured `host` and return it, or throw (F-01, audit 2026-07-31).
 *
 * `host` was interpolated raw into six URL templates (`https://${host}:${port}/prove` and friends).
 * A value like `evil.com/#` or `evil.com:1/` re-points every one of them: the private witness is
 * POSTed to `/prove`, so a mis-set — or attacker-influenced — host sends it straight off the machine.
 * Parsing through `URL` is what makes that unrepresentable: the value must survive round-tripping as
 * a pure hostname, so anything carrying a path, port, query, fragment, credentials, or scheme is
 * rejected rather than silently reinterpreted.
 *
 * Loopback-only is a real constraint, not paranoia: the accelerator is a local service, and its HTTPS
 * identity is a CA name-constrained to loopback, so a remote host could not present a certificate the
 * SDK's own trust model accepts. A non-loopback host is therefore always a misconfiguration.
 */
export function assertPort(port: number, field: string): number {
  // Validating `host` alone left the authority half-open. `port` is TYPED `number`, but TypeScript
  // types are erased at runtime, so a JS caller — or a config that came from JSON, a URL parameter,
  // or an env var — can pass a string. `{host: "127.0.0.1", port: "80@evil.com"}` builds
  // `http://127.0.0.1:80@evil.com/prove`, whose real authority is `evil.com`: `127.0.0.1` becomes
  // the username and `80` the password. The witness goes to a remote host that never had to defeat
  // the host check at all. (Found by the codex pass over the F-01 fix; verified by construction.)
  if (typeof port !== "number" || !Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error(
      `Invalid accelerator ${field} ${JSON.stringify(port)}: expected an integer from 1 to 65535.`,
    );
  }
  return port;
}

export function assertFlag(value: boolean, field: string): boolean {
  // The SAME runtime-erasure argument that justifies `assertPort` — and I applied it to the numbers
  // and not to the booleans, which is worse, because these two ARE the transport's security policy
  // (codex round 4). `configure({allowInsecureDowngrade: "false"})` assigned the string `"false"`,
  // which is truthy, so the documented opt-out switched ON via a value that reads as OFF. Coercion
  // would repeat the mistake in a quieter form; a non-boolean here means the caller's config is not
  // what they think it is, and they should hear about it.
  if (typeof value !== "boolean") {
    throw new Error(
      `Invalid accelerator ${field} ${JSON.stringify(value)}: expected true or false.`,
    );
  }
  return value;
}

export function assertLoopbackHost(host: string): string {
  const reject = (why: string): never => {
    throw new Error(
      `Invalid accelerator host ${JSON.stringify(host)}: ${why}. ` +
        `Expected a loopback host such as "127.0.0.1", "::1", or "localhost".`,
    );
  };
  if (typeof host !== "string" || host.length === 0) reject("it must be a non-empty string");

  let parsed: URL;
  try {
    // Bracket a bare IPv6 literal so `::1` parses as a host rather than a scheme.
    const literal = host.includes(":") && !host.startsWith("[") ? `[${host}]` : host;
    parsed = new URL(`http://${literal}`);
  } catch {
    return reject("it is not a bare hostname");
  }
  // `new URL` would happily absorb a path, port, credentials or query into the surrounding template.
  if (parsed.port !== "" || parsed.username !== "" || parsed.password !== "") {
    reject("it must not carry a port or credentials");
  }
  if (parsed.pathname !== "/" || parsed.search !== "" || parsed.hash !== "") {
    reject("it must not carry a path, query, or fragment");
  }

  // `URL` normalises IPv6 to a bracketed lowercase form and IPv4 to dotted-quad, so compare on that.
  const h = parsed.hostname.toLowerCase();
  const isLoopback =
    h === "localhost" ||
    h === "[::1]" ||
    // The whole 127.0.0.0/8 block, which is what the OS actually routes to the loopback interface.
    /^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(h);
  if (!isLoopback) reject("it is not a loopback address");

  // Return the NORMALISED spelling, which is also the only one the URL templates can use: `URL`
  // brackets IPv6 (`::1` → `[::1]`, required before `:port`) and canonicalises short IPv4 forms
  // (`127.1` → `127.0.0.1`). Interpolating the raw `::1` would have built `https://::1:59834`.
  return h;
}

/**
 * A settled `/health` probe: the {@link Response}, which protocol reached it, and the response body
 * parsed ONCE under {@link HEALTH_BODY_TIMEOUT_MS}/{@link HEALTH_BODY_MAX_BYTES} (`undefined` =
 * unparseable, over-cap, or stalled). The body stream is consumed here — callers use `body`, never
 * `response.json()`.
 */
type ProbeResult = { response: Response; protocol: AcceleratorProtocol; body: unknown };

/** Experimental Fetch/LNA types kept local until they are part of TypeScript's `lib.dom`. */
type LoopbackRequestInit = RequestInit & { targetAddressSpace?: "loopback" };
type LoopbackRequest = Request & { readonly targetAddressSpace?: string };
type LoopbackPermissionName = "loopback-network" | "local-network-access";
type LoopbackPermissionDescriptor = { name: LoopbackPermissionName };
type LoopbackPermissions = {
  query(descriptor: LoopbackPermissionDescriptor): Promise<{ state: PermissionState }>;
};

/** Payload-free private sentinels: raw browser fetch failures never cross the transport boundary. */
const PROBE_FAILED = Object.freeze({});
const LOOPBACK_PERMISSION_DENIED = Object.freeze({});

/** Internal-only discriminator used by the prover to map the public blocked status. */
export function isLoopbackPermissionDenied(error: unknown): boolean {
  return error === LOOPBACK_PERMISSION_DENIED;
}

/** SSR-safe capability check for the experimental LNA fetch annotation. */
export function supportsLoopbackTargetAddressSpace(): boolean {
  try {
    return (
      typeof Request !== "undefined" &&
      typeof Request.prototype === "object" &&
      "targetAddressSpace" in (Request.prototype as LoopbackRequest)
    );
  } catch {
    return false;
  }
}

/**
 * Query the browser's fine-grained loopback permission, falling back to the legacy umbrella name only
 * when the modern descriptor itself is rejected. Missing APIs, prompt/granted, and query failures
 * are intentionally inconclusive.
 */
async function isLoopbackPermissionExplicitlyDenied(): Promise<boolean> {
  let permissions: LoopbackPermissions | undefined;
  try {
    if (typeof navigator === "undefined") return false;
    permissions = navigator.permissions as unknown as LoopbackPermissions | undefined;
    if (!permissions || typeof permissions.query !== "function") return false;
  } catch {
    return false;
  }

  try {
    const status = await permissions.query({ name: "loopback-network" });
    return status.state === "denied";
  } catch {
    try {
      const status = await permissions.query({ name: "local-network-access" });
      return status.state === "denied";
    } catch {
      return false;
    }
  }
}

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
/**
 * Non-2xx envelope for `/prove`: distinguishes "the accelerator ANSWERED with an HTTP error"
 * from "no response at all" — the F14 classifier keys on exactly that split, and confusing the
 * two can activate the plaintext downgrade retry. `data` carries the bounded pre-read error body
 * (an object for JSON bodies; a string for the server's text/plain JSON-string bodies; undefined
 * when the body stalled, overflowed the cap, or failed to parse — a body-read failure must never
 * demote the classification to a network failure). Internal: exported for the prover, not from
 * the package barrel.
 */
export class TransportHttpError extends Error {
  constructor(
    readonly response: Response,
    readonly data: unknown,
  ) {
    super(`accelerator answered HTTP ${response.status}`);
    this.name = "TransportHttpError";
  }
}

/**
 * `fetch` with a HEADER deadline only: the timer is cleared the moment the response settles, so
 * the body-read budget belongs exclusively to the bounded readers. A signal that keeps ticking
 * after headers would silently shrink the body readers' independent deadline by however long the
 * headers took.
 */
async function fetchHeaderBounded(
  url: string,
  init: RequestInit,
  timeoutMs: number,
): Promise<Response> {
  const ctrl = new AbortController();
  const timer = setTimeout(
    () => ctrl.abort(new DOMException("header deadline", "TimeoutError")),
    timeoutMs,
  );
  try {
    const boundedInit: LoopbackRequestInit = { ...init, signal: ctrl.signal };
    // Declare the intended address space only for plaintext loopback traffic. HTTPS never needs the
    // mixed-content exemption, and omitting the experimental member in unsupported runtimes avoids
    // WebKit's released targetAddressSpace regression. The annotation does not bypass permission.
    if (new URL(url).protocol === "http:" && supportsLoopbackTargetAddressSpace()) {
      boundedInit.targetAddressSpace = "loopback";
    }
    return await fetch(url, boundedInit);
  } finally {
    clearTimeout(timer);
  }
}

export function isRecognizedHealthBody(body: unknown): boolean {
  if (typeof body !== "object" || body === null || Array.isArray(body)) return false;
  const b = body as Record<string, unknown>;
  return b.status === "ok" && b.api_version === ACCELERATOR_API_VERSION;
}

/**
 * Read + JSON-parse a response body with a hard deadline and byte cap. Consumes the body (no
 * `clone()` — a clone tees the stream and can buffer an unbounded pending branch). Returns
 * `undefined` on any failure: non-JSON, over-cap, deadline, or stream error.
 *
 * The cap and deadline are PARAMETERS, defaulting to the `/health` policy (F-11, audit
 * 2026-07-31-9c4cb0c). `/prove` returns JSON too — `{"proof": "<base64>"}` — so it reuses this exact
 * reader rather than getting a second one: the empty-chunk, partial-body and never-settling-cancel
 * defences below were each found by adversarial review, and a parallel implementation would have to
 * re-earn all of them. Only the POLICY differs per endpoint; the mechanics must not.
 */
async function readJsonBounded(
  response: Response,
  maxBytes: number = HEALTH_BODY_MAX_BYTES,
  timeoutMs: number = HEALTH_BODY_TIMEOUT_MS,
): Promise<unknown> {
  const text = await readTextBounded(response, maxBytes, timeoutMs);
  if (text === undefined) return undefined;
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

/** The bounded-read MECHANICS behind {@link readJsonBounded}, returning the raw text — the error
 * pre-read needs the undecoded shape (content-type decides string vs object, matching what
 * `parseServerError` distinguishes). Same single-implementation rule: policy varies, mechanics
 * don't. */
async function readTextBounded(
  response: Response,
  maxBytes: number = HEALTH_BODY_MAX_BYTES,
  timeoutMs: number = HEALTH_BODY_TIMEOUT_MS,
): Promise<string | undefined> {
  try {
    const stream = response.body;
    if (!stream) {
      // No stream (exotic environments / test doubles): deadline-race a plain text() read, ALWAYS
      // clearing the timer so it can't keep the event loop alive past the read (codex Low).
      let timer: ReturnType<typeof setTimeout> | undefined;
      const text = await Promise.race([
        response.text(),
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(new Error("body deadline")), timeoutMs);
        }),
      ]).finally(() => clearTimeout(timer));
      // Measure ENCODED bytes, not UTF-16 code units (codex Low).
      if (new TextEncoder().encode(text).length > maxBytes) return undefined;
      return text;
    }
    const reader = stream.getReader();
    // A CONTIGUOUS buffer grown geometrically, not an array of chunks. Retaining one `Uint8Array`
    // object per chunk bounds the BYTES but not the allocation: a peer that dribbles millions of
    // one-byte chunks stays under the cap while the per-object overhead consumes orders of magnitude
    // more (post-impl codex round 7). Copying as we go makes peak memory O(maxBytes).
    let buffer = new Uint8Array(Math.min(maxBytes, 64 * 1024));
    let total = 0;
    // A deadline that fires mid-body cancels the reader → the pending read() resolves `done` with a
    // PARTIAL buffer. Track that it fired so a truncated-but-coincidentally-valid body is NOT parsed as
    // healthy (codex Medium). Cancellation is fire-and-forget — a never-settling `cancel()` must not
    // hang us (codex Medium).
    let timedOut = false;
    const deadline = setTimeout(() => {
      timedOut = true;
      void reader.cancel().catch(() => {});
    }, timeoutMs);
    const started = performance.now();
    // A stream that endlessly enqueues ZERO-LENGTH chunks resolves every read() immediately, starving
    // the microtask loop so the `setTimeout` deadline never fires and `total` never grows (codex).
    // Three defences: an in-loop wall-clock check that does not depend on the timer; never RETAINING
    // empty chunks (their repro accumulated ~936 MB); and a hard cap on how many we tolerate, so the
    // spin ends immediately rather than burning CPU until the deadline.
    const MAX_EMPTY_CHUNKS = 64;
    let emptyChunks = 0;
    try {
      for (;;) {
        const { done, value } = await reader.read();
        // Elapsed check BEFORE `done`: a stream that spams empty chunks past the deadline and THEN
        // closes would otherwise reach `done` first, skip this check, clear the still-unfired timer,
        // and get parsed as healthy (codex Medium).
        if (performance.now() - started > timeoutMs) {
          timedOut = true;
          void reader.cancel().catch(() => {});
          return undefined;
        }
        if (done) break;
        if (value.byteLength === 0) {
          if (++emptyChunks > MAX_EMPTY_CHUNKS) {
            void reader.cancel().catch(() => {});
            return undefined;
          }
          continue; // never retained — empty chunks carry no body bytes
        }
        if (total + value.byteLength > maxBytes) {
          void reader.cancel().catch(() => {}); // fire-and-forget — do not await a possibly-stuck cancel
          return undefined;
        }
        if (total + value.byteLength > buffer.byteLength) {
          const grown = new Uint8Array(
            Math.min(maxBytes, Math.max(buffer.byteLength * 2, total + value.byteLength)),
          );
          grown.set(buffer.subarray(0, total));
          buffer = grown;
        }
        buffer.set(value, total);
        total += value.byteLength;
      }
    } finally {
      clearTimeout(deadline);
    }
    if (timedOut) return undefined; // partial body from a deadline cancel — never "healthy"
    const merged = buffer.subarray(0, total);
    return new TextDecoder().decode(merged);
  } catch {
    return undefined;
  }
}

/**
 * Owns all network I/O to the local accelerator: endpoint/URL construction, the
 * dual HTTP/HTTPS `/health` probe + protocol negotiation, the short-lived status
 * cache, and the `/prove` POST. Plain `fetch` for both endpoints; a non-2xx
 * `/prove` answer throws {@link TransportHttpError}, so the thrown-error surface
 * is uniform.
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

  /**
   * Has a healthy HTTPS accelerator ever answered at the CURRENT endpoint? Set by
   * {@link commitStatus} when a probe pins `https`, cleared by {@link configure}.
   *
   * F-01 (audit 2026-07-31): this is what turns "prefer HTTPS" into "do not DOWNGRADE from HTTPS".
   * Preference alone was defeated by a single network-layer `/prove` failure, after which the SDK
   * retried the same private witness over plaintext HTTP.
   */
  #httpsWasHealthy = false;

  /** Escape hatch for the above. See {@link AcceleratorConfig.allowInsecureDowngrade}. */
  #allowInsecureDowngrade: boolean;

  constructor(
    host: string,
    port: number,
    httpsPort: number,
    httpsOnly = false,
    allowInsecureDowngrade = false,
  ) {
    this.#host = assertLoopbackHost(host);
    this.#port = assertPort(port, "port");
    this.#httpsPort = assertPort(httpsPort, "httpsPort");
    this.#httpsOnly = assertFlag(httpsOnly, "httpsOnly");
    this.#allowInsecureDowngrade = assertFlag(allowInsecureDowngrade, "allowInsecureDowngrade");
  }

  /**
   * Update connection settings. Resets the negotiated protocol and the status cache — each is keyed
   * to the old endpoint — and bumps the generation so any in-flight probe's commit is discarded.
   */
  configure(config: {
    port?: number;
    httpsPort?: number;
    host?: string;
    httpsOnly?: boolean;
    allowInsecureDowngrade?: boolean;
  }) {
    // Read every property ONCE, up front. `config` is caller-supplied, so a property can be a getter
    // with side effects or one that throws — reading the policy flags only after the addresses were
    // already committed left `{port: X, get httpsOnly() { throw } }` with a moved port and none of
    // the resets below (codex round 4). Reading first also means each property is observed exactly
    // once, so a getter cannot return one value to the validator and another to the assignment.
    const raw = {
      port: config.port,
      httpsPort: config.httpsPort,
      host: config.host,
      httpsOnly: config.httpsOnly,
      allowInsecureDowngrade: config.allowInsecureDowngrade,
    };

    // Then validate EVERYTHING into locals before touching a single field. The first version assigned
    // as it went, so `configure({port: attackerPort, host: "evil.com"})` left the port pointing
    // somewhere new while the host check threw — and because the throw skipped the resets below, the
    // stale "healthy" status cache stayed valid for an endpoint that had moved (codex round 2). A
    // rejected call must change nothing at all.
    const port = raw.port === undefined ? this.#port : assertPort(raw.port, "port");
    const httpsPort =
      raw.httpsPort === undefined ? this.#httpsPort : assertPort(raw.httpsPort, "httpsPort");
    // NORMALISED before comparing: `#host` holds the canonical spelling, so comparing the raw input
    // against it made `configure({host: "127.1"})` — or a bare "::1" against the stored "[::1]" —
    // read as a move and wipe the HTTPS history for an endpoint that never moved (codex round 2).
    // Losing that history is what re-opens the plaintext downgrade.
    const host = raw.host === undefined ? this.#host : assertLoopbackHost(raw.host);
    const httpsOnly =
      raw.httpsOnly === undefined ? this.#httpsOnly : assertFlag(raw.httpsOnly, "httpsOnly");
    const allowInsecureDowngrade =
      raw.allowInsecureDowngrade === undefined
        ? this.#allowInsecureDowngrade
        : assertFlag(raw.allowInsecureDowngrade, "allowInsecureDowngrade");

    const movedEndpoint =
      port !== this.#port || httpsPort !== this.#httpsPort || host !== this.#host;

    this.#port = port;
    this.#httpsPort = httpsPort;
    this.#host = host;
    this.#httpsOnly = httpsOnly;
    this.#allowInsecureDowngrade = allowInsecureDowngrade;
    this.#protocol = null;
    this.#statusCache = null;
    // A different ENDPOINT has its own HTTPS history — carrying the old one over would either block a
    // legitimate HTTP-only endpoint or, worse, vouch for a new one we have never reached over TLS.
    // Only an address change resets it: flipping a policy flag must not erase what we learned about
    // the endpoint we are still talking to (codex: any `setAcceleratorConfig` used to clear it).
    if (movedEndpoint) this.#httpsWasHealthy = false;
    this.#generation++;
  }

  /**
   * Should this transport behave as HTTPS-only right now? True in explicit strict mode, and ALSO
   * once a healthy HTTPS accelerator has answered at this endpoint (unless the integrator opted out).
   *
   * The second half is the fix for a hole the first version of F-01 left open: `allowsHttpDowngrade`
   * was consulted only when an HTTPS `/prove` FAILED, so the plaintext path was still reachable by
   * going around it — let the 10s status cache expire, take the HTTP port while HTTPS is
   * unavailable, and the next dual probe simply pins HTTP with no downgrade check in sight. Refusing
   * to construct an `http://` URL at PROBE time is what actually closes it: an unreachable HTTPS
   * endpoint then reports offline and the prover falls back to WASM.
   */
  get #effectiveHttpsOnly(): boolean {
    return this.#httpsOnly || !this.allowsHttpDowngrade;
  }

  /** The current configuration generation — capture before a probe, pass to {@link commitStatus}. */
  get generation(): number {
    return this.#generation;
  }

  /** Whether strict HTTPS-only mode is on (no HTTP URL is ever constructed). */
  get httpsOnly(): boolean {
    return this.#httpsOnly;
  }

  /**
   * May a failed HTTPS `/prove` be retried over plaintext HTTP right now? (F-01.)
   *
   * False once a healthy HTTPS accelerator has answered at this endpoint, unless the integrator opted
   * in. The pre-fix code validated the HTTP endpoint before downgrading, but that check is the
   * `/health` SHAPE contract — its own comment calls it collision resistance, not authentication — so
   * any local account that binds 127.0.0.1:59833 answers it and receives the witness.
   */
  get allowsHttpDowngrade(): boolean {
    if (this.#httpsOnly) return false;
    return !this.#httpsWasHealthy || this.#allowInsecureDowngrade;
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
    if (this.#effectiveHttpsOnly || this.#protocol !== "https") return false;
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
    if (transition.pin === "set") {
      this.#protocol = transition.protocol;
      // F-01: remember that TLS worked here. Only a "set" counts — it is the transition that follows
      // a 2xx `/health` matching the accelerator's body contract.
      if (transition.protocol === "https") this.#httpsWasHealthy = true;
    } else if (transition.pin === "clear") this.#protocol = null;
    // "keep" → #protocol unchanged.
    return this.cacheStatus(status);
  }

  /**
   * Base URL for `/prove`. In strict {@link AcceleratorTransport.#httpsOnly} mode it is ALWAYS the
   * HTTPS endpoint (an `http://` URL is never constructed, even before negotiation). Otherwise it is
   * `https` iff the negotiated protocol is `https`, else the `http` default.
   */
  get baseUrl(): string {
    if (this.#effectiveHttpsOnly || this.#protocol === "https") {
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
   * maps it to `reason: "error"`); nothing retries under the caller's retry logic.
   *
   * In strict {@link AcceleratorTransport.#httpsOnly} mode, only the HTTPS endpoint is ever probed
   * (no `http://` URL is constructed); an unreachable HTTPS ⇒ rejects ⇒ caller maps to `offline`.
   */
  async probeHealth(): Promise<ProbeResult> {
    const httpsUrl = `https://${this.#host}:${this.#httpsPort}/health`;

    const fire = (url: string, protocol: AcceleratorProtocol): Promise<ProbeResult> =>
      fetchHeaderBounded(
        url,
        {
          // A 307/308 is a downgrade vector: fetch preserves the method AND body across those,
          // so an https->http redirect would carry the request off the endpoint we validated —
          // and in strict mode, off HTTPS entirely (post-impl codex High). Never follow.
          redirect: "error",
        },
        // Bounds time-to-headers only; a non-2xx still RESOLVES (the caller maps it to `reason:
        // "error"`), and nothing here retries on its own.
        HEALTH_PROBE_TIMEOUT_MS,
      ).then(async (response) => ({ response, protocol, body: await readJsonBounded(response) }));

    const probe = () => {
      // Strict mode: probe HTTPS ONLY — never even *construct* an http URL (contract compliance).
      // Also taken once HTTPS has proven healthy here, which is what stops a later probe from
      // quietly re-pinning plaintext (see `#effectiveHttpsOnly`).
      if (this.#effectiveHttpsOnly) return fire(httpsUrl, "https");
      const httpUrl = `http://${this.#host}:${this.#port}/health`;
      return this.#probePreferHttps(fire(httpsUrl, "https"), fire(httpUrl, "http"));
    };

    const probeRound = async (): Promise<ProbeResult> => {
      try {
        return await probe();
      } catch {
        // Fetch deliberately exposes LNA denials as indistinguishable network errors. Only the
        // permission state can classify one, and only explicit `denied` is conclusive.
        if (await isLoopbackPermissionExplicitlyDenied()) throw LOOPBACK_PERMISSION_DENIED;
        throw PROBE_FAILED;
      }
    };

    try {
      return await probeRound();
    } catch (error) {
      // A persistent permission denial cannot be repaired by waiting, so skip the startup retry.
      if (error === LOOPBACK_PERMISSION_DENIED) throw error;
      await new Promise((resolve) => setTimeout(resolve, PROBE_RETRY_DELAY_MS));
      return probeRound();
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
    // Settlement is guaranteed: fire() bounds both headers (abort signal) and body (readJsonBounded).
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

  /**
   * Probe ONE protocol's `/health` and report whether it answers the accelerator's health contract.
   *
   * This exists for the `/prove` downgrade path, which must NEVER hand the witness to an endpoint it
   * has not itself validated. A healthy HTTPS probe says nothing about who is listening on the HTTP
   * port — a foreign responder there would otherwise receive the serialized witness the moment HTTPS
   * failed (post-impl codex Critical). Bounded exactly like the dual probe: abort signal for headers,
   * {@link readJsonBounded} for the body.
   */
  async isProtocolHealthy(protocol: AcceleratorProtocol): Promise<boolean> {
    // Strict mode never speaks plaintext, so it must not even probe it — and neither does an
    // endpoint that already answered over HTTPS (codex: this is the validate-before-downgrade call,
    // so letting it run was the last way to reach the plaintext POST).
    if (protocol === "http" && this.#effectiveHttpsOnly) return false;
    const url =
      protocol === "https"
        ? `https://${this.#host}:${this.#httpsPort}/health`
        : `http://${this.#host}:${this.#port}/health`;
    try {
      const response = await fetchHeaderBounded(
        url,
        { redirect: "error" },
        HEALTH_PROBE_TIMEOUT_MS,
      );
      if (!response.ok) return false;
      return isRecognizedHealthBody(await readJsonBounded(response));
    } catch {
      return false;
    }
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
   * targets the endpoint THIS attempt was made against, not a since-reconfigured pin). Throws
   * {@link TransportHttpError} on a non-2xx response (the caller maps `403` → origin denial),
   * with the error body pre-read under the `/health` bounds — the status is preserved even when
   * that body stalls, overflows, or is malformed, so a hostile error body can never re-route the
   * classification to the network-failure path.
   */
  async postProve(
    body: Uint8Array<ArrayBuffer>,
    aztecVersion: string | undefined,
    url?: string,
  ): Promise<Response> {
    const response = await fetchHeaderBounded(
      url ?? `${this.baseUrl}/prove`,
      {
        method: "POST",
        body,
        // Never follow a redirect with the witness in the body: 307/308 preserve method + body, so a
        // redirect here would forward it to an endpoint we never validated (post-impl codex High).
        redirect: "error",
        headers: {
          "content-type": "application/octet-stream",
          ...(aztecVersion ? { "x-aztec-version": aztecVersion } : {}),
        },
      },
      PROVE_TIMEOUT_MS,
    );
    if (!response.ok) {
      // Content-type decides the pre-read shape, mirroring what parseServerError distinguishes:
      // the server's text/plain JSON-string bodies stay STRINGS, a JSON media type parses to an
      // object; an unreadable body is undefined — the status alone then drives classification.
      // Match on the media-type ESSENCE (parameters stripped, case-folded) and accept `+json`
      // suffixes — a substring test would misread `text/plain; note=application/json`.
      const raw = await readTextBounded(response);
      const essence = (response.headers.get("content-type") ?? "")
        .split(";")[0]
        ?.trim()
        .toLowerCase();
      const isJson = essence === "application/json" || (essence?.endsWith("+json") ?? false);
      let data: unknown = raw;
      if (raw !== undefined && isJson) {
        try {
          data = JSON.parse(raw);
        } catch {
          data = undefined;
        }
      }
      throw new TransportHttpError(response, data);
    }
    return response;
  }

  /**
   * Read a `/prove` success body under a byte cap and a body deadline, returning the base64 proof
   * string — or `undefined` if the body is over-cap, stalls, is not JSON, or lacks a string `proof`
   * (F-11, audit 2026-07-31-9c4cb0c).
   *
   * Before this, the caller read the body with a bare `res.json()`: `PROVE_TIMEOUT_MS` bounds
   * time-to-headers, so an endpoint answering `200` and then streaming forever
   * had no bound at all. The witness has already left the machine by this point, so the exposure here
   * is availability — but it is the dApp's tab that dies, and the fallback path never runs because
   * the promise simply never settles.
   *
   * Deliberately the SAME reader as `/health`, with a different policy — see {@link readJsonBounded}.
   */
  async readProveBody(
    response: Response,
    // Overridable ONLY so the deadline path can be tested in milliseconds; a test that proves
    // settlement by actually waiting out the 60 s production deadline costs a minute of CI per run.
    maxBytes: number = PROVE_BODY_MAX_BYTES,
    timeoutMs: number = PROVE_BODY_TIMEOUT_MS,
  ): Promise<string | undefined> {
    const body = await readJsonBounded(response, maxBytes, timeoutMs);
    if (typeof body !== "object" || body === null) return undefined;
    const proof = (body as { proof?: unknown }).proof;
    if (typeof proof !== "string") return undefined;
    // No separate decode cap: the base64 string is a substring of a body already bounded by
    // `maxBytes`, so it cannot exceed it, and `Buffer.from` allocates ~0.75x of that. A second
    // constant at the same value only looked like defence in depth (post-impl codex flagged it as
    // dead policy) — the real bound is the body cap above.
    return proof;
  }
}
