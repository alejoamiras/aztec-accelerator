// @aztec/foundation's logger builds a worker-thread transport at import time unless this env is
// set (its Jest branch: sync fd destination, no Worker). Load-bearing HERE specifically: this
// preload's GlobalRegistrator.register() replaces globalThis.MessagePort with a DOM-shaped one,
// and bun 1.4.0's Worker bootstrap reads that mutable global — any Worker constructed afterwards
// throws "port.on is not a function" (oven-sh/bun#40268, fixed upstream in #40271; remove once a
// release carrying that fix is our pin). `??=` so bun test --parallel's worker IDs survive.
process.env.JEST_WORKER_ID ??= "1";

import { expect } from "bun:test";
import { GlobalRegistrator } from "@happy-dom/global-registrator";

GlobalRegistrator.register();

// Patch expect for @aztec/foundation compatibility (same as SDK/server)
if (!(expect as unknown as { addEqualityTesters?: unknown }).addEqualityTesters) {
  (expect as unknown as Record<string, unknown>).addEqualityTesters = () => {};
}
if (
  (globalThis as unknown as { expect?: { addEqualityTesters?: unknown } }).expect &&
  !(globalThis as unknown as { expect: { addEqualityTesters?: unknown } }).expect.addEqualityTesters
) {
  (globalThis as unknown as { expect: Record<string, unknown> }).expect.addEqualityTesters =
    () => {};
}
