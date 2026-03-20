/**
 * E2E test setup — runs once before all test files via preload.
 *
 * Asserts that Aztec node is reachable (mandatory).
 * Accelerator health is only checked when ACCELERATOR_URL is set
 * (the accelerator is a desktop app — optional by design).
 * Throws immediately if required services are unavailable.
 */

import { expect } from "bun:test";
import { configure, getConsoleSink, parseLogLevel } from "@logtape/logtape";

// Patch expect for @aztec/foundation compatibility
if (!(expect as any).addEqualityTesters) {
  (expect as any).addEqualityTesters = () => {};
}
if ((globalThis as any).expect && !(globalThis as any).expect.addEqualityTesters) {
  (globalThis as any).expect.addEqualityTesters = () => {};
}

// Configure LogTape
const logLevel = parseLogLevel(process.env.LOG_LEVEL || "warning");

await configure({
  sinks: {
    console: getConsoleSink(),
  },
  loggers: [
    {
      category: ["logtape", "meta"],
      sinks: ["console"],
      lowestLevel: "warning",
    },
    {
      category: ["aztec-accelerator"],
      sinks: ["console"],
      lowestLevel: logLevel,
    },
  ],
});

// Environment configuration
export const config = {
  nodeUrl: process.env.AZTEC_NODE_URL || "http://localhost:8080",
  /** Optional accelerator URL — accelerated tests are skipped when not set. */
  acceleratorUrl: process.env.ACCELERATOR_URL || "",
};

/** True when pointing at a local sandbox (default). */
export const isLocalNetwork =
  config.nodeUrl.includes("localhost") || config.nodeUrl.includes("127.0.0.1");

// Assert local services are available — fail fast with a clear message.
// Only checks when targeting local network. Remote networks (testnet) are
// validated by the test files themselves (remote-network.test.ts, etc.).
async function assertLocalServicesAvailable(): Promise<void> {
  if (!isLocalNetwork) return;

  const aztecOk = await fetch(`${config.nodeUrl}/status`, { signal: AbortSignal.timeout(5000) })
    .then((r) => r.ok)
    .catch(() => false);

  if (!aztecOk) {
    throw new Error(
      `Aztec node not available at ${config.nodeUrl}. ` +
        "Start Aztec local network before running e2e tests.\n" +
        "  aztec start --local-network",
    );
  }

  if (config.acceleratorUrl) {
    const acceleratorOk = await fetch(`${config.acceleratorUrl}/health`, {
      signal: AbortSignal.timeout(5000),
    })
      .then((r) => r.ok)
      .catch(() => false);

    if (!acceleratorOk) {
      throw new Error(
        `Accelerator not available at ${config.acceleratorUrl}. ` +
          "ACCELERATOR_URL is set but the accelerator is not responding.\n" +
          "  Start the accelerator desktop app or unset ACCELERATOR_URL to skip accelerator tests.",
      );
    }
  }
}

await assertLocalServicesAvailable();
