import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import type { AcceleratorStatus } from "./accelerator-prover.js";
import { AcceleratorTransport } from "./accelerator-transport.js";

const offlineStatus: AcceleratorStatus = { available: false, reason: "offline" };

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
    let realNow: typeof Date.now;
    beforeEach(() => {
      realNow = Date.now;
    });
    afterEach(() => {
      Date.now = realNow;
    });

    test("returns a cached status within the TTL, null once it expires", () => {
      const t = new AcceleratorTransport("127.0.0.1", 59833, 59834);
      expect(t.getFreshCachedStatus()).toBeNull(); // nothing cached yet

      expect(t.cacheStatus(offlineStatus)).toEqual(offlineStatus); // returns what it stored
      expect(t.getFreshCachedStatus()).toEqual(offlineStatus); // fresh hit

      // Advance past the 10s TTL → stale → re-probe required
      Date.now = () => realNow() + 11_000;
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
});
