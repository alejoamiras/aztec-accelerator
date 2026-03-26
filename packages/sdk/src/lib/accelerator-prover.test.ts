import { afterEach, beforeEach, describe, expect, mock, spyOn, test } from "bun:test";
import { BBLazyPrivateKernelProver } from "@aztec/bb-prover/client/lazy";
import { WASMSimulator } from "@aztec/simulator/client";
import * as stdlibKernel from "@aztec/stdlib/kernel";
import sdkPkg from "../../package.json" with { type: "json" };
import { AcceleratorProver } from "./accelerator-prover.js";

const SDK_AZTEC_VERSION = (sdkPkg.dependencies as Record<string, string>)["@aztec/stdlib"];

// --- Test helpers ---

const fakeStep = {
  functionName: "test_fn",
  witness: new Map([[0, "val"]]),
  bytecode: new Uint8Array([0, 1]),
  vk: new Uint8Array([2, 3]),
  timings: { witgen: 10 },
} as any;

type RouteHandler = (url: string, request: Request | string) => Response | Promise<Response>;

function mockFetch(routes: Record<string, RouteHandler> = {}): { fetchedUrls: string[] } {
  const fetchedUrls: string[] = [];

  globalThis.fetch = mock(async (input: any, _init?: any) => {
    const url: string = typeof input === "string" ? input : input.url;
    fetchedUrls.push(url);

    for (const [pattern, handler] of Object.entries(routes)) {
      if (url.includes(pattern)) {
        return handler(url, input);
      }
    }
    return new Response("not found", { status: 404 });
  }) as any;

  return { fetchedUrls };
}

function mockFetchOffline() {
  globalThis.fetch = mock(async () => {
    throw new TypeError("fetch failed (connection refused)");
  }) as any;
}

function mockWasmProver() {
  const spy = spyOn(BBLazyPrivateKernelProver.prototype, "createChonkProof");
  spy.mockRejectedValue(new Error("local prover not available in test"));
  return spy;
}

function mockSerializer() {
  return spyOn(stdlibKernel, "serializePrivateExecutionSteps").mockReturnValue(
    Buffer.from([0xde, 0xad]),
  );
}

// --- Tests ---

describe("AcceleratorProver", () => {
  let originalFetch: typeof globalThis.fetch;

  beforeEach(() => {
    originalFetch = globalThis.fetch;
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
  });

  describe("Proving", () => {
    test("falls back to WASM when accelerator is unavailable", async () => {
      mockFetchOffline();
      const wasmSpy = mockWasmProver();

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });

      await expect(prover.createChonkProof([fakeStep])).rejects.toThrow(
        "local prover not available in test",
      );
      expect(wasmSpy).toHaveBeenCalled();
      wasmSpy.mockRestore();
    });

    test("falls back to WASM with legacy accelerator on version mismatch", async () => {
      mockFetch({
        "/health": () => Response.json({ status: "ok", aztec_version: "0.0.0-fake" }),
      });
      const wasmSpy = mockWasmProver();

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });

      await expect(prover.createChonkProof([fakeStep])).rejects.toThrow(
        "local prover not available in test",
      );
      expect(wasmSpy).toHaveBeenCalled();
      wasmSpy.mockRestore();
    });

    test("emits downloading phase when accelerator needs bb download", async () => {
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            aztec_version: "5.0.0-nightly.20260101",
            available_versions: ["5.0.0-nightly.20260101"],
          }),
      });
      const serializeSpy = mockSerializer();
      const phases: string[] = [];

      const prover = new AcceleratorProver({
        simulator: new WASMSimulator(),
        onPhase: (phase) => phases.push(phase),
      });

      try {
        await prover.createChonkProof([fakeStep]);
      } catch {
        // Expected — mock /prove returns 404
      }

      expect(phases).toContain("downloading");
      serializeSpy.mockRestore();
    });

    test("sends x-aztec-version header on /prove requests", async () => {
      let capturedHeaders: Headers | null = null;
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          }),
        "/prove": (_url, request) => {
          if (request instanceof Request) {
            capturedHeaders = request.headers;
          }
          return Response.json({ proof: "" });
        },
      });
      const serializeSpy = mockSerializer();

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });

      try {
        await prover.createChonkProof([fakeStep]);
      } catch {
        // May fail on proof deserialization — that's fine, we're testing the header
      }

      expect(capturedHeaders).not.toBeNull();
      expect(capturedHeaders!.get("x-aztec-version")).toBe(SDK_AZTEC_VERSION);
      serializeSpy.mockRestore();
    });

    test("falls back to WASM with denied phase on 403 (origin not authorized)", async () => {
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          }),
        "/prove": () =>
          Response.json({ error: "origin_denied", message: "Access denied" }, { status: 403 }),
      });
      const serializeSpy = mockSerializer();
      const wasmSpy = mockWasmProver();
      const phases: string[] = [];

      const prover = new AcceleratorProver({
        simulator: new WASMSimulator(),
        onPhase: (phase) => phases.push(phase),
      });

      await expect(prover.createChonkProof([fakeStep])).rejects.toThrow(
        "local prover not available in test",
      );

      // Should emit: detect → serialize → transmit → proving → denied → fallback → proving
      expect(phases).toContain("denied");
      expect(phases).toContain("fallback");
      expect(wasmSpy).toHaveBeenCalled();
      wasmSpy.mockRestore();
      serializeSpy.mockRestore();
    });

    test("multi-version accelerator always proceeds (no WASM fallback on version mismatch)", async () => {
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            aztec_version: "5.0.0-nightly.20260101",
            available_versions: ["5.0.0-nightly.20260101"],
          }),
      });
      const serializeSpy = mockSerializer();

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });

      try {
        await prover.createChonkProof([fakeStep]);
      } catch {
        // Expected — mock /prove returns 404
      }

      expect(serializeSpy).toHaveBeenCalled();
      serializeSpy.mockRestore();
    });
  });

  describe("checkAcceleratorStatus", () => {
    test("returns available + version info when healthy (multi-version)", async () => {
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION, "5.0.0-nightly.20260101"],
          }),
      });

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const status = await prover.checkAcceleratorStatus();

      expect(status.available).toBe(true);
      expect(status.needsDownload).toBe(false);
      expect(status.acceleratorVersion).toBe(SDK_AZTEC_VERSION);
      expect(status.availableVersions).toEqual([SDK_AZTEC_VERSION, "5.0.0-nightly.20260101"]);
      expect(status.sdkAztecVersion).toBe(SDK_AZTEC_VERSION);
      expect(status.protocol).toBeDefined();
    });

    test("returns needsDownload when SDK version not in available_versions", async () => {
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            aztec_version: "5.0.0-nightly.20260101",
            available_versions: ["5.0.0-nightly.20260101"],
          }),
      });

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const status = await prover.checkAcceleratorStatus();

      expect(status.available).toBe(true);
      expect(status.needsDownload).toBe(true);
    });

    test("returns available: false when fetch fails (connection refused)", async () => {
      mockFetchOffline();

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const status = await prover.checkAcceleratorStatus();

      expect(status.available).toBe(false);
      expect(status.sdkAztecVersion).toBe(SDK_AZTEC_VERSION);
      expect(status.protocol).toBeUndefined();
    });

    test("returns available: false on non-ok health response", async () => {
      mockFetch({
        "/health": () => new Response("Internal Server Error", { status: 500 }),
      });

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const status = await prover.checkAcceleratorStatus();

      expect(status.available).toBe(false);
      expect(status.sdkAztecVersion).toBe(SDK_AZTEC_VERSION);
    });

    test("does not cache protocol on non-ok health response", async () => {
      // First check: accelerator returns 500 — protocol should NOT be cached
      mockFetch({
        "/health": () => new Response("Internal Server Error", { status: 500 }),
      });

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const status1 = await prover.checkAcceleratorStatus();
      expect(status1.available).toBe(false);

      // Second check: accelerator is healthy — should re-probe and find it
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          }),
      });
      const status2 = await prover.checkAcceleratorStatus();
      expect(status2.available).toBe(true);
      expect(status2.protocol).toBeDefined();
    });

    test("returns available: false on legacy version mismatch", async () => {
      mockFetch({
        "/health": () => Response.json({ status: "ok", aztec_version: "0.0.0-fake" }),
      });

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const status = await prover.checkAcceleratorStatus();

      expect(status.available).toBe(false);
      expect(status.acceleratorVersion).toBe("0.0.0-fake");
    });

    test("falls back to HTTPS when HTTP fails (Safari mixed-content)", async () => {
      // Simulate: HTTP fetch throws (mixed-content block), HTTPS succeeds
      globalThis.fetch = mock(async (input: any) => {
        const url: string = typeof input === "string" ? input : input.url;
        if (url.startsWith("http://")) {
          throw new TypeError("fetch failed (mixed content)");
        }
        if (url.includes("/health")) {
          return Response.json({
            status: "ok",
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          });
        }
        return new Response("not found", { status: 404 });
      }) as any;

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const status = await prover.checkAcceleratorStatus();

      expect(status.available).toBe(true);
      expect(status.protocol).toBe("https");
    });

    test("returns unavailable when both HTTP and HTTPS fail", async () => {
      mockFetchOffline();

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const status = await prover.checkAcceleratorStatus();

      expect(status.available).toBe(false);
      expect(status.protocol).toBeUndefined();
    });

    test("detected protocol is used for subsequent /prove calls", async () => {
      const { fetchedUrls } = mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          }),
      });
      const serializeSpy = mockSerializer();

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });

      try {
        await prover.createChonkProof([fakeStep]);
      } catch {
        // Expected — mock /prove returns 404
      }

      // The /prove request should use whichever protocol the health check used
      const proveUrls = fetchedUrls.filter((u) => u.includes("/prove"));
      expect(proveUrls.length).toBe(1);
      // Protocol matches whichever responded first (in test, both succeed via mockFetch, so HTTP wins)
      expect(proveUrls[0]).toMatch(/^https?:\/\/127\.0\.0\.1:\d+\/prove$/);
      serializeSpy.mockRestore();
    });

    test("protocol resets after setAcceleratorConfig()", async () => {
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          }),
      });

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });

      // First check caches the protocol
      const status1 = await prover.checkAcceleratorStatus();
      expect(status1.protocol).toBeDefined();

      // Reset config clears cached protocol
      prover.setAcceleratorConfig({ port: 12345 });

      // Next check re-probes both protocols
      const status2 = await prover.checkAcceleratorStatus();
      expect(status2.protocol).toBeDefined();
    });
  });

  describe("Constructor", () => {
    test("defaults work with zero-config constructor", async () => {
      mockFetchOffline();
      const wasmSpy = mockWasmProver();

      const prover = new AcceleratorProver();

      await expect(prover.createChonkProof([fakeStep])).rejects.toThrow(
        "local prover not available in test",
      );
      // accelerated mode falls back to WASM when offline
      expect(wasmSpy).toHaveBeenCalled();
      wasmSpy.mockRestore();
    });

    test("invalid env port falls back to default", async () => {
      const { fetchedUrls } = mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          }),
      });

      const originalPort = process.env.AZTEC_ACCELERATOR_PORT;
      process.env.AZTEC_ACCELERATOR_PORT = "not-a-number";

      try {
        const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
        await prover.checkAcceleratorStatus();

        // Should use default port 59833, not NaN
        const healthUrls = fetchedUrls.filter((u) => u.includes("/health"));
        expect(healthUrls.some((u) => u.includes(":59833"))).toBe(true);
        expect(healthUrls.every((u) => !u.includes("NaN"))).toBe(true);
      } finally {
        if (originalPort === undefined) {
          delete process.env.AZTEC_ACCELERATOR_PORT;
        } else {
          process.env.AZTEC_ACCELERATOR_PORT = originalPort;
        }
      }
    });

    test("env vars override default ports", async () => {
      const { fetchedUrls } = mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          }),
      });

      const originalPort = process.env.AZTEC_ACCELERATOR_PORT;
      process.env.AZTEC_ACCELERATOR_PORT = "51337";

      try {
        const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
        await prover.checkAcceleratorStatus();

        const healthUrls = fetchedUrls.filter((u) => u.includes("/health"));
        expect(healthUrls.some((u) => u.includes(":51337"))).toBe(true);
      } finally {
        if (originalPort === undefined) {
          delete process.env.AZTEC_ACCELERATOR_PORT;
        } else {
          process.env.AZTEC_ACCELERATOR_PORT = originalPort;
        }
      }
    });

    test("phase callbacks fire in correct order", async () => {
      mockFetchOffline();
      const wasmSpy = mockWasmProver();
      const phases: string[] = [];

      const prover = new AcceleratorProver({
        simulator: new WASMSimulator(),
        onPhase: (phase) => phases.push(phase),
      });

      try {
        await prover.createChonkProof([fakeStep]);
      } catch {
        // Expected — WASM mock throws
      }

      // Offline → detect → fallback → proving → (throws before proved/receive)
      expect(phases[0]).toBe("detect");
      expect(phases[1]).toBe("fallback");
      expect(phases[2]).toBe("proving");
      wasmSpy.mockRestore();
    });
  });
});
