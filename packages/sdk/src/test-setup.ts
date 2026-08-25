// @aztec/foundation's logger builds a worker-thread transport at import time unless this env is
// set (its Jest branch: sync fd destination, no Worker) — required under bun 1.4.0, where
// happy-dom's global MessagePort replacement breaks Worker construction (oven-sh/bun#40268,
// fixed upstream in #40271; remove once a release carrying that fix is our pin). `??=` so bun
// test --parallel's real per-worker IDs survive.
process.env.JEST_WORKER_ID ??= "1";

import { expect } from "bun:test";

// Patch expect for @aztec/foundation compatibility
// @aztec/foundation checks if expect.addEqualityTesters exists (vitest API)
if (!(expect as any).addEqualityTesters) {
  (expect as any).addEqualityTesters = () => {};
}
if ((globalThis as any).expect && !(globalThis as any).expect.addEqualityTesters) {
  (globalThis as any).expect.addEqualityTesters = () => {};
}
