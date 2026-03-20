import { afterEach, describe, expect, mock, test } from "bun:test";
import { checkAccelerator, checkAztecNode, setUiMode, state } from "./aztec";

// ── fetch mocking ──
const originalFetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = originalFetch;
  // Reset state
  state.prover = null;
  state.wallet = null;
  state.embeddedWallet = null;
  state.registeredAddresses = [];
  state.selectedAccountIndex = 0;
  state.uiMode = "accelerated";
  state.proofsRequired = false;
  state.feePaymentMethod = undefined;
});

// ── checkAztecNode ──
describe("checkAztecNode", () => {
  test("returns reachable when status responds 200", async () => {
    let callCount = 0;
    globalThis.fetch = mock(() => {
      callCount++;
      if (callCount === 1) return Promise.resolve(new Response("OK", { status: 200 }));
      // RPC call for node version
      return Promise.resolve(
        new Response(JSON.stringify({ result: { nodeVersion: "4.1.0-rc.2" } }), { status: 200 }),
      );
    });
    expect(await checkAztecNode()).toEqual({ reachable: true, nodeVersion: "4.1.0-rc.2" });
  });

  test("returns reachable without version when RPC fails", async () => {
    let callCount = 0;
    globalThis.fetch = mock(() => {
      callCount++;
      if (callCount === 1) return Promise.resolve(new Response("OK", { status: 200 }));
      return Promise.reject(new Error("rpc failed"));
    });
    expect(await checkAztecNode()).toEqual({ reachable: true });
  });

  test("returns not reachable when status responds 500", async () => {
    globalThis.fetch = mock(() => Promise.resolve(new Response("", { status: 500 })));
    expect(await checkAztecNode()).toEqual({ reachable: false });
  });

  test("returns not reachable when fetch throws", async () => {
    globalThis.fetch = mock(() => Promise.reject(new Error("network error")));
    expect(await checkAztecNode()).toEqual({ reachable: false });
  });
});

// ── checkAccelerator ──
describe("checkAccelerator", () => {
  test("returns true when health check succeeds", async () => {
    globalThis.fetch = mock(() =>
      Promise.resolve(new Response(JSON.stringify({ status: "ok" }), { status: 200 })),
    );
    expect(await checkAccelerator()).toBe(true);
  });

  test("returns false when fetch throws", async () => {
    globalThis.fetch = mock(() => Promise.reject(new Error("connection refused")));
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
    state.uiMode = "local";
    setUiMode("accelerated");
    expect(state.uiMode).toBe("accelerated");
  });
});
