import { type Browser, type BrowserContext, expect, type Page, test } from "@playwright/test";

const PLAYGROUND_ORIGIN = "http://127.0.0.1:5173";
const LANDING_ORIGIN = "http://127.0.0.1:5174";
const HEALTH_URL = "http://127.0.0.1:59833/health";
const HEALTH_ADMIN = "http://127.0.0.1:59833";

async function resetHealthHits(): Promise<void> {
  const response = await fetch(`${HEALTH_ADMIN}/__reset`, { method: "POST" });
  expect(response.ok).toBe(true);
}

async function healthHits(): Promise<number> {
  const response = await fetch(`${HEALTH_ADMIN}/__hits`);
  return ((await response.json()) as { healthHits: number }).healthHits;
}

async function denyLocalNetwork(context: BrowserContext, origin: string): Promise<void> {
  // CDP's grant operation rejects every permission omitted from the list. An empty list is an
  // explicit deny override, not the browser's headless prompt default.
  await context.grantPermissions([], { origin });
}

async function grantLocalNetwork(context: BrowserContext, origin: string): Promise<void> {
  // Playwright 1.58 maps this umbrella permission to Chromium's legacy and split local/loopback
  // protocol permission names.
  await context.grantPermissions(["local-network-access"], { origin });
}

async function permissionState(page: Page): Promise<string> {
  return page.evaluate(async () => {
    const permissions = navigator.permissions as unknown as {
      query(descriptor: { name: string }): Promise<{ state: string }>;
    };
    try {
      return (await permissions.query({ name: "loopback-network" })).state;
    } catch {
      return (await permissions.query({ name: "local-network-access" })).state;
    }
  });
}

async function rawAnnotatedHealth(page: Page): Promise<boolean> {
  return page.evaluate(async (url) => {
    try {
      const response = await fetch(url, {
        targetAddressSpace: "loopback",
      } as RequestInit & { targetAddressSpace: "loopback" });
      const body = (await response.json()) as { status?: unknown; api_version?: unknown };
      return response.ok && body.status === "ok" && body.api_version === 1;
    } catch {
      return false;
    }
  }, HEALTH_URL);
}

async function mockPlaygroundNode(page: Page): Promise<void> {
  await page.route(`${PLAYGROUND_ORIGIN}/aztec`, (route) =>
    route.fulfill({ status: 503, body: "node intentionally absent from LNA harness" }),
  );
}

async function mockLandingExternals(page: Page): Promise<void> {
  await page.route(`${LANDING_ORIGIN}/releases/latest.json`, (route) =>
    route.fulfill({ status: 404, body: "not needed" }),
  );
  await page.route("https://api.github.com/**", (route) =>
    route.fulfill({ status: 404, body: "not needed" }),
  );
}

async function deniedContext(browser: Browser, origin: string): Promise<BrowserContext> {
  const context = await browser.newContext();
  await denyLocalNetwork(context, origin);
  return context;
}

test("harness proves a real denied and granted public-to-loopback fetch", async ({ browser }) => {
  await resetHealthHits();
  const context = await deniedContext(browser, PLAYGROUND_ORIGIN);
  const page = await context.newPage();
  await mockPlaygroundNode(page);
  await page.goto(PLAYGROUND_ORIGIN);
  await expect(page.locator("#accelerator-permission-help")).toBeVisible();

  const capabilities = await page.evaluate(() => ({
    secure: window.isSecureContext,
    targetAddressSpace: "targetAddressSpace" in Request.prototype,
  }));
  expect(capabilities).toEqual({ secure: true, targetAddressSpace: true });
  expect(await permissionState(page)).toBe("denied");
  expect(await healthHits()).toBe(0);

  expect(await rawAnnotatedHealth(page)).toBe(false);
  expect(await healthHits()).toBe(0);

  await grantLocalNetwork(context, PLAYGROUND_ORIGIN);
  expect(await permissionState(page)).toBe("granted");
  await expect(page.locator("#accelerator-secure-help")).toBeVisible();
  const automaticHits = await healthHits();
  expect(automaticHits).toBeGreaterThan(0);
  expect(await rawAnnotatedHealth(page)).toBe(true);
  expect(await healthHits()).toBe(automaticHits + 1);
  await page.locator("#accelerator-use-http").click();
  await page.locator("#http-session-confirm").click();
  await expect(page.locator("#accelerator-label")).toHaveText("available");
  await context.close();
});

test("playground denial gives guidance and same-context grant automatically recovers", async ({
  browser,
}) => {
  await resetHealthHits();
  const context = await deniedContext(browser, PLAYGROUND_ORIGIN);
  const page = await context.newPage();
  await mockPlaygroundNode(page);
  await page.goto(PLAYGROUND_ORIGIN);

  await expect(page.locator("#accelerator-label")).toHaveText("local access blocked");
  await expect(page.locator("#accelerator-permission-help")).toBeVisible();
  await expect(page.locator("#accel-banner")).toBeHidden();
  await expect(page.locator("#accelerator-cta")).toBeHidden();
  expect(await healthHits()).toBe(0);

  await grantLocalNetwork(context, PLAYGROUND_ORIGIN);
  await expect(page.locator("#accelerator-secure-help")).toBeVisible();
  await page.locator("#accelerator-use-http").click();
  await page.locator("#http-session-confirm").click();
  await expect(page.locator("#accelerator-label")).toHaveText("available");
  await expect(page.locator("#accelerator-permission-help")).toBeHidden();
  await expect(page.locator("#accelerator-status")).toHaveAttribute("data-status", "online");
  expect(await healthHits()).toBeGreaterThan(0);
  await context.close();
});

test("landing denial suppresses download and same-context grant automatically recovers", async ({
  browser,
}) => {
  await resetHealthHits();
  const context = await deniedContext(browser, LANDING_ORIGIN);
  const page = await context.newPage();
  await mockLandingExternals(page);
  await page.goto(LANDING_ORIGIN);

  await expect(page.locator("#landing-permission-help")).toBeVisible();
  await expect(page.locator("#download-actions")).toBeHidden();
  expect(await permissionState(page)).toBe("denied");
  expect(await healthHits()).toBe(0);

  await grantLocalNetwork(context, LANDING_ORIGIN);
  await expect(page.locator("#landing-permission-help")).toBeHidden();
  await expect(page.locator("#landing-secure-help")).toBeVisible();
  await expect(page.locator("#download-actions")).toBeVisible();
  await expect(page.locator("#landing-secure-title")).toHaveText("Accelerator is reachable");
  expect(await healthHits()).toBeGreaterThan(0);
  await context.close();
});

test("playground automatically recovers when an open prompt is allowed after probe timeout", async ({
  browser,
}) => {
  await resetHealthHits();
  const context = await browser.newContext();
  const page = await context.newPage();
  await mockPlaygroundNode(page);
  await page.goto(PLAYGROUND_ORIGIN);

  // The prompt remains unresolved longer than both bounded SDK rounds. This used to settle the UI as
  // offline permanently even after the browser later changed the permission to granted.
  await expect(page.locator("#accelerator-secure-retry")).toBeEnabled({ timeout: 15_000 });
  await expect(page.locator("#accelerator-label")).toContainText("secure connection unavailable");
  expect(await permissionState(page)).toBe("prompt");
  expect(await healthHits()).toBe(0);

  await grantLocalNetwork(context, PLAYGROUND_ORIGIN);
  await expect(page.locator("#accelerator-secure-help")).toBeVisible();
  await page.locator("#accelerator-use-http").click();
  await page.locator("#http-session-confirm").click();
  await expect(page.locator("#accelerator-label")).toHaveText("available");
  await expect(page.locator("#accelerator-status")).toHaveAttribute("data-status", "online");
  expect(await healthHits()).toBeGreaterThan(0);
  await context.close();
});

test("landing automatically renders blocked guidance when an open prompt is denied after timeout", async ({
  browser,
}) => {
  await resetHealthHits();
  const context = await browser.newContext();
  const page = await context.newPage();
  await mockLandingExternals(page);
  await page.goto(LANDING_ORIGIN);

  await expect(page.locator("#landing-secure-retry")).toBeEnabled({ timeout: 10_000 });
  expect(await permissionState(page)).toBe("prompt");
  expect(await healthHits()).toBe(0);

  await denyLocalNetwork(context, LANDING_ORIGIN);
  await expect(page.locator("#landing-permission-help")).toBeVisible();
  await expect(page.locator("#download-actions")).toBeHidden();
  expect(await healthHits()).toBe(0);
  await context.close();
});
