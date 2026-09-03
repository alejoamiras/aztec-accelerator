import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import type { AcceleratorStatus } from "@alejoamiras/aztec-accelerator";
import {
  AcceleratorStatusController,
  acceleratorStatusView,
  HttpSessionConsentController,
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

  test.each([
    ["https-disabled", "Encrypted Connection is disabled", false],
    ["tls-or-trust-failure", "Secure connection is not trusted", false],
    ["accelerator-reachable", "Accelerator is reachable", false],
    ["unconfirmed", "Secure connection unavailable", true],
  ] as const)("renders the %s secure recovery diagnosis", (diagnosis, title, showInstall) => {
    const view = acceleratorStatusView({
      available: false,
      reason: "secure-connection-unavailable",
      diagnosis,
    });
    expect(view.showSecureConnectionHelp).toBe(true);
    expect(view.showPermissionHelp).toBe(false);
    expect(view.secureConnectionTitle).toBe(title);
    expect(view.showInstall).toBe(showInstall);
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

  test("duplicate secure retry clicks share one forced refresh", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    let calls = 0;
    const controller = new AcceleratorStatusController({
      check: async (options) => {
        calls++;
        expect(options).toEqual({ forceRefresh: true });
        await gate;
        return { available: false, reason: "offline" };
      },
      render: () => {},
      setPending: () => {},
    });

    const first = controller.retrySecureConnection();
    const duplicate = controller.retrySecureConnection();
    expect(duplicate).toBe(first);
    expect(calls).toBe(1);
    release();
    await first;
    expect(calls).toBe(1);
  });
});

describe("HttpSessionConsentController", () => {
  test("cancellation leaves HTTPS-only configuration untouched", async () => {
    let configured = 0;
    let refreshed = 0;
    const openStates: boolean[] = [];
    const announcements: string[] = [];
    const controller = new HttpSessionConsentController({
      configure: () => configured++,
      refresh: async () => {
        refreshed++;
      },
      setConfirmationOpen: (open) => openStates.push(open),
      setPending: () => {},
      announce: (message) => announcements.push(message),
    });

    controller.request();
    controller.cancel();
    await controller.confirm();

    expect(openStates).toEqual([true, false]);
    expect(configured).toBe(0);
    expect(refreshed).toBe(0);
    expect(announcements.at(-1)).toContain("HTTPS-only proving remains enabled");
  });

  test("confirmation configures once, force-refreshes once, and announces accessibly", async () => {
    let configured = 0;
    let refreshed = 0;
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const openStates: boolean[] = [];
    const pendingStates: boolean[] = [];
    const announcements: string[] = [];
    const controller = new HttpSessionConsentController({
      configure: () => configured++,
      refresh: async () => {
        refreshed++;
        await gate;
      },
      setConfirmationOpen: (open) => openStates.push(open),
      setPending: (pending) => pendingStates.push(pending),
      announce: (message) => announcements.push(message),
    });

    controller.request();
    const first = controller.confirm();
    const duplicate = controller.confirm();
    expect(duplicate).toBe(first);
    await Promise.resolve();
    await Promise.resolve();
    expect(configured).toBe(1);
    expect(refreshed).toBe(1);
    release();
    await first;

    expect(openStates).toEqual([true, false]);
    expect(pendingStates).toEqual([true, false]);
    expect(announcements[0]).toContain("this tab");
    expect(announcements.at(-1)).toContain("this tab only");
  });

  test("a failed refresh announces that HTTP was enabled and clears pending state", async () => {
    const pendingStates: boolean[] = [];
    const announcements: string[] = [];
    const controller = new HttpSessionConsentController({
      configure: () => {},
      refresh: async () => {
        throw new Error("refresh failed");
      },
      setConfirmationOpen: () => {},
      setPending: (pending) => pendingStates.push(pending),
      announce: (message) => announcements.push(message),
    });

    controller.request();
    await expect(controller.confirm()).rejects.toThrow("refresh failed");

    expect(pendingStates).toEqual([true, false]);
    expect(announcements.at(-1)).toContain("HTTP is allowed for this tab");
    expect(announcements.at(-1)).toContain("refresh failed");
  });

  test("a failed configuration reports that HTTPS-only proving remains enabled", async () => {
    const announcements: string[] = [];
    const controller = new HttpSessionConsentController({
      configure: () => {
        throw new Error("configuration failed");
      },
      refresh: async () => {},
      setConfirmationOpen: () => {},
      setPending: () => {},
      announce: (message) => announcements.push(message),
    });

    controller.request();
    await expect(controller.confirm()).rejects.toThrow("configuration failed");

    expect(announcements.at(-1)).toContain("HTTP was not enabled");
    expect(announcements.at(-1)).toContain("HTTPS-only proving remains enabled");
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
