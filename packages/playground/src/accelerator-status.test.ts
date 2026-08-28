import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import type { AcceleratorStatus } from "@alejoamiras/aztec-accelerator";
import {
  AcceleratorStatusController,
  acceleratorStatusView,
  watchLoopbackPermissionChanges,
} from "./accelerator-status";

let permissionsDescriptor: PropertyDescriptor | undefined;

beforeEach(() => {
  permissionsDescriptor = Object.getOwnPropertyDescriptor(navigator, "permissions");
});

afterEach(() => {
  if (permissionsDescriptor) {
    Object.defineProperty(navigator, "permissions", permissionsDescriptor);
  } else {
    Reflect.deleteProperty(navigator, "permissions");
  }
});

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

  test("permission change waits out the old SDK single-flight before forcing a fresh probe", async () => {
    let resolveStartup!: (status: AcceleratorStatus) => void;
    const startupStatus = new Promise<AcceleratorStatus>((resolve) => {
      resolveStartup = resolve;
    });
    const available: AcceleratorStatus = {
      available: true,
      needsDownload: false,
      protocol: "https",
    };
    let calls = 0;
    const rendered: AcceleratorStatus[] = [];
    const controller = new AcceleratorStatusController({
      check: async () => (++calls === 1 ? startupStatus : available),
      render: (status) => rendered.push(status),
      setPending: () => {},
    });

    const startup = controller.refresh();
    const permissionRefresh = controller.refreshAfterPermissionChange();
    await Promise.resolve();
    expect(calls).toBe(1);

    resolveStartup({ available: false, reason: "offline" });
    await permissionRefresh;
    await startup;

    expect(calls).toBe(2);
    expect(rendered).toEqual([available]);
  });
});

test("permission watcher uses the modern descriptor and stops cleanly", async () => {
  let state: PermissionState = "prompt";
  const status = new EventTarget() as EventTarget & { readonly state: PermissionState };
  Object.defineProperty(status, "state", { get: () => state });
  const names: string[] = [];
  Object.defineProperty(navigator, "permissions", {
    configurable: true,
    value: {
      query: async ({ name }: { name: string }) => {
        names.push(name);
        return status;
      },
    },
  });
  const seen: PermissionState[] = [];

  const stop = await watchLoopbackPermissionChanges((next) => seen.push(next));
  state = "denied";
  status.dispatchEvent(new Event("change"));
  stop();
  state = "granted";
  status.dispatchEvent(new Event("change"));

  expect(names).toEqual(["loopback-network"]);
  expect(seen).toEqual(["denied"]);
});
