import { afterEach, beforeEach, describe, expect, mock, spyOn, test } from "bun:test";
import { BBLazyPrivateKernelProver } from "@aztec/bb-prover/client/lazy";
import { WASMSimulator } from "@aztec/simulator/client";
import * as stdlibKernel from "@aztec/stdlib/kernel";
import sdkPkg from "../../package.json" with { type: "json" };
import { AcceleratorProver } from "./accelerator-prover.js";
import { AcceleratorHttpError } from "./errors.js";

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

    test("a 200 whose body cannot be decoded degrades to WASM instead of breaking the dApp", async () => {
      // The decode sits outside the transport catch (a bad body is not a transport failure and must
      // not re-trigger the HTTPS→HTTP demotion). It used to ESCAPE from there, so an accelerator that
      // answered 200 with garbage failed the caller outright — while the identical body on the HTTP
      // retry path was absorbed into a WASM fallback. A 200 we can't parse says nothing about WASM's
      // ability to finish the proof, so both paths degrade.
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            api_version: 1,
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          }),
        "/prove": () => new Response("not json at all", { status: 200 }),
      });
      const serializeSpy = mockSerializer();
      const wasmSpy = mockWasmProver();
      const phases: string[] = [];

      const prover = new AcceleratorProver({
        simulator: new WASMSimulator(),
        onPhase: (phase) => phases.push(phase),
      });

      // Reaching the WASM prover at all is the assertion — the mock is what throws, not the decode.
      await expect(prover.createChonkProof([fakeStep])).rejects.toThrow(
        "local prover not available in test",
      );
      expect(phases).toContain("fallback");
      expect(wasmSpy).toHaveBeenCalled();

      wasmSpy.mockRestore();
      serializeSpy.mockRestore();
    });

    test("falls back to WASM on 503 (accelerator unavailable)", async () => {
      // The accelerator answers 503 when it is shutting down, and (as of the F-06 version lease) when
      // the cached bb for this version is being evicted. Both used to reach `throw err`, because the
      // network-failure branch is gated on `!(err instanceof HTTPError)` and a 503 is one — so
      // quitting the app mid-proof failed the dApp's transaction outright. The accelerator is an
      // optimisation; the only correct degradation is WASM.
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            api_version: 1,
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          }),
        "/prove": () =>
          Response.json(
            { error: "service_unavailable", message: "Proving service shutting down" },
            { status: 503 },
          ),
      });
      const serializeSpy = mockSerializer();
      const wasmSpy = mockWasmProver();
      const phases: string[] = [];

      const prover = new AcceleratorProver({
        simulator: new WASMSimulator(),
        onPhase: (phase) => phases.push(phase),
      });

      // Reaching the WASM prover at all is the assertion — the mock rejects, which is how this suite
      // observes "we got there". Pre-fix the 503 propagated instead and this threw an HTTPError.
      await expect(prover.createChonkProof([fakeStep])).rejects.toThrow(
        "local prover not available in test",
      );

      expect(phases).toContain("fallback");
      expect(wasmSpy).toHaveBeenCalled();
      wasmSpy.mockRestore();
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

    // ── B7 (F14): the prove error taxonomy ──
    // The accelerator sends its error body as text/plain carrying a JSON string (server.rs
    // `prove_error_responses_stay_text_plain`), so `ky` makes `err.data` a STRING — the fixtures below use
    // that exact shape (NOT `Response.json`) so the code-recovery path is exercised the way production hits
    // it. Recognised conditions degrade to WASM; only a caller misconfiguration / unrecognised error is
    // thrown as a typed `AcceleratorHttpError`.
    const healthOk = () =>
      Response.json({
        status: "ok",
        api_version: 1,
        aztec_version: SDK_AZTEC_VERSION,
        available_versions: [SDK_AZTEC_VERSION],
      });
    const proveError = (status: number, code?: string) =>
      new Response(code ? JSON.stringify({ error: code, message: "server said so" }) : "", {
        status,
        headers: { "content-type": "text/plain" },
      });

    // [name, status, code, expected phase (or null), phase that must NOT appear]
    const fallbackCases: Array<[string, number, string | undefined, string | null, string | null]> =
      [
        ["403 origin_denied", 403, "origin_denied", "denied", null],
        ["403 authorization_timeout", 403, "authorization_timeout", "denied", null],
        ["403 authorization_cancelled", 403, "authorization_cancelled", "denied", null],
        ["403 version_not_allowed", 403, "version_not_allowed", "version-mismatch", "denied"],
        ["403 authorization_cooldown", 403, "authorization_cooldown", null, "denied"],
        ["503 service_unavailable", 503, "service_unavailable", null, null],
        ["408 body_read_timeout", 408, "body_read_timeout", null, null],
        ["413 payload_too_large", 413, "payload_too_large", null, null],
        ["429 too_many_requests", 429, "too_many_requests", null, null],
        ["429 prove_queue_full", 429, "prove_queue_full", null, null],
        ["500 download_failed", 500, "download_failed", null, null],
        ["500 prove_failed", 500, "prove_failed", null, null],
      ];
    for (const [name, status, code, wantPhase, notPhase] of fallbackCases) {
      test(`F14 falls back to WASM: ${name}`, async () => {
        mockFetch({ "/health": healthOk, "/prove": () => proveError(status, code) });
        const serializeSpy = mockSerializer();
        const wasmSpy = mockWasmProver();
        const phases: string[] = [];
        const prover = new AcceleratorProver({
          simulator: new WASMSimulator(),
          onPhase: (p) => phases.push(p),
        });
        // Reaching the WASM prover (which the mock rejects) is how we observe a fallback.
        await expect(prover.createChonkProof([fakeStep])).rejects.toThrow(
          "local prover not available in test",
        );
        expect(wasmSpy).toHaveBeenCalled();
        expect(phases).toContain("fallback");
        if (wantPhase) expect(phases, `must emit ${wantPhase}`).toContain(wantPhase);
        if (notPhase) expect(phases, `must NOT emit ${notPhase}`).not.toContain(notPhase);
        wasmSpy.mockRestore();
        serializeSpy.mockRestore();
      });
    }

    // no_raw_ky_error_escapes: a misconfiguration or unrecognised error is a TYPED throw, never a raw ky
    // HTTPError, and never silently masked as WASM. [mut: revert the `#fallbackOrThrowHttp` throw → WASM].
    const throwCases: Array<[string, number, string | undefined]> = [
      ["400 invalid_version", 400, "invalid_version"],
      ["400 invalid_origin", 400, "invalid_origin"],
      ["500 unrecognised code", 500, "some_unknown_fault"],
      ["418 unrecognised status", 418, undefined],
      ["404 not found", 404, "not_found"],
    ];
    for (const [name, status, code] of throwCases) {
      test(`F14 throws typed AcceleratorHttpError, never falls back: ${name}`, async () => {
        mockFetch({ "/health": healthOk, "/prove": () => proveError(status, code) });
        const serializeSpy = mockSerializer();
        const wasmSpy = mockWasmProver();
        const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
        const err = await prover.createChonkProof([fakeStep]).catch((e) => e);
        expect(err, `${name} must be typed`).toBeInstanceOf(AcceleratorHttpError);
        expect(err.status).toBe(status);
        if (code) expect(err.code).toBe(code);
        expect(wasmSpy, "must NOT mask a misconfiguration as WASM").not.toHaveBeenCalled();
        wasmSpy.mockRestore();
        serializeSpy.mockRestore();
      });
    }

    test("B7: surfaces the accelerator appVersion + apiVersion from /health", async () => {
      mockFetch({
        "/health": () =>
          Response.json({
            status: "ok",
            api_version: 1,
            version: "2.0.0",
            aztec_version: SDK_AZTEC_VERSION,
            available_versions: [SDK_AZTEC_VERSION],
          }),
      });
      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      const status = await prover.checkAcceleratorStatus();
      expect(status.available).toBe(true);
      if (status.available) {
        // Mutation proof: drop `appVersion`/`apiVersion` from #classifyHealth's return and these fail.
        expect(status.appVersion).toBe("2.0.0");
        expect(status.apiVersion).toBe(1);
      }
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

    // ── F-01 regression (audit 2026-07-31) ────────────────────────────────────────────────────
    // These two used to be ONE test asserting the downgrade always happened. The `isProtocolHealthy`
    // guard it relied on is the `/health` SHAPE contract — its own comment calls it collision
    // resistance, not authentication — and ANY local account can bind 127.0.0.1:59833 to satisfy it.
    // So a cross-user attacker who could make the HTTPS `/prove` fail received the private witness in
    // cleartext. "Prefer HTTPS" is now "never downgrade FROM a working HTTPS", with an opt-out.
    const downgradeScenario = () =>
      mockFetch({
        // Both endpoints answer the contract; HTTPS wins the probe by preference.
        "http://127.0.0.1:59833/health": () => healthyBody(),
        "https://127.0.0.1:59834/health": () => healthyBody(),
        // HTTPS /prove dies at the network layer (trust/listener changed since /health)...
        "https://127.0.0.1:59834/prove": () => {
          throw new TypeError("TLS handshake failed");
        },
        // ...and the HTTP endpoint would happily take the witness.
        "http://127.0.0.1:59833/prove": () => Response.json({ proof: "" }),
      });

    test("a failed HTTPS /prove is NOT retried over plaintext HTTP by default", async () => {
      const serSpy = mockSerializer();
      const wasmSpy = mockWasmProver();
      const { fetchedUrls } = downgradeScenario();

      const prover = new AcceleratorProver({ simulator: new WASMSimulator() });
      await expect(prover.createChonkProof([fakeStep])).rejects.toThrow(
        "local prover not available in test", // WASM fallback was reached instead
      );

      expect(fetchedUrls).toContain("https://127.0.0.1:59834/prove");
      // The assertion: the witness never reaches the plaintext port. Pre-F-01 it did.
      expect(fetchedUrls).not.toContain("http://127.0.0.1:59833/prove");
      expect(wasmSpy).toHaveBeenCalled();
      wasmSpy.mockRestore();
      serSpy.mockRestore();
    });

    test("an unreadable body on the HTTP retry degrades to WASM instead of reaching the dApp", async () => {
      // F-11 + post-impl codex round 4: the retry path returned `#decodeProof(...)` WITHOUT
      // awaiting it, so its rejection escaped the surrounding try/catch and never reached the
      // fallback below — bounding the body would have converted a hang into a hard failure for the
      // dApp rather than the WASM degradation the catch exists to provide. The sibling call on the
      // non-retry path awaited; this one did not.
      const serSpy = mockSerializer();
      const wasmSpy = mockWasmProver();
      mockFetch({
        "http://127.0.0.1:59833/health": () => healthyBody(),
        "https://127.0.0.1:59834/health": () => healthyBody(),
        "https://127.0.0.1:59834/prove": () => {
          throw new TypeError("TLS handshake failed");
        },
        // The retry succeeds at the network layer, then returns a body past the 8 MiB cap.
        "http://127.0.0.1:59833/prove": () => Response.json({ proof: "A".repeat(9 * 1024 * 1024) }),
      });

      const prover = new AcceleratorProver({
        simulator: new WASMSimulator(),
        accelerator: { allowInsecureDowngrade: true },
      });
      // Reaching the WASM prover at all is the assertion — its stub rejection is the marker. Before
      // the fix this rejected with the /prove body error instead, never having tried WASM.
      await expect(prover.createChonkProof([fakeStep])).rejects.toThrow(
        "local prover not available in test",
      );
      wasmSpy.mockRestore();
      serSpy.mockRestore();
    });

    test("allowInsecureDowngrade opts back into the HTTP retry", async () => {
      const serSpy = mockSerializer();
      const { fetchedUrls } = downgradeScenario();

      const prover = new AcceleratorProver({
        simulator: new WASMSimulator(),
        accelerator: { allowInsecureDowngrade: true },
      });
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
