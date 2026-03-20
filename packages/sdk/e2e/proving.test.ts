import { describe, expect, test } from "bun:test";
import { AcceleratorProver } from "../src/index";

const AZTEC_NODE_URL = process.env.AZTEC_NODE_URL;

describe.skipIf(!AZTEC_NODE_URL)("AcceleratorProver E2E", () => {
  test("checkAcceleratorStatus returns a result", async () => {
    const prover = new AcceleratorProver();
    const status = await prover.checkAcceleratorStatus();
    expect(typeof status.available).toBe("boolean");
    expect(typeof status.needsDownload).toBe("boolean");
  });
});
