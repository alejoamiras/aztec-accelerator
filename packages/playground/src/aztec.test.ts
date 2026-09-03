import { afterEach, describe, expect, mock, test } from "bun:test";
import {
  checkAcceleratorStatus,
  checkAztecNode,
  getAcceleratorProver,
  setUiMode,
  state,
} from "./aztec";

// ── fetch mocking ──
const originalFetch = globalThis.fetch;

// Bun's `typeof fetch` includes `preconnect`, which the test doubles don't need.
function setFetchMock(impl: () => Promise<Response>): void {
  globalThis.fetch = mock(impl) as unknown as typeof fetch;
}

afterEach(() => {
  globalThis.fetch = originalFetch;
  // Reset state
  state.prover = null;
  state.wallet = null;
  state.embeddedWallet = null;
  state.registeredAddresses = [];
  state.sessionAddresses = [];
  state.uiMode = "accelerated";
  state.proofsRequired = false;
  state.feePaymentMethod = undefined;
});

// ── checkAztecNode ──
// Health check is the node_getNodeInfo JSON-RPC POST (5.0.0 nodes 405 a plain GET /status).
describe("checkAztecNode", () => {
  test("returns reachable with version when the RPC responds", async () => {
    setFetchMock(() =>
      Promise.resolve(
        new Response(JSON.stringify({ result: { nodeVersion: "5.0.0" } }), { status: 200 }),
      ),
    );
    expect(await checkAztecNode()).toEqual({ reachable: true, nodeVersion: "5.0.0" });
  });

  test("returns reachable without version when the RPC responds without a result", async () => {
    setFetchMock(() => Promise.resolve(new Response(JSON.stringify({}), { status: 200 })));
    expect(await checkAztecNode()).toEqual({ reachable: true });
  });

  test("returns not reachable when the RPC responds 500", async () => {
    setFetchMock(() => Promise.resolve(new Response("", { status: 500 })));
    expect(await checkAztecNode()).toEqual({ reachable: false });
  });

  test("returns not reachable when fetch throws", async () => {
    setFetchMock(() => Promise.reject(new Error("network error")));
    expect(await checkAztecNode()).toEqual({ reachable: false });
  });
});

// Real-node integration check: mocks missed the 5.0.0 GET-/status-405 change that broke
// the deployed playground — this closes that loop against an actual node when one is
// configured (AZTEC_NODE_URL=https://... bun run test:live). Must run with this package as
// cwd: bunfig's preloaded happydom.ts carries the expect.addEqualityTesters patch that
// @aztec/foundation's field module needs at import time under bun:test.
describe.skipIf(!process.env.AZTEC_NODE_URL)("checkAztecNode (live node)", () => {
  test(
    "real node answers the node_getNodeInfo probe",
    async () => {
      const result = await checkAztecNode();
      expect(result.reachable).toBe(true);
      expect(result.nodeVersion).toBeDefined();
    },
    { retry: 1 },
  );
});

// ── checkAcceleratorStatus ──
describe("checkAcceleratorStatus", () => {
  test("returns the SDK status when the recognized health check succeeds", async () => {
    setFetchMock(() =>
      Promise.resolve(
        Response.json({
          status: "ok",
          api_version: 1,
          available_versions: [],
        }),
      ),
    );
    expect((await checkAcceleratorStatus()).available).toBe(true);
  });

  test("returns an actionable secure status when browser HTTPS and its diagnostic fail", async () => {
    setFetchMock(() => Promise.reject(new Error("connection refused")));
    expect(await checkAcceleratorStatus()).toMatchObject({
      available: false,
      reason: "secure-connection-unavailable",
      diagnosis: "unconfirmed",
    });
  });

  test("startup checks and wallet/proving access reuse one lazy prover", () => {
    const startup = getAcceleratorProver();
    expect(state.prover).toBe(startup);
    expect(getAcceleratorProver()).toBe(startup);
  });
});

// ── setUiMode ──
describe("setUiMode", () => {
  test("sets uiMode to local", () => {
    setUiMode("local");
    expect(state.uiMode).toBe("local");
  });

  test("sets uiMode to accelerated", () => {
    setUiMode("local");
    setUiMode("accelerated");
    expect(state.uiMode).toBe("accelerated");
  });
});
