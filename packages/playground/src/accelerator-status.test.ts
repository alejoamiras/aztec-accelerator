import { describe, expect, test } from "bun:test";
import type { AcceleratorStatus } from "@alejoamiras/aztec-accelerator";
import { AcceleratorStatusController, acceleratorStatusView } from "./accelerator-status";

describe("acceleratorStatusView", () => {
  test("renders every unavailable reason without contradictory install UI", () => {
    const offline = acceleratorStatusView({ available: false, reason: "offline" });
    expect(offline.showInstall).toBe(true);

    for (const status of [
      { available: false, reason: "permission-blocked" },
      { available: false, reason: "error", protocol: "http" },
      {
        available: false,
        reason: "version-mismatch",
        acceleratorVersion: "4.0.0",
        protocol: "https",
      },
    ] satisfies AcceleratorStatus[]) {
      const view = acceleratorStatusView(status);
      expect(view.connected).toBe(false);
      expect(view.showInstall).toBe(false);
    }

    expect(
      acceleratorStatusView({ available: false, reason: "permission-blocked" }).showPermissionHelp,
    ).toBe(true);
  });
});

describe("AcceleratorStatusController", () => {
  test("a stale result cannot overwrite a newer forced refresh", async () => {
    let resolveFirst!: (status: AcceleratorStatus) => void;
    const first = new Promise<AcceleratorStatus>((resolve) => {
      resolveFirst = resolve;
    });
    const recovered: AcceleratorStatus = {
      available: true,
      needsDownload: false,
      protocol: "http",
    };
    let calls = 0;
    const rendered: AcceleratorStatus[] = [];
    const controller = new AcceleratorStatusController({
      check: async () => (++calls === 1 ? first : recovered),
      render: (status) => rendered.push(status),
      setPending: () => {},
    });

    const startup = controller.refresh();
    await controller.refresh({ forceRefresh: true });
    resolveFirst({ available: false, reason: "offline" });
    await startup;

    expect(rendered).toEqual([recovered]);
    expect(controller.displayed).toEqual(recovered);
  });

  test("proof fallback refresh is coalesced and only starts after available was displayed", async () => {
    let releases = 0;
    let resolveRefresh!: (status: AcceleratorStatus) => void;
    const available: AcceleratorStatus = {
      available: true,
      needsDownload: false,
      protocol: "https",
    };
    let calls = 0;
    const controller = new AcceleratorStatusController({
      check: async () => {
        calls++;
        if (calls === 1) return available;
        return new Promise<AcceleratorStatus>((resolve) => {
          resolveRefresh = (status) => {
            releases++;
            resolve(status);
          };
        });
      },
      render: () => {},
      setPending: () => {},
    });

    controller.refreshAfterFallback();
    expect(calls).toBe(0);
    await controller.refresh();
    controller.refreshAfterFallback();
    controller.refreshAfterFallback();
    await Promise.resolve();
    expect(calls).toBe(2);
    resolveRefresh({ available: false, reason: "offline" });
    await Promise.resolve();
    await Promise.resolve();
    expect(releases).toBe(1);
  });
});
