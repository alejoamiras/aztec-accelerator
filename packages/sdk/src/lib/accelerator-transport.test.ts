import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { AcceleratorTransport, isRecognizedHealthBody } from "./accelerator-transport.js";
import type { AcceleratorStatus } from "./types.js";

const offlineStatus: AcceleratorStatus = { available: false, reason: "offline" };
/** The minimal body the real accelerator always serves (server.rs — both minimal + detailed). */
const HEALTHY = { status: "ok", api_version: 1 };

describe("AcceleratorTransport", () => {
  describe("baseUrl / protocol negotiation", () => {
    test("defaults to http://host:port before any protocol is negotiated", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      expect(t.baseUrl).toBe("http://127.0.0.1:59833");
    });

    test("switches to https://host:httpsPort once https is pinned", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.setProtocol("https");
      expect(t.baseUrl).toBe("https://127.0.0.1:59834");
    });

    test("setProtocol('http') and setProtocol(null) both resolve to the http endpoint", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.setProtocol("https");
      t.setProtocol(null);
      expect(t.baseUrl).toBe("http://127.0.0.1:59833");
      t.setProtocol("http");
      expect(t.baseUrl).toBe("http://127.0.0.1:59833");
    });

    test("configure() updates the endpoint AND resets the negotiated protocol", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.setProtocol("https");
      t.configure({ port: 12345, host: "0.0.0.0" });
      // protocol reset → back to http, now pointing at the new host+port
      expect(t.baseUrl).toBe("http://0.0.0.0:12345");
    });
  });

  describe("status cache", () => {
    // The cache clock is performance.now() (monotonic — a wall-clock step backwards must not
    // extend the TTL), so the TTL tests patch performance.now, not Date.now.
    let realNow: typeof performance.now;
    beforeEach(() => {
      realNow = performance.now.bind(performance);
    });
    afterEach(() => {
      performance.now = realNow;
    });

    test("returns a cached status within the TTL, null once it expires", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      expect(t.getFreshCachedStatus()).toBeNull(); // nothing cached yet

      expect(t.cacheStatus(offlineStatus)).toEqual(offlineStatus); // returns what it stored
      expect(t.getFreshCachedStatus()).toEqual(offlineStatus); // fresh hit

      // Advance past the 10s TTL → stale → re-probe required
      performance.now = () => realNow() + 11_000;
      expect(t.getFreshCachedStatus()).toBeNull();
    });

    test("configure() clears the status cache (endpoint changed)", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.cacheStatus(offlineStatus);
      expect(t.getFreshCachedStatus()).toEqual(offlineStatus);
      t.configure({ port: 12345 });
      expect(t.getFreshCachedStatus()).toBeNull();
    });
  });

  // q7e3-F-06: pin the three-way set/clear/keep transition so a refactor can't flatten it. The
  // audit's concern: a "derive pin from the status discriminant" rewrite would unify the two
  // error exits — but `!response.ok` must KEEP an existing pin while malformed-JSON must CLEAR it.
  describe("commitStatus protocol-pin transitions", () => {
    const okStatus: AcceleratorStatus = {
      available: true,
      needsDownload: false,
      protocol: "https",
    };
    const errStatus: AcceleratorStatus = { available: false, reason: "error", protocol: "https" };

    test('"set" pins the winning protocol (drives /prove)', () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.commitStatus(okStatus, { pin: "set", protocol: "https" });
      expect(t.baseUrl).toBe("https://127.0.0.1:59834");
    });

    test('"keep" leaves an EXISTING pin untouched (the !response.ok case)', () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.setProtocol("https"); // a prior OK probe pinned https
      t.commitStatus(errStatus, { pin: "keep" });
      expect(t.baseUrl).toBe("https://127.0.0.1:59834"); // still https — NOT cleared, NOT repinned
    });

    test('"clear" unpins (the malformed-JSON / offline case)', () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.setProtocol("https"); // a prior OK probe pinned https
      t.commitStatus(errStatus, { pin: "clear" });
      expect(t.baseUrl).toBe("http://127.0.0.1:59833"); // back to the http default
    });

    test("caches the status it commits", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      expect(t.commitStatus(offlineStatus, { pin: "clear" })).toEqual(offlineStatus);
      expect(t.getFreshCachedStatus()).toEqual(offlineStatus);
    });
  });

  describe("probeHealth", () => {
    let originalFetch: typeof globalThis.fetch;
    beforeEach(() => {
      originalFetch = globalThis.fetch;
    });
    afterEach(() => {
      globalThis.fetch = originalFetch;
    });

    test("resolves with the protocol whose probe wins (http when both answer)", async () => {
      globalThis.fetch = mock(async (input: any) => {
        const url: string = typeof input === "string" ? input : input.url;
        // Make HTTPS deterministically slower so HTTP wins the race.
        if (url.startsWith("https://")) await new Promise((r) => setTimeout(r, 20));
        return new Response("ok", { status: 200 });
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      const { response, protocol } = await t.probeHealth();
      expect(response.ok).toBe(true);
      expect(protocol).toBe("http");
    });

    test("falls back to https when http rejects (Safari mixed-content)", async () => {
      globalThis.fetch = mock(async (input: any) => {
        const url: string = typeof input === "string" ? input : input.url;
        if (url.startsWith("http://")) throw new TypeError("blocked (mixed content)");
        return new Response("ok", { status: 200 });
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      const { protocol } = await t.probeHealth();
      expect(protocol).toBe("https");
    });

    test("resolves a non-2xx response instead of throwing (caller maps it to 'error')", async () => {
      globalThis.fetch = mock(async () => new Response("nope", { status: 500 })) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      const { response } = await t.probeHealth();
      // throwHttpErrors:false → a 500 still resolves (not thrown); response.ok is false.
      expect(response.ok).toBe(false);
      expect(response.status).toBe(500);
    });

    test("rejects when both protocols fail twice (offline)", async () => {
      globalThis.fetch = mock(async () => {
        throw new TypeError("connection refused");
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      await expect(t.probeHealth()).rejects.toBeDefined();
    });
  });

  // Phase 2 (audit R2 / H-2): HTTPS is preferred ONLY when it's healthy (2xx + parseable JSON),
  // with a bounded grace so the common no-HTTPS path adds no latency.
  describe("probeHealth — prefer-HTTPS-when-healthy", () => {
    let originalFetch: typeof globalThis.fetch;
    beforeEach(() => {
      originalFetch = globalThis.fetch;
    });
    afterEach(() => {
      globalThis.fetch = originalFetch;
    });

    const json = (obj: unknown, status = 200) => new Response(JSON.stringify(obj), { status });

    test("healthy HTTPS wins even when HTTP answers first", async () => {
      globalThis.fetch = mock(async (input: any) => {
        const url: string = typeof input === "string" ? input : input.url;
        // HTTP answers immediately; HTTPS answers a bit later but well within the grace.
        if (url.startsWith("https://")) await new Promise((r) => setTimeout(r, 15));
        return json(HEALTHY);
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      const { protocol } = await t.probeHealth();
      expect(protocol).toBe("https");
    });

    test("no added latency when HTTPS is absent (refused resolves fast → HTTP wins)", async () => {
      globalThis.fetch = mock(async (input: any) => {
        const url: string = typeof input === "string" ? input : input.url;
        if (url.startsWith("https://")) throw new TypeError("connection refused");
        return json(HEALTHY);
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      const start = performance.now();
      const { protocol } = await t.probeHealth();
      const elapsedMs = performance.now() - start;
      expect(protocol).toBe("http");
      // The 250ms grace must NOT be paid when HTTPS refuses instantly.
      expect(elapsedMs).toBeLessThan(150);
    });

    test("stalled HTTPS + OK HTTP → HTTP wins after the bounded grace (not the full HTTPS timeout)", async () => {
      globalThis.fetch = mock(async (input: any) => {
        const url: string = typeof input === "string" ? input : input.url;
        // HTTPS is bound but stalls far past the grace; HTTP is healthy immediately.
        if (url.startsWith("https://")) await new Promise((r) => setTimeout(r, 1_000));
        return json(HEALTHY);
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      const start = performance.now();
      const { protocol } = await t.probeHealth();
      const elapsedMs = performance.now() - start;
      expect(protocol).toBe("http");
      // Waited ~the grace, NOT the full 1s stall.
      expect(elapsedMs).toBeGreaterThanOrEqual(200);
      expect(elapsedMs).toBeLessThan(600);
    });

    test("HTTPS 500 + healthy HTTP → HTTP wins (a non-OK HTTPS must not beat healthy HTTP)", async () => {
      globalThis.fetch = mock(async (input: any) => {
        const url: string = typeof input === "string" ? input : input.url;
        if (url.startsWith("https://")) return json({ error: "boom" }, 500);
        return json(HEALTHY);
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      const { protocol } = await t.probeHealth();
      expect(protocol).toBe("http");
    });

    test("HTTPS 200-but-malformed + healthy HTTP → HTTP wins (unparseable body isn't healthy)", async () => {
      globalThis.fetch = mock(async (input: any) => {
        const url: string = typeof input === "string" ? input : input.url;
        // A 200 whose body is NOT parseable JSON — reachable via a foreign server on the HTTPS port.
        if (url.startsWith("https://"))
          return new Response("<html>not json</html>", { status: 200 });
        return json(HEALTHY);
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      const { protocol } = await t.probeHealth();
      expect(protocol).toBe("http");
    });

    test("the winning probe carries its parsed body (read once, no clone)", async () => {
      const detailed = { ...HEALTHY, aztec_version: "5.0.0" };
      globalThis.fetch = mock(async () => json(detailed)) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      const { body, protocol } = await t.probeHealth();
      expect(protocol).toBe("https");
      // The transport consumed the stream exactly once (bounded) and hands the caller the parsed
      // body — the caller never re-reads the response.
      expect(body).toEqual(detailed);
    });

    test("HTTPS 200 with foreign-but-valid JSON loses to healthy HTTP (recognized-shape gate)", async () => {
      globalThis.fetch = mock(async (input: any) => {
        const url: string = typeof input === "string" ? input : input.url;
        // Squatter shapes that USED to pass the old field-presence check — none carry the real
        // contract (status:"ok" AND api_version:1), so none may steal the pin.
        if (url.startsWith("https://")) return json({ status: false, api_version: 99 });
        return json(HEALTHY);
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      const { protocol } = await t.probeHealth();
      expect(protocol).toBe("http");
    });

    test("a foreign fast HTTP does NOT beat a healthy-but-slower HTTPS (grace gates on health, not just ok)", async () => {
      globalThis.fetch = mock(async (input: any) => {
        const url: string = typeof input === "string" ? input : input.url;
        // Healthy HTTPS answers at 400ms — PAST the 250ms grace. Foreign HTTP 200 answers instantly.
        if (url.startsWith("https://")) {
          await new Promise((r) => setTimeout(r, 400));
          return json(HEALTHY);
        }
        return json({ hello: "world" }); // 200 but NOT the health contract
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      const { protocol } = await t.probeHealth();
      // The old `httpRes.response.ok` grace shortcut returned the foreign HTTP at ~250ms; now an
      // unhealthy HTTP falls through and the healthy HTTPS wins even though it was slower.
      expect(protocol).toBe("https");
    });

    test("HTTPS 200 that stalls its body forever cannot hang the probe (bounded body read)", async () => {
      globalThis.fetch = mock(async (input: any) => {
        const url: string = typeof input === "string" ? input : input.url;
        if (url.startsWith("https://")) {
          // Headers arrive, the body starts JSON and then never finishes — the old clone().json()
          // hung on this indefinitely.
          const stalled = new ReadableStream<Uint8Array>({
            start(controller) {
              controller.enqueue(new TextEncoder().encode('{"status":"ok",'));
              // never close, never enqueue again
            },
          });
          return new Response(stalled, { status: 200 });
        }
        return json(HEALTHY);
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      const start = performance.now();
      const { protocol } = await t.probeHealth();
      // Healthy HTTP wins after the grace; the stalled HTTPS neither wins nor blocks resolution.
      expect(protocol).toBe("http");
      expect(performance.now() - start).toBeLessThan(1_500);
    });
  });

  describe("isRecognizedHealthBody (mirrors core probe.rs::is_healthy_aztec_response)", () => {
    test("accepts exactly the accelerator contract", () => {
      expect(isRecognizedHealthBody({ status: "ok", api_version: 1 })).toBe(true);
      expect(isRecognizedHealthBody({ status: "ok", api_version: 1, version: "1.2.3" })).toBe(true);
    });

    test("rejects foreign / wrong / malformed shapes", () => {
      expect(isRecognizedHealthBody({ status: "ok", api_version: 2 })).toBe(false);
      expect(isRecognizedHealthBody({ status: "error", api_version: 1 })).toBe(false);
      expect(isRecognizedHealthBody({ api_version: 1 })).toBe(false);
      expect(isRecognizedHealthBody({ status: "ok" })).toBe(false);
      expect(isRecognizedHealthBody({ status: false })).toBe(false);
      expect(isRecognizedHealthBody({ hello: "world" })).toBe(false);
      expect(isRecognizedHealthBody({})).toBe(false);
      expect(isRecognizedHealthBody([])).toBe(false);
      expect(isRecognizedHealthBody("not even an object")).toBe(false);
      expect(isRecognizedHealthBody(null)).toBe(false);
      expect(isRecognizedHealthBody(undefined)).toBe(false);
    });
  });

  describe("configuration generation guard", () => {
    test("a commit from a probe started before configure() is discarded (no pin, no cache)", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      const staleGen = t.generation;
      t.configure({ port: 12345 }); // reconfigured while the probe was in flight
      const status: AcceleratorStatus = {
        available: true,
        needsDownload: false,
        protocol: "https",
      };
      // The stale probe completes and tries to commit: must be a no-op.
      t.commitStatus(status, { pin: "set", protocol: "https" }, staleGen);
      expect(t.baseUrl).toBe("http://127.0.0.1:12345"); // NOT https — stale pin discarded
      expect(t.getFreshCachedStatus()).toBeNull(); // NOT cached
    });

    test("a commit with the current generation applies normally", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      const gen = t.generation;
      const status: AcceleratorStatus = {
        available: true,
        needsDownload: false,
        protocol: "https",
      };
      t.commitStatus(status, { pin: "set", protocol: "https" }, gen);
      expect(t.baseUrl).toBe("https://127.0.0.1:59834");
      expect(t.getFreshCachedStatus()).toEqual(status);
    });
  });

  describe("demoteHttpsPin", () => {
    test("clears an https pin (and the cache) outside strict mode", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.setProtocol("https");
      t.cacheStatus({ available: true, needsDownload: false, protocol: "https" });
      expect(t.demoteHttpsPin()).toBe(true);
      expect(t.baseUrl).toBe("http://127.0.0.1:59833");
      expect(t.getFreshCachedStatus()).toBeNull();
    });

    test("no-op when the pin is not https", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.setProtocol("http");
      expect(t.demoteHttpsPin()).toBe(false);
      expect(t.baseUrl).toBe("http://127.0.0.1:59833");
    });

    test("no-op in strict httpsOnly mode (never demote to a plaintext URL)", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834, true);
      t.setProtocol("https");
      expect(t.demoteHttpsPin()).toBe(false);
      expect(t.baseUrl).toBe("https://127.0.0.1:59834");
    });
  });

  describe("httpsOnly strict mode", () => {
    let originalFetch: typeof globalThis.fetch;
    beforeEach(() => {
      originalFetch = globalThis.fetch;
    });
    afterEach(() => {
      globalThis.fetch = originalFetch;
    });

    test("never constructs an http:// URL and pins https", async () => {
      const urls: string[] = [];
      globalThis.fetch = mock(async (input: any) => {
        const url: string = typeof input === "string" ? input : input.url;
        urls.push(url);
        return new Response(JSON.stringify(HEALTHY), { status: 200 });
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834, true);
      const { protocol } = await t.probeHealth();
      expect(protocol).toBe("https");
      expect(urls.every((u) => u.startsWith("https://"))).toBe(true);
      expect(urls.some((u) => u.startsWith("http://"))).toBe(false);
      // baseUrl for /prove is https even before any pin, and never the http endpoint.
      expect(t.baseUrl).toBe("https://127.0.0.1:59834");
    });

    test("a body that streams COMPLETE valid JSON but never closes is NOT accepted as healthy", async () => {
      // codex Medium: the deadline cancels the reader, read() reports `done` with the fully-buffered
      // (but stream-never-closed) bytes; parsing those as EOF would accept a timed-out body as healthy.
      // The `timedOut` flag must force `undefined`. httpsOnly so this HTTPS body is the deciding probe.
      globalThis.fetch = mock(async () => {
        const stream = new ReadableStream<Uint8Array>({
          start(controller) {
            controller.enqueue(
              new TextEncoder().encode(JSON.stringify({ status: "ok", api_version: 1 })),
            );
            // Full valid JSON emitted — but never close(): the stream stays open past the deadline.
          },
        });
        return new Response(stream, { status: 200 });
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834, true);
      const { body } = await t.probeHealth();
      expect(body).toBeUndefined(); // deadline fired → undefined, NOT the parsed {status:ok}
    }, 10_000);

    test("a stream of endless ZERO-LENGTH chunks cannot starve the deadline", async () => {
      // codex Low (round 2): zero-length chunks resolve every read() immediately, starving the
      // setTimeout so neither the deadline nor the byte cap ever fires. The in-loop wall-clock check
      // must still bail. httpsOnly so this is the deciding probe.
      globalThis.fetch = mock(async () => {
        const stream = new ReadableStream<Uint8Array>({
          pull(controller) {
            controller.enqueue(new Uint8Array(0)); // never any bytes, never closes
          },
        });
        return new Response(stream, { status: 200 });
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834, true);
      const started = performance.now();
      const { body } = await t.probeHealth();
      expect(body).toBeUndefined();
      // Bounded by the body deadline (2s), not hung forever.
      expect(performance.now() - started).toBeLessThan(6_000);
    }, 15_000);

    test("unreachable HTTPS rejects (→ caller maps to offline), never touching http", async () => {
      const urls: string[] = [];
      globalThis.fetch = mock(async (input: any) => {
        const url: string = typeof input === "string" ? input : input.url;
        urls.push(url);
        throw new TypeError("connection refused");
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834, true);
      await expect(t.probeHealth()).rejects.toBeDefined();
      expect(urls.some((u) => u.startsWith("http://"))).toBe(false);
    });
  });
});
