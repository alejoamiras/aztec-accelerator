import { expect } from "bun:test";

// Patch expect for @aztec/foundation compatibility (twin of packages/sdk/src/test-setup.ts).
// @aztec/foundation's field module calls expect.addEqualityTesters (a vitest/jest API) whenever a
// global `expect` exists; bun:test's expect lacks it, which crashes any test that imports the real
// client stack — the live-node block in aztec.test.ts is the only such path (mocked unit tests
// never load it, so CI never sees this).
if (!(expect as any).addEqualityTesters) {
  (expect as any).addEqualityTesters = () => {};
}
if ((globalThis as any).expect && !(globalThis as any).expect.addEqualityTesters) {
  (globalThis as any).expect.addEqualityTesters = () => {};
}
