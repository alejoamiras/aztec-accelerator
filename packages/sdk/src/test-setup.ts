import { expect } from "bun:test";

// Patch expect for @aztec/foundation compatibility
// @aztec/foundation checks if expect.addEqualityTesters exists (vitest API)
if (!(expect as any).addEqualityTesters) {
  (expect as any).addEqualityTesters = () => {};
}
if ((globalThis as any).expect && !(globalThis as any).expect.addEqualityTesters) {
  (globalThis as any).expect.addEqualityTesters = () => {};
}
