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
        "/health": () =>
          Response.json({ status: "ok", api_version: 1, aztec_version: "0.0.0-fake" }),
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
            api_version: 1,
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
            api_version: 1,
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

    test("emits 'proved' phase even when the server omits x-prove-duration-ms", async () => {
      // Regression: the native success path previously emitted "proved" only when the
      // x-prove-duration-ms header was present, leaving the UI stuck on "proving" for a
      // header-less response. It must always emit "proved" (client-measured fallback).
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            api_version: 1,
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          }),
        "/prove": () => Response.json({ proof: "" }), // no x-prove-duration-ms header
      });
      const serializeSpy = mockSerializer();
      const phases: string[] = [];
      const prover = new AcceleratorProver({
        simulator: new WASMSimulator(),
        onPhase: (p) => phases.push(p),
      });

      try {
        await prover.createChonkProof([fakeStep]);
      } catch {
        // proof deserialization may fail in-test; we only assert the phase sequence
      }

      expect(phases).toContain("proving");
      expect(phases).toContain("proved");
      expect(phases.indexOf("proved")).toBeGreaterThan(phases.indexOf("proving"));
      serializeSpy.mockRestore();
    });

    test("falls back to WASM with denied phase on 403 (origin not authorized)", async () => {
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            api_version: 1,
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
            api_version: 1,
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
            api_version: 1,
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
            api_version: 1,
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

    test("reachable host with malformed JSON → unavailable reason 'error', not 'offline'", async () => {
      // Post-impl audit guard (codex): a 200 with an unparseable body means the host ANSWERED
      // (reachable) — so reason must be "error" (carrying the protocol), not "offline" (which is
      // documented as both probes failing). The available:false → WASM-fallback outcome is unchanged.
      mockFetch({
        "/health": () => new Response("not valid json {{{", { status: 200 }),
      });

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const status = await prover.checkAcceleratorStatus();

      expect(status.available).toBe(false);
      if (status.available) throw new Error("expected unavailable");
      expect(status.reason).toBe("error");
      if (status.reason === "error") {
        expect(status.protocol).toBeDefined();
      }
    });

    // CHARACTERIZATION (quality-refactor Phase 0 — Q12 guard). Pins the reachable AcceleratorStatus
    // discriminant invariants the discriminated-union refactor (Q12) must mirror exactly:
    // available:true ⟹ protocol defined + version present; available:false ⟹ protocol undefined.
    test("status discriminant invariants the Q12 union must preserve", async () => {
      const mk = () => new AcceleratorProver({ simulator: new WASMSimulator() });

      // available + compatible → protocol set, needsDownload false, version present
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            api_version: 1,
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          }),
      });
      let s = await mk().checkAcceleratorStatus();
      expect(s.available).toBe(true);
      expect(s.needsDownload).toBe(false);
      expect(s.protocol).toBeDefined();
      expect(typeof s.sdkAztecVersion).toBe("string");

      // available + incompatible (SDK version absent) → needsDownload true, protocol still set
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            api_version: 1,
            aztec_version: "0.0.0-other",
            available_versions: ["0.0.0-other"],
          }),
      });
      s = await mk().checkAcceleratorStatus();
      expect(s.available).toBe(true);
      expect(s.needsDownload).toBe(true);
      expect(s.protocol).toBeDefined();

      // unavailable (offline) → protocol undefined (the key discriminant)
      mockFetchOffline();
      s = await mk().checkAcceleratorStatus();
      expect(s.available).toBe(false);
      expect(s.protocol).toBeUndefined();
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

      // Advance past the status cache TTL (10s) so the next call re-probes. The cache clock is
      // performance.now() (monotonic), so that's what gets patched.
      const realNow = performance.now.bind(performance);
      performance.now = () => realNow() + 11_000;

      // Second check: accelerator is healthy — should re-probe and find it
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            api_version: 1,
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          }),
      });
      const status2 = await prover.checkAcceleratorStatus();
      expect(status2.available).toBe(true);
      expect(status2.protocol).toBeDefined();

      performance.now = realNow; // restore
    });

    test("returns available: false on legacy version mismatch", async () => {
      mockFetch({
        "/health": () =>
          Response.json({ status: "ok", api_version: 1, aztec_version: "0.0.0-fake" }),
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
            api_version: 1,
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

    test("returns cached status within TTL without re-probing", async () => {
      let probeCount = 0;
      mockFetch({
        "/health": () => {
          probeCount++;
          return Response.json({
            status: "ok",
            api_version: 1,
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          });
        },
      });

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });

      const status1 = await prover.checkAcceleratorStatus();
      expect(status1.available).toBe(true);
      const probesAfterFirst = probeCount;

      // Second call within TTL — should return cached result, no new probe
      const status2 = await prover.checkAcceleratorStatus();
      expect(status2.available).toBe(true);
      expect(probeCount).toBe(probesAfterFirst); // no additional probes
    });

    test("re-probes after status cache TTL expires", async () => {
      let probeCount = 0;
      mockFetch({
        "/health": () => {
          probeCount++;
          return Response.json({
            status: "ok",
            api_version: 1,
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          });
        },
      });

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });

      await prover.checkAcceleratorStatus();
      const probesAfterFirst = probeCount;

      // Advance past TTL (10s) — the cache clock is performance.now() (monotonic).
      const realNow = performance.now.bind(performance);
      performance.now = () => realNow() + 11_000;

      await prover.checkAcceleratorStatus();
      expect(probeCount).toBeGreaterThan(probesAfterFirst); // re-probed

      performance.now = realNow;
    });

    test("caches offline status and skips 1s retry on repeat calls", async () => {
      mockFetchOffline();

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });

      // First call: probes + retries (takes ~1s due to retry delay)
      const start = performance.now();
      const status1 = await prover.checkAcceleratorStatus();
      const firstCallMs = performance.now() - start;
      expect(status1.available).toBe(false);

      // Second call within TTL: should return immediately from cache (< 50ms)
      const start2 = performance.now();
      const status2 = await prover.checkAcceleratorStatus();
      const secondCallMs = performance.now() - start2;
      expect(status2.available).toBe(false);
      expect(secondCallMs).toBeLessThan(50); // cached, no probe or retry delay
    });

    test("detected protocol is used for subsequent /prove calls", async () => {
      const { fetchedUrls } = mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            api_version: 1,
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
            api_version: 1,
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

    test("setAcceleratorConfig() invalidates the status cache (no stale endpoint)", async () => {
      // Regression: setAcceleratorConfig reset the protocol but NOT #statusCache, so for up
      // to the TTL a re-check returned the OLD endpoint's cached result after reconfig.
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            api_version: 1,
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          }),
      });
      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });

      const first = await prover.checkAcceleratorStatus();
      expect(first.available).toBe(true);

      // Reconfigure, then make the new endpoint unreachable.
      prover.setAcceleratorConfig({ port: 12345 });
      mockFetchOffline();

      // Must re-probe (cache cleared) and see the new endpoint offline — not the stale true.
      const second = await prover.checkAcceleratorStatus();
      expect(second.available).toBe(false);
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
            api_version: 1,
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
            api_version: 1,
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

  // Post-impl hardening (codex bug-hunt fixes): strict health contract, single-flight probes, and
  // the /prove network-failure demotion path.
  describe("hardening — strict health contract + prove demotion", () => {
    const healthyBody = () =>
      Response.json({
        status: "ok",
        api_version: 1,
        aztec_version: SDK_AZTEC_VERSION,
        available_versions: [SDK_AZTEC_VERSION],
      });

    test("a 200 with foreign JSON (no status/api_version contract) is 'error', never available", async () => {
      mockFetch({
        // Valid JSON, wrong shape — the old field-presence check let shapes like this through.
        "/health": () => Response.json({ status: false, hello: "world" }),
      });
      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const status = await prover.checkAcceleratorStatus();
      expect(status.available).toBe(false);
      if (!status.available) expect(status.reason).toBe("error");
    });

    test("httpsOnly: a 200 array body is 'error' (strict mode enforces the contract too)", async () => {
      mockFetch({
        // The old strict-mode path skipped health validation entirely, so `200 []` was accepted
        // and the witness would have been POSTed to the squatter.
        "/health": () => Response.json([]),
      });
      const prover = new AcceleratorProver({
        simulator: new WASMSimulator(),
        accelerator: { httpsOnly: true },
      });
      const status = await prover.checkAcceleratorStatus();
      expect(status.available).toBe(false);
      if (!status.available) expect(status.reason).toBe("error");
    });

    test("concurrent status checks share one in-flight probe (single-flight)", async () => {
      let healthCalls = 0;
      mockFetch({
        "/health": () => {
          healthCalls++;
          return healthyBody();
        },
      });
      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const [a, b, c] = await Promise.all([
        prover.checkAcceleratorStatus(),
        prover.checkAcceleratorStatus(),
        prover.checkAcceleratorStatus(),
      ]);
      expect(a.available).toBe(true);
      expect(b).toEqual(a);
      expect(c).toEqual(a);
      // One dual probe = at most 2 /health requests (http + https) — NOT 2 per caller.
      expect(healthCalls).toBeLessThanOrEqual(2);
    });

    test("pinned-HTTPS /prove network failure retries over HTTP once that endpoint VALIDATES", async () => {
      const serSpy = mockSerializer();
      const { fetchedUrls } = mockFetch({
        // Both endpoints are ours and healthy; HTTPS wins the probe by preference.
        "http://127.0.0.1:59833/health": () => healthyBody(),
        "https://127.0.0.1:59834/health": () => healthyBody(),
        // HTTPS /prove dies at the network layer (trust/listener changed since /health)...
        "https://127.0.0.1:59834/prove": () => {
          throw new TypeError("TLS handshake failed");
        },
        // ...and the HTTP endpoint, re-validated first, still proves fine.
        "http://127.0.0.1:59833/prove": () => Response.json({ proof: "" }),
      });

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      try {
        await prover.createChonkProof([fakeStep]);
      } catch {
        // The stub {proof:""} body fails ChonkProof decode — the retried REQUEST is the assertion.
      }
      expect(fetchedUrls).toContain("https://127.0.0.1:59834/prove");
      expect(fetchedUrls).toContain("http://127.0.0.1:59833/prove");
      serSpy.mockRestore();
    });

    test("the witness is NEVER downgraded to an HTTP endpoint that fails the health contract", async () => {
      // codex CRITICAL: the demotion retry used to POST straight to HTTP without validating it. A
      // healthy HTTPS probe says nothing about who is listening on the HTTP port, so a foreign
      // responder there received the serialized witness the moment HTTPS failed.
      const serSpy = mockSerializer();
      const wasmSpy = mockWasmProver();
      const { fetchedUrls } = mockFetch({
        // Something else is on the HTTP port: 200 JSON, but NOT the accelerator's contract.
        "http://127.0.0.1:59833/health": () => Response.json({ hello: "not the accelerator" }),
        "https://127.0.0.1:59834/health": () => healthyBody(),
        "https://127.0.0.1:59834/prove": () => {
          throw new TypeError("TLS handshake failed");
        },
        // If this is ever reached, the witness has gone to the foreign responder.
        "http://127.0.0.1:59833/prove": () => Response.json({ proof: "" }),
      });

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      await expect(prover.createChonkProof([fakeStep])).rejects.toThrow(
        "local prover not available in test", // WASM fallback was reached instead
      );
      expect(fetchedUrls).toContain("https://127.0.0.1:59834/prove");
      expect(fetchedUrls).not.toContain("http://127.0.0.1:59833/prove");
      wasmSpy.mockRestore();
      serSpy.mockRestore();
    });

    test("httpsOnly: /prove network failure falls back to WASM (never a plaintext retry)", async () => {
      const serSpy = mockSerializer();
      const wasmSpy = mockWasmProver();
      const { fetchedUrls } = mockFetch({
        "https://127.0.0.1:59834/health": () => healthyBody(),
        "https://127.0.0.1:59834/prove": () => {
          throw new TypeError("TLS handshake failed");
        },
      });

      const prover = new AcceleratorProver({
        simulator: new WASMSimulator(),
        accelerator: { httpsOnly: true },
      });
      await expect(prover.createChonkProof([fakeStep])).rejects.toThrow(
        "local prover not available in test", // WASM fallback WAS reached (mock throws this)
      );
      expect(wasmSpy).toHaveBeenCalled();
      expect(fetchedUrls.every((u) => !u.startsWith("http://"))).toBe(true);
      wasmSpy.mockRestore();
      serSpy.mockRestore();
    });

    test("a proof reconfigured mid-flight does NOT retry against the new endpoint (WASM instead)", async () => {
      const serSpy = mockSerializer();
      const wasmSpy = mockWasmProver();
      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const { fetchedUrls } = mockFetch({
        // HTTP health refused → HTTPS pinned.
        "http://127.0.0.1:59833/health": () => {
          throw new TypeError("refused");
        },
        "https://127.0.0.1:59834/health": () => healthyBody(),
        // HTTPS /prove: reconfigure the endpoint (bump generation) THEN fail at the network layer.
        "https://127.0.0.1:59834/prove": () => {
          prover.setAcceleratorConfig({ port: 51337, httpsPort: 51338 });
          throw new TypeError("TLS handshake failed");
        },
      });

      await expect(prover.createChonkProof([fakeStep])).rejects.toThrow(
        "local prover not available in test", // WASM fallback WAS reached
      );
      // The witness must NEVER have been POSTed to the reconfigured endpoint B (codex High).
      expect(fetchedUrls.some((u) => u.includes("51337") || u.includes("51338"))).toBe(false);
      wasmSpy.mockRestore();
      serSpy.mockRestore();
    });

    test("an onPhase callback that reconfigures mid-proof cannot redirect the witness", async () => {
      // codex High (round 2): the initial postProve() used to read the MUTABLE baseUrl *after* the
      // onPhase callbacks ran, so a handler calling setAcceleratorConfig(B) sent the witness to the
      // unprobed B. The attempt now POSTs to a URL snapshotted before any callback.
      const serSpy = mockSerializer();
      const wasmSpy = mockWasmProver();
      let prover!: AcceleratorProver;
      prover = new AcceleratorProver({
        simulator: new WASMSimulator(),
        onPhase: (phase) => {
          // Reconfigure at the last possible moment before transmission.
          if (phase === "transmit") prover.setAcceleratorConfig({ port: 51337, httpsPort: 51338 });
        },
      });
      const { fetchedUrls } = mockFetch({
        "127.0.0.1:59833/health": () => healthyBody(),
        "127.0.0.1:59833/prove": () => Response.json({ proof: "" }),
      });

      try {
        await prover.createChonkProof([fakeStep]);
      } catch {
        // Either the WASM fallback (endpoint changed) or a decode failure — the URLs are the assertion.
      }
      // The witness must NEVER have gone to the reconfigured endpoint B.
      expect(fetchedUrls.some((u) => u.includes("51337") || u.includes("51338"))).toBe(false);
      wasmSpy.mockRestore();
      serSpy.mockRestore();
    });

    test("a status from a probe whose endpoint changed mid-flight does not drive a remote prove", async () => {
      // codex High (round 2): the probe's pin/cache commit was discarded on a generation change, but
      // its `available:true` was still returned and proved upon — against the NEW, unprobed endpoint.
      const serSpy = mockSerializer();
      const wasmSpy = mockWasmProver();
      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const { fetchedUrls } = mockFetch({
        "/health": async () => {
          // Reconfigure while the probe is in flight, then answer healthy for the OLD endpoint.
          prover.setAcceleratorConfig({ port: 51337, httpsPort: 51338 });
          return healthyBody();
        },
      });

      await expect(prover.createChonkProof([fakeStep])).rejects.toThrow(
        "local prover not available in test", // WASM fallback was reached
      );
      expect(fetchedUrls.some((u) => u.includes("/prove"))).toBe(false);
      wasmSpy.mockRestore();
      serSpy.mockRestore();
    });

    test("two concurrent proofs failing over the pinned HTTPS BOTH fall back (neither left with the raw error)", async () => {
      const serSpy = mockSerializer();
      const wasmSpy = mockWasmProver();
      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      mockFetch({
        "http://127.0.0.1:59833/health": () => {
          throw new TypeError("refused");
        },
        "https://127.0.0.1:59834/health": () => healthyBody(),
        "https://127.0.0.1:59834/prove": () => {
          throw new TypeError("TLS handshake failed");
        },
        // The HTTP retry also fails → both must degrade to WASM.
        "http://127.0.0.1:59833/prove": () => {
          throw new TypeError("refused");
        },
      });

      const results = await Promise.allSettled([
        prover.createChonkProof([fakeStep]),
        prover.createChonkProof([fakeStep]),
      ]);
      // codex Medium: the old demote-as-gate left the SECOND caller rethrowing the raw TLS error.
      // Both must reach the WASM fallback (mock throws this), decided per-request not by the shared pin.
      for (const r of results) {
        expect(r.status).toBe("rejected");
        expect(String((r as PromiseRejectedResult).reason)).toContain(
          "local prover not available in test",
        );
      }
      wasmSpy.mockRestore();
      serSpy.mockRestore();
    });

    test("a probe raced by setAcceleratorConfig cannot pin the new endpoint (generation guard)", async () => {
      // Slow /health against endpoint A; reconfigure to B mid-flight; A's late result must not
      // pin/cache anything for B.
      mockFetch({
        "/health": async () => {
          await new Promise((r) => setTimeout(r, 150));
          return healthyBody();
        },
      });
      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const statusP = prover.checkAcceleratorStatus(); // in flight against A
      prover.setAcceleratorConfig({ port: 51337, httpsPort: 51338 }); // now B
      await statusP; // A's probe completes; its commit must be discarded

      // A fresh check against B must actually probe B (no stale cache/pin from A's commit).
      const { fetchedUrls } = mockFetch({
        "/health": () => healthyBody(),
      });
      const status = await prover.checkAcceleratorStatus();
      expect(status.available).toBe(true);
      expect(fetchedUrls.some((u) => u.includes(":51337") || u.includes(":51338"))).toBe(true);
    });
  });
});
