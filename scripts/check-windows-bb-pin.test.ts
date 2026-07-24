import { afterEach, describe, expect, test } from "bun:test";
import { resolveAztecBb } from "../packages/accelerator/scripts/copy-bb.ts";
import { checkWindowsBbPin } from "./check-windows-bb-pin.ts";

// F-008: the pin-status check must resolve the LIVE bb.js version and NEVER touch the network.
describe("check-windows-bb-pin", () => {
  const origFetch = globalThis.fetch;
  afterEach(() => {
    globalThis.fetch = origFetch;
  });

  test("reports the live bb.js version's pin as present, with no network", () => {
    globalThis.fetch = (() => {
      throw new Error("check-windows-bb-pin must not fetch");
      // via `unknown`: a throw-only stub deliberately doesn't implement fetch's full surface
      // (e.g. `preconnect`) — the point is that calling it at all fails the test.
    }) as unknown as typeof fetch;
    const { version, present, message } = checkWindowsBbPin();
    expect(version).toBe(resolveAztecBb().version); // keys on the gate's version, not argv
    expect(present).toBe(true); // the committed live version has a manual-review pin
    expect(message).toContain("present");
  });

  test("an unpinned version reports MANUAL PIN REQUIRED, with no network", () => {
    globalThis.fetch = (() => {
      throw new Error("check-windows-bb-pin must not fetch");
      // via `unknown`: a throw-only stub deliberately doesn't implement fetch's full surface
      // (e.g. `preconnect`) — the point is that calling it at all fails the test.
    }) as unknown as typeof fetch;
    const { present, message } = checkWindowsBbPin("9.9.9");
    expect(present).toBe(false);
    expect(message).toContain("MANUAL PIN REQUIRED");
  });
});
