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
      // 127.0.0.2 rather than the 0.0.0.0 this used to pass: still a DIFFERENT host (which is all
      // the test needs) but a real loopback one, since `assertLoopbackHost` now rejects the rest.
      t.configure({ port: 12345, host: "127.0.0.2" });
      // protocol reset → back to http, now pointing at the new host+port
      expect(t.baseUrl).toBe("http://127.0.0.2:12345");
    });
  });

  // ── F-01 regression (audit 2026-07-31) ──────────────────────────────────────────────────────
  // `host` was interpolated raw into six URL templates, one of which POSTs the private witness.
  describe("host validation", () => {
    test("a host that re-points the URL template is rejected", () => {
      // Each of these silently redirects `https://${host}:${port}/prove` off the machine.
      for (const evil of [
        "evil.com",
        "evil.com/#",
        "evil.com:1/",
        "127.0.0.1.evil.com",
        "127.0.0.1/../../evil",
        "user:pass@evil.com",
        "127.0.0.1:1234",
        "127.0.0.1?x=1",
        "",
        " ",
      ]) {
        expect(() => new AcceleratorTransport(evil, 59833, 59834)).toThrow(
          /Invalid accelerator host/,
        );
      }
    });

    test("loopback spellings are accepted and normalised for the URL template", () => {
      expect(new AcceleratorTransport("127.0.0.1", 1, 2).baseUrl).toBe("http://127.0.0.1:1");
      expect(new AcceleratorTransport("localhost", 1, 2).baseUrl).toBe("http://localhost:1");
      expect(new AcceleratorTransport("127.0.0.2", 1, 2).baseUrl).toBe("http://127.0.0.2:1");
      // A bare IPv6 literal MUST come back bracketed, or the template builds `http://::1:1`.
      expect(new AcceleratorTransport("::1", 1, 2).baseUrl).toBe("http://[::1]:1");
      expect(new AcceleratorTransport("[::1]", 1, 2).baseUrl).toBe("http://[::1]:1");
    });

    test("configure() validates too — the endpoint can be changed after construction", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      expect(() => t.configure({ host: "evil.com" })).toThrow(/Invalid accelerator host/);
      expect(t.baseUrl).toBe("http://127.0.0.1:59833");
    });
  });

  // ── F-01 round 2 (codex pass over the fix) ──────────────────────────────────────────────────
  // Validating `host` closed only HALF the authority. `port`/`httpsPort` are typed `number`, but
  // types are erased at runtime, so a JS caller — or a config from JSON, a URL param, an env var —
  // passes a string straight into the template.
  describe("port validation", () => {
    test("a port that re-points the authority is rejected", () => {
      // `http://127.0.0.1:80@evil.com/prove` has authority `evil.com`: `127.0.0.1` is the username
      // and `80` the password. The witness leaves the machine without touching the host check.
      const evil = "80@evil.com" as unknown as number;
      expect(() => new AcceleratorTransport("127.0.0.1", evil, 59834)).toThrow(
        /Invalid accelerator port/,
      );
      expect(() => new AcceleratorTransport("127.0.0.1", 59833, evil)).toThrow(
        /Invalid accelerator httpsPort/,
      );
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      expect(() => t.configure({ port: evil })).toThrow(/Invalid accelerator port/);
      expect(() => t.configure({ httpsPort: evil })).toThrow(/Invalid accelerator httpsPort/);
      // …and the transport is untouched by the rejected call.
      expect(t.baseUrl).toBe("http://127.0.0.1:59833");
    });

    test("out-of-range and non-integer ports are rejected", () => {
      for (const bad of [0, -1, 65536, 1.5, Number.NaN, "59833" as unknown as number]) {
        expect(() => new AcceleratorTransport("127.0.0.1", bad, 59834)).toThrow(
          /Invalid accelerator port/,
        );
      }
      expect(new AcceleratorTransport("127.0.0.1", 1, 65535).baseUrl).toBe("http://127.0.0.1:1");
    });
  });

  // ── F-01 round 4: the policy flags are erased at runtime too ────────────────────────────────
  describe("policy flag validation", () => {
    test("a non-boolean flag is rejected rather than coerced", () => {
      // codex round 4: I applied the runtime-erasure argument to the NUMBERS and not to the two
      // booleans that ARE the transport's security policy. `"false"` is truthy, so the documented
      // opt-out switched ON via a value that reads as OFF.
      for (const bad of ["false", "true", 0, 1, null]) {
        expect(
          () =>
            new AcceleratorTransport("127.0.0.1", 59833, 59834, false, bad as unknown as boolean),
        ).toThrow(/Invalid accelerator allowInsecureDowngrade/);
        expect(
          () => new AcceleratorTransport("127.0.0.1", 59833, 59834, bad as unknown as boolean),
        ).toThrow(/Invalid accelerator httpsOnly/);
      }
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      expect(() => t.configure({ allowInsecureDowngrade: "false" as unknown as boolean })).toThrow(
        /Invalid accelerator allowInsecureDowngrade/,
      );
      // The real thing still works.
      t.configure({ allowInsecureDowngrade: true, httpsOnly: false });
      t.commitStatus({ available: true, needsDownload: false }, { pin: "set", protocol: "https" });
      expect(t.allowsHttpDowngrade).toBe(true);
    });

    test("a rejected flag does not enable the downgrade", () => {
      // The end-to-end consequence: pre-fix, this call switched the opt-out ON.
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.commitStatus({ available: true, needsDownload: false }, { pin: "set", protocol: "https" });
      expect(() =>
        t.configure({ allowInsecureDowngrade: "false" as unknown as boolean }),
      ).toThrow();
      expect(t.allowsHttpDowngrade).toBe(false);
    });
  });

  // ── F-01: do not DOWNGRADE from a working HTTPS endpoint ────────────────────────────────────
  describe("allowsHttpDowngrade", () => {
    const healthy = { pin: "set", protocol: "https" } as const;

    test("permitted until a healthy HTTPS endpoint has answered", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      expect(t.allowsHttpDowngrade).toBe(true);
      // An accelerator with HTTPS off (the wizard choice, and the headless server) is untouched:
      // nothing ever pins https, so HTTP keeps working exactly as before.
      t.commitStatus({ available: false, reason: "offline" }, { pin: "clear" });
      expect(t.allowsHttpDowngrade).toBe(true);
    });

    test("refused once HTTPS has proven healthy at this endpoint", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.commitStatus({ available: true, needsDownload: false }, healthy);
      expect(t.allowsHttpDowngrade).toBe(false);
    });

    test("the documented opt-out restores it", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834, false, true);
      t.commitStatus({ available: true, needsDownload: false }, healthy);
      expect(t.allowsHttpDowngrade).toBe(true);
    });

    test("reconfiguring the ADDRESS forgets the old endpoint's HTTPS history", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.commitStatus({ available: true, needsDownload: false }, healthy);
      expect(t.allowsHttpDowngrade).toBe(false);
      t.configure({ port: 12345 });
      expect(t.allowsHttpDowngrade).toBe(true);
    });

    test("but flipping a policy flag does NOT", () => {
      // codex: any `setAcceleratorConfig` used to clear the history, so a dApp that toggles an
      // unrelated flag handed back the plaintext downgrade for an endpoint it is still talking to.
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.commitStatus({ available: true, needsDownload: false }, healthy);
      t.configure({ httpsOnly: false });
      expect(t.allowsHttpDowngrade).toBe(false);
      // Re-setting the SAME address is not a move either.
      t.configure({ port: 59833, host: "127.0.0.1" });
      expect(t.allowsHttpDowngrade).toBe(false);
    });

    test("a canonical re-spelling of the SAME host is not a move", () => {
      // codex round 2: `movedEndpoint` compared the RAW input against the NORMALISED stored host, so
      // `configure({host: "127.1"})` after healthy HTTPS looked like a move, wiped the history, and
      // handed the plaintext downgrade back for an endpoint that had not moved at all.
      // NOT "LOCALHOST": that normalises to "localhost", a genuinely different authority from
      // "127.0.0.1" (it can resolve to ::1), so treating it as a move is correct — codex listed it as
      // an equivalent trigger, but it is not one.
      for (const spelling of ["127.1", "0x7f.0.0.1", "2130706433", "127.000.000.001"]) {
        const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
        t.commitStatus({ available: true, needsDownload: false }, healthy);
        t.configure({ host: spelling });
        expect(t.allowsHttpDowngrade).toBe(false);
        // Normalised back to the same address — and still HTTPS, since the history survived.
        expect(t.baseUrl).toBe("https://127.0.0.1:59834");
      }
      // `::1` and `[::1]` are likewise the same endpoint.
      const six = new AcceleratorTransport("::1", 59833, 59834);
      six.commitStatus({ available: true, needsDownload: false }, healthy);
      six.configure({ host: "[::1]" });
      expect(six.allowsHttpDowngrade).toBe(false);

      // The hostname form re-spelled is the SAME authority — DNS names are case-insensitive, and
      // `URL` lower-cases them. Untested until codex round 4 pointed out the omission.
      const named = new AcceleratorTransport("localhost", 59833, 59834);
      named.commitStatus({ available: true, needsDownload: false }, healthy);
      named.configure({ host: "LOCALHOST" });
      expect(named.allowsHttpDowngrade).toBe(false);
      expect(named.baseUrl).toBe("https://localhost:59834");

      // A different loopback AUTHORITY — including the hostname form — still counts as a move.
      for (const other of ["127.0.0.2", "localhost", "[::1]"]) {
        const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
        t.commitStatus({ available: true, needsDownload: false }, healthy);
        t.configure({ host: other });
        expect(t.allowsHttpDowngrade).toBe(true);
      }

      const moved = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      moved.commitStatus({ available: true, needsDownload: false }, healthy);
      moved.configure({ host: "127.0.0.2" });
      expect(moved.allowsHttpDowngrade).toBe(true);
    });

    test("a REJECTED configure() changes nothing at all", () => {
      // codex round 2: fields were assigned as validation proceeded, so a call that threw on `host`
      // had already moved the port — and the throw skipped the cache/protocol/generation resets, so
      // a stale "healthy" status stayed valid for an endpoint that had silently moved.
      // Observe the HTTP baseUrl — it is the only thing that exposes `#port`, which is the field the
      // old code moved before throwing. (The HTTPS branch reads `#httpsPort`, so asserting on it
      // would have let the regression through: the first version of this test did exactly that.)
      const plain = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      expect(() => plain.configure({ port: 51337, host: "evil.com" })).toThrow(
        /Invalid accelerator host/,
      );
      expect(plain.baseUrl).toBe("http://127.0.0.1:59833");

      // …and a bad httpsPort arriving after a good host must not move the host either.
      expect(() =>
        plain.configure({ host: "127.0.0.2", httpsPort: "443@evil.com" as unknown as number }),
      ).toThrow(/Invalid accelerator httpsPort/);
      expect(plain.baseUrl).toBe("http://127.0.0.1:59833");

      // `httpsPort` needs its own observable — the HTTP baseUrl above reads `#port`, so a mutant
      // that committed a VALID new httpsPort before rejecting the host would have passed (codex
      // round 4). Pin the protocol to read the HTTPS side.
      const https = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      https.setProtocol("https");
      expect(() => https.configure({ httpsPort: 44444, host: "evil.com" })).toThrow(
        /Invalid accelerator host/,
      );
      expect(https.baseUrl).toBe("https://127.0.0.1:59834");

      // A policy flag rejected AFTER valid addresses must not move them either.
      const flags = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      expect(() =>
        flags.configure({ port: 51337, httpsOnly: "yes" as unknown as boolean }),
      ).toThrow(/Invalid accelerator httpsOnly/);
      expect(flags.baseUrl).toBe("http://127.0.0.1:59833");

      // Same when the property THROWS on read rather than failing validation.
      const getter = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      expect(() =>
        getter.configure({
          port: 51337,
          get httpsOnly(): boolean {
            throw new Error("boom");
          },
        }),
      ).toThrow(/boom/);
      expect(getter.baseUrl).toBe("http://127.0.0.1:59833");

      // The rest of the state must be untouched too: a throw used to skip the resets below the
      // assignments, leaving a stale "healthy" cache valid for an endpoint that had already moved.
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.commitStatus({ available: true, needsDownload: false }, healthy);
      const genBefore = t.generation;

      expect(() => t.configure({ port: 51337, host: "evil.com" })).toThrow(
        /Invalid accelerator host/,
      );

      expect(t.generation).toBe(genBefore);
      expect(t.getFreshCachedStatus()).not.toBeNull();
      expect(t.allowsHttpDowngrade).toBe(false);
    });

    test("a later probe cannot re-pin plaintext once HTTPS has worked", async () => {
      // codex CRITICAL: `allowsHttpDowngrade` was consulted only when an HTTPS `/prove` FAILED, so
      // the plaintext path was reachable by going AROUND it — let the status cache expire, take the
      // HTTP port while HTTPS is unavailable, and the next dual probe just pins HTTP. The fix is to
      // stop constructing an http:// URL at PROBE time, so this now reports offline (→ WASM).
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      t.commitStatus({ available: true, needsDownload: false }, healthy);

      const seen: string[] = [];
      const realFetch = globalThis.fetch;
      // ky passes a Request, so read `.url` (matching the other helpers here) — `.toString()` on a
      // Request yields "[object Request]" and would silently match nothing.
      globalThis.fetch = ((input: any) => {
        const url: string = typeof input === "string" ? input : input.url;
        seen.push(url);
        // HTTPS is gone; a squatter answers the plaintext port perfectly.
        if (url.startsWith("https://")) return Promise.reject(new TypeError("connection refused"));
        return Promise.resolve(Response.json({ status: "ok", api_version: 1 }));
      }) as typeof fetch;
      try {
        await expect(t.probeHealth()).rejects.toThrow();
      } finally {
        globalThis.fetch = realFetch;
      }
      expect(seen.some((u) => u.startsWith("http://"))).toBe(false);
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

    test("empty chunks past the deadline followed by close are NOT accepted as healthy", async () => {
      // codex Medium (round 3): the `done` branch was evaluated BEFORE the in-loop clock check, so a
      // stream that emitted valid JSON, spammed empty chunks past the deadline, and only THEN closed
      // reached `done` first, cleared the still-unfired timer, and parsed as healthy. (Their repro
      // also accumulated ~936MB by retaining the empty chunks — now they are never pushed.)
      globalThis.fetch = mock(async () => {
        let emitted = 0;
        const stream = new ReadableStream<Uint8Array>({
          pull(controller) {
            if (emitted === 0) {
              controller.enqueue(new TextEncoder().encode(JSON.stringify(HEALTHY)));
              emitted++;
              return;
            }
            // Spam empties; close only well after the 2s body deadline.
            if (emitted++ < 5_000) {
              controller.enqueue(new Uint8Array(0));
              return;
            }
            controller.close();
          },
        });
        return new Response(stream, { status: 200 });
      }) as any;

      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834, true);
      const { body } = await t.probeHealth();
      expect(body).toBeUndefined();
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
