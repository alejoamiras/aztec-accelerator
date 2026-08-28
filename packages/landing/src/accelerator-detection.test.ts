import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import {
  detectAccelerator,
  type LandingAcceleratorStatus,
  LandingDetectionController,
  watchLoopbackPermissionChanges,
} from "./accelerator-detection";

describe("landing accelerator detection", () => {
  let originalFetch: typeof fetch;
  let targetDescriptor: PropertyDescriptor | undefined;
  let permissionsDescriptor: PropertyDescriptor | undefined;

  beforeEach(() => {
    originalFetch = globalThis.fetch;
    targetDescriptor = Object.getOwnPropertyDescriptor(Request.prototype, "targetAddressSpace");
    permissionsDescriptor = Object.getOwnPropertyDescriptor(navigator, "permissions");
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
    if (targetDescriptor) {
      Object.defineProperty(Request.prototype, "targetAddressSpace", targetDescriptor);
    } else {
      delete (Request.prototype as Request & { targetAddressSpace?: string }).targetAddressSpace;
    }
    if (permissionsDescriptor) {
      Object.defineProperty(navigator, "permissions", permissionsDescriptor);
    } else {
      Reflect.deleteProperty(navigator, "permissions");
    }
  });

  const permissions = (
    query: (descriptor: { name: string }) => Promise<{ state: PermissionState }>,
  ) => Object.defineProperty(navigator, "permissions", { configurable: true, value: { query } });

  test("accepts only the exact health identity", async () => {
    globalThis.fetch = mock(async () => Response.json({ status: "ok", api_version: 1 })) as any;
    expect(await detectAccelerator()).toBe("available");

    for (const body of [
      { status: "ok" },
      { api_version: 1 },
      { status: "ok", api_version: 2 },
      { status: false, api_version: 1 },
      [],
    ]) {
      globalThis.fetch = mock(async () => Response.json(body)) as any;
      expect(await detectAccelerator()).toBe("error");
    }
  });

  test("annotates HTTP only when supported and never annotates HTTPS", async () => {
    Object.defineProperty(Request.prototype, "targetAddressSpace", {
      configurable: true,
      get: () => undefined,
    });
    const seen: Array<{ url: string; annotation?: string }> = [];
    globalThis.fetch = mock(async (input: string | URL | Request, init?: RequestInit) => {
      seen.push({
        url: String(input),
        annotation: (init as RequestInit & { targetAddressSpace?: string })?.targetAddressSpace,
      });
      return Response.json({ status: "ok", api_version: 1 });
    }) as any;
    await detectAccelerator();
    expect(seen).toEqual([
      { url: "http://127.0.0.1:59833/health", annotation: "loopback" },
      { url: "https://127.0.0.1:59834/health", annotation: undefined },
    ]);
  });

  test("mirrors modern then legacy permission lookup and classifies only denied", async () => {
    const names: string[] = [];
    permissions(async ({ name }) => {
      names.push(name);
      if (name === "loopback-network") throw new TypeError("unsupported");
      return { state: "denied" };
    });
    globalThis.fetch = mock(async () => {
      throw new TypeError("blocked");
    }) as any;
    expect(await detectAccelerator()).toBe("permission-blocked");
    expect(names).toEqual(["loopback-network", "local-network-access"]);

    names.length = 0;
    permissions(async ({ name }) => {
      names.push(name);
      return { state: "prompt" };
    });
    expect(await detectAccelerator()).toBe("offline");
    expect(names).toEqual(["loopback-network"]);
  });

  test("httpsOnly never constructs the HTTP candidate", async () => {
    const urls: string[] = [];
    globalThis.fetch = mock(async (input: string | URL | Request) => {
      urls.push(String(input));
      return Response.json({ status: "ok", api_version: 1 });
    }) as any;
    expect(await detectAccelerator({ httpsOnly: true })).toBe("available");
    expect(urls).toEqual(["https://127.0.0.1:59834/health"]);
  });

  test("watches a delayed prompt decision and removes the listener on cleanup", async () => {
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
});

test("landing Retry is uncached and stale results cannot overwrite recovery", async () => {
  let resolveFirst!: (status: LandingAcceleratorStatus) => void;
  const first = new Promise<LandingAcceleratorStatus>((resolve) => {
    resolveFirst = resolve;
  });
  let calls = 0;
  const rendered: LandingAcceleratorStatus[] = [];
  const controller = new LandingDetectionController(
    async () => (++calls === 1 ? first : "available"),
    (status) => rendered.push(status),
    () => {},
  );
  const startup = controller.refresh();
  await controller.refresh();
  resolveFirst("permission-blocked");
  await startup;
  expect(calls).toBe(2);
  expect(rendered).toEqual(["available"]);
});

test("landing permission refresh starts an uncached probe and owns the display epoch", async () => {
  let resolveFirst!: (status: LandingAcceleratorStatus) => void;
  const first = new Promise<LandingAcceleratorStatus>((resolve) => {
    resolveFirst = resolve;
  });
  let calls = 0;
  const rendered: LandingAcceleratorStatus[] = [];
  const controller = new LandingDetectionController(
    async () => (++calls === 1 ? first : "permission-blocked"),
    (status) => rendered.push(status),
    () => {},
  );

  const startup = controller.refresh();
  await controller.refreshAfterPermissionChange();
  resolveFirst("offline");
  await startup;

  expect(calls).toBe(2);
  expect(rendered).toEqual(["permission-blocked"]);
});
