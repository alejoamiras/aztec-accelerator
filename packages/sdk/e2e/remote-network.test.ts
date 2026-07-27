/**
 * Remote network connectivity smoke tests
 *
 * Validates that the Aztec node (testnet or other remote network) is
 * reachable and healthy. Auto-skipped when running against local sandbox.
 *
 * Run with: bun run test:e2e:remote
 */

import { describe, expect, test } from "bun:test";
import { createAztecNodeClient } from "@aztec/aztec.js/node";
import { getLogger } from "@logtape/logtape";
import { config, isLocalNetwork } from "./e2e-setup.js";

const logger = getLogger(["aztec-accelerator", "sdk", "e2e", "remote-network"]);

describe.skipIf(isLocalNetwork)("Remote Network Connectivity", () => {
  test("should reach the Aztec node via node_getNodeInfo RPC", async () => {
    // 5.0.0 nodes reject a plain GET /status with 405 — probe via JSON-RPC.
    const res = await fetch(config.nodeUrl, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", method: "node_getNodeInfo", params: [], id: 1 }),
      signal: AbortSignal.timeout(10_000),
    });
    expect(res.ok).toBe(true);
    const data = (await res.json()) as { result?: { nodeVersion?: string } };
    expect(data.result?.nodeVersion).toBeDefined();
    logger.info("Node RPC reachable", { url: config.nodeUrl, version: data.result?.nodeVersion });
  });

  test("should return non-sandbox chain ID", async () => {
    const node = createAztecNodeClient(config.nodeUrl);
    const nodeInfo = await node.getNodeInfo();

    expect(nodeInfo.l1ChainId).toBeDefined();
    expect(nodeInfo.l1ChainId).not.toBe(31337);
    logger.info("Chain ID verified", { chainId: nodeInfo.l1ChainId });
  });

  test("should return valid node info", async () => {
    const node = createAztecNodeClient(config.nodeUrl);
    const nodeInfo = await node.getNodeInfo();

    expect(nodeInfo.l1ChainId).toBeGreaterThan(0);
    expect(nodeInfo.nodeVersion).toBeDefined();
    logger.info("Node info valid", {
      chainId: nodeInfo.l1ChainId,
      nodeVersion: nodeInfo.nodeVersion,
    });
  });
});
