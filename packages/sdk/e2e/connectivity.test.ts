/**
 * Connectivity tests — verify Aztec node and accelerator are reachable.
 *
 * Services must be running before tests start (asserted by e2e-setup.ts preload).
 */

import { describe, expect, test } from "bun:test";
import { createAztecNodeClient } from "@aztec/aztec.js/node";
import { getLogger } from "@logtape/logtape";
import { config } from "./e2e-setup";

const logger = getLogger(["aztec-accelerator", "sdk", "e2e", "connectivity"]);

describe("Service Connectivity", () => {
  describe("Aztec Node", () => {
    test("should return node info", async () => {
      const node = createAztecNodeClient(config.nodeUrl);
      const nodeInfo = await node.getNodeInfo();

      expect(nodeInfo).toBeDefined();
      expect(nodeInfo.l1ChainId).toBeDefined();
      logger.info("Got node info", { chainId: nodeInfo.l1ChainId });
    });
  });

  describe.skipIf(!config.acceleratorUrl)("Accelerator", () => {
    test("should return health status", async () => {
      const response = await fetch(`${config.acceleratorUrl}/health`);
      expect(response.ok).toBe(true);

      const data = await response.json();
      expect(data.available_versions).toBeDefined();
      expect(Array.isArray(data.available_versions)).toBe(true);
      logger.info("Accelerator healthy", { versions: data.available_versions });
    });
  });
});
