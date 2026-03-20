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
