import { afterEach, describe, expect, mock, test } from "bun:test";
import { checkAccelerator, checkAztecNode, setUiMode, state } from "./aztec";

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
  state.selectedAccountIndex = 0;
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
// configured (AZTEC_NODE_URL=https://... bun test src/aztec.test.ts).
describe.skipIf(!process.env.AZTEC_NODE_URL)("checkAztecNode (live node)", () => {
  test("real node answers the node_getNodeInfo probe", async () => {
    const result = await checkAztecNode();
    expect(result.reachable).toBe(true);
    expect(result.nodeVersion).toBeDefined();
  });
});

// ── checkAccelerator ──
describe("checkAccelerator", () => {
  test("returns true when health check succeeds", async () => {
    setFetchMock(() =>
      Promise.resolve(new Response(JSON.stringify({ status: "ok" }), { status: 200 })),
    );
    expect(await checkAccelerator()).toBe(true);
  });

  test("returns false when fetch throws", async () => {
    setFetchMock(() => Promise.reject(new Error("connection refused")));
    expect(await checkAccelerator()).toBe(false);
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
