import { expect, test } from "@playwright/test";

// ── Helpers ──

// The app's node health check is the node_getNodeInfo JSON-RPC POST to /aztec
// (5.0.0 nodes 405 a plain GET /status, so there is no /status probe anymore).

/** Block the node RPC so the app stays in "services unavailable" state. */
async function mockServicesOffline(page: import("@playwright/test").Page) {
  await page.route("**/aztec", (route) =>
    route.fulfill({ status: 503, body: "Service Unavailable" }),
  );
}

/**
 * Answer the health probe (node_getNodeInfo) as healthy; every other RPC 500s
 * so wallet init fails gracefully (there is no real node behind the mock).
 */
async function mockServicesOnline(page: import("@playwright/test").Page) {
  await page.route("**/aztec", (route) => {
    const req = route.request();
    if (req.method() === "POST" && req.postData()?.includes("node_getNodeInfo")) {
      return route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ jsonrpc: "2.0", id: 1, result: { nodeVersion: "5.0.0" } }),
      });
    }
    return route.fulfill({ status: 500, body: "not a real node" });
  });
}

const HEALTHY = JSON.stringify({ status: "ok", api_version: 1 });

async function mockHealth(
  page: import("@playwright/test").Page,
  handler: (route: import("@playwright/test").Route) => Promise<void> | void,
) {
  await page.route("http://127.0.0.1:59833/health", handler);
  await page.route("https://127.0.0.1:59834/health", handler);
}

async function mockPermissionState(
  page: import("@playwright/test").Page,
  initial: "denied" | "prompt" | "granted",
) {
  await page.addInitScript((state) => {
    const target = window as typeof window & { __mockLnaPermission?: string };
    target.__mockLnaPermission = state;
    Object.defineProperty(navigator, "permissions", {
      configurable: true,
      value: {
        query: async () => ({ state: target.__mockLnaPermission }),
      },
    });
  }, initial);
}

// ── JS error safety net — catches runtime errors across all mocked tests ──

const jsErrors: string[] = [];

test.beforeEach(async ({ page }) => {
  jsErrors.length = 0;
  page.on("pageerror", (err) => jsErrors.push(err.message));
});

test.afterEach(() => {
  expect(jsErrors, "Unexpected JS runtime errors").toEqual([]);
});

// ── Tests ──
// Assertions use data-* attributes (data-active, data-status) instead of CSS
// classes, so design refactors don't break tests.

test("page loads with correct initial state", async ({ page }) => {
  await mockServicesOffline(page);
  await page.goto("/");

  // Embedded UI is visible (wait for init to complete — accelerator health check has 2s timeout)
  await expect(page.locator("#embedded-ui")).toBeVisible({ timeout: 10000 });

  // Accelerated mode button is active by default
  await expect(page.locator("#mode-accelerated")).toHaveAttribute("data-active", "true");
  await expect(page.locator("#mode-local")).toHaveAttribute("data-active", "false");

  // Action buttons are disabled
  await expect(page.locator("#deploy-btn")).toBeDisabled();
  await expect(page.locator("#token-flow-btn")).toBeDisabled();
});

test("mode buttons toggle active state", async ({ page }) => {
  await mockServicesOffline(page);
  await page.goto("/");
  await expect(page.locator("#log")).toContainText("Checking Aztec node");

  // Click Local
  await page.click("#mode-local");
  await expect(page.locator("#mode-local")).toHaveAttribute("data-active", "true");
  await expect(page.locator("#mode-accelerated")).toHaveAttribute("data-active", "false");

  // Click Accelerated
  await page.click("#mode-accelerated");
  await expect(page.locator("#mode-accelerated")).toHaveAttribute("data-active", "true");
  await expect(page.locator("#mode-local")).toHaveAttribute("data-active", "false");
});

test("service dots show online when Aztec node responds OK", async ({ page }) => {
  // Health probe answers; every other RPC 500s so wallet init fails gracefully.
  await mockServicesOnline(page);
  await page.goto("/");

  // Aztec dot should be online
  await expect(page.locator("#aztec-status")).toHaveAttribute("data-status", "online");
});

test("service dots show offline when Aztec node fails", async ({ page }) => {
  await mockServicesOffline(page);
  await page.goto("/");

  // Aztec dot should be offline
  await expect(page.locator("#aztec-status")).toHaveAttribute("data-status", "offline");
});

test("log panel shows checking Aztec node message on load", async ({ page }) => {
  await mockServicesOffline(page);
  await page.goto("/");

  await expect(page.locator("#log")).toContainText("Checking Aztec node");
});

test("accelerator status is shown in services panel", async ({ page }) => {
  await mockServicesOffline(page);
  await page.goto("/");

  await expect(page.locator("#accelerator-status")).toBeVisible();
  await expect(page.locator("#accelerator-label")).toBeVisible();
});

test("recognized health renders available and suppresses install UI", async ({ page }) => {
  await mockServicesOffline(page);
  await mockHealth(page, (route) =>
    route.fulfill({ status: 200, contentType: "application/json", body: HEALTHY }),
  );
  await page.goto("/");

  await expect(page.locator("#accelerator-label")).toHaveText("running");
  await expect(page.locator("#accelerator-status")).toHaveAttribute("data-status", "online");
  await expect(page.locator("#accel-banner")).toBeHidden();
  await expect(page.locator("#accelerator-cta")).toBeHidden();
});

test("an unconfirmed secure connection keeps install guidance and shows recovery", async ({
  page,
}) => {
  await mockServicesOffline(page);
  await mockPermissionState(page, "prompt");
  await mockHealth(page, (route) => route.abort());
  await page.goto("/");

  await expect(page.locator("#accelerator-label")).toContainText("secure connection unavailable");
  await expect(page.locator("#accelerator-permission-help")).toBeHidden();
  await expect(page.locator("#accelerator-secure-help")).toBeVisible();
  await expect(page.locator("#accel-banner")).toBeVisible();
  await expect(page.locator("#accelerator-cta")).toBeVisible();
});

test("health error and version mismatch do not offer a contradictory install", async ({ page }) => {
  await mockServicesOffline(page);
  await mockHealth(page, (route) => route.fulfill({ status: 500, body: "error" }));
  await page.goto("/");
  await expect(page.locator("#accelerator-label")).toContainText("health check error");
  await expect(page.locator("#accel-banner")).toBeHidden();
  await expect(page.locator("#accelerator-cta")).toBeHidden();

  await page.unroute("http://127.0.0.1:59833/health");
  await page.unroute("https://127.0.0.1:59834/health");
  await mockHealth(page, (route) =>
    route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ status: "ok", api_version: 1, aztec_version: "0.0.0" }),
    }),
  );
  await page.reload();
  await expect(page.locator("#accelerator-label")).toContainText("version mismatch");
  await expect(page.locator("#accel-banner")).toBeHidden();
  await expect(page.locator("#accelerator-cta")).toBeHidden();
});

test("permission-blocked guidance recovers through immediate Retry", async ({ page }) => {
  await mockServicesOffline(page);
  await mockPermissionState(page, "denied");
  let blocked = true;
  await mockHealth(page, async (route) => {
    if (blocked) return route.abort();
    await new Promise((resolve) => setTimeout(resolve, 100));
    return route.fulfill({ status: 200, contentType: "application/json", body: HEALTHY });
  });
  await page.goto("/");

  await expect(page.locator("#accelerator-label")).toHaveText("local access blocked");
  await expect(page.locator("#accelerator-permission-help")).toBeVisible();
  await expect(page.locator("#accel-banner")).toBeHidden();
  await expect(page.locator("#accelerator-cta")).toBeHidden();

  blocked = false;
  await page.evaluate(() => {
    (window as typeof window & { __mockLnaPermission?: string }).__mockLnaPermission = "granted";
  });
  await page.locator("#accelerator-permission-retry").click();
  await expect(page.locator("#accelerator-permission-retry")).toBeDisabled();
  await expect(page.locator("#accelerator-label")).toHaveText("running");
  await expect(page.locator("#accelerator-permission-help")).toBeHidden();
});

test("HTTP recovery requires confirmation and resets on reload", async ({ page }) => {
  await mockServicesOffline(page);
  await mockPermissionState(page, "granted");
  const detailedHealth = JSON.stringify({
    status: "ok",
    api_version: 1,
    version: "3.0.0",
    aztec_version: "5.2.0",
    available_versions: ["5.2.0"],
    bb_available: true,
  });
  const plaintextProofRequests: string[] = [];
  page.on("request", (request) => {
    if (request.url() === "http://127.0.0.1:59833/prove") {
      plaintextProofRequests.push(request.url());
    }
  });
  await page.route("https://127.0.0.1:59834/health", (route) => route.abort());
  await page.route("http://127.0.0.1:59833/health", (route) =>
    route.fulfill({ status: 200, contentType: "application/json", body: detailedHealth }),
  );
  await page.goto("/");

  await expect(page.locator("#accelerator-secure-title")).toHaveText(
    "Encrypted Connection is disabled",
  );
  await expect(page.locator("#accelerator-secure-help")).toBeVisible();
  expect(plaintextProofRequests).toEqual([]);
  const storageBefore = await page.evaluate(() => ({ ...localStorage }));
  const cookieBefore = await page.evaluate(() => document.cookie);
  const urlBefore = page.url();

  await page.locator("#accelerator-use-http").click();
  await expect(page.locator("#http-session-confirmation")).toBeVisible();
  await expect(page.locator("#http-session-cancel")).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(page.locator("#http-session-confirm")).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.locator("#http-session-cancel")).toBeFocused();
  await expect(page.locator("#http-session-warning")).toHaveText(
    "HTTP can expose private proving data to another local user or process. Use it only if you accept this risk for the current tab.",
  );
  await page.locator("#http-session-cancel").click();
  await expect(page.locator("#http-session-confirmation")).toBeHidden();
  await expect(page.locator("#accelerator-service-status")).toBeFocused();
  await expect(page.locator("#accelerator-label")).toContainText("secure connection unavailable");

  await page.locator("#accelerator-use-http").click();
  await page.locator("#http-session-confirm").click();
  await expect(page.locator("#accelerator-label")).toHaveText("running");
  await expect(page.locator("#accelerator-secure-help")).toBeHidden();
  await expect(page.locator("#accelerator-service-status")).toBeFocused();
  await expect(page.locator("#accelerator-recovery-announcement")).toContainText("this tab only");
  expect(await page.evaluate(() => ({ ...localStorage }))).toEqual(storageBefore);
  expect(await page.evaluate(() => document.cookie)).toBe(cookieBefore);
  expect(page.url()).toBe(urlBefore);

  await page.reload();
  await expect(page.locator("#accelerator-secure-help")).toBeVisible();
  await expect(page.locator("#accelerator-label")).toContainText("secure connection unavailable");
  expect(plaintextProofRequests).toEqual([]);
});

// ── Expanded coverage ──

test("mode switch logs the change", async ({ page }) => {
  await mockServicesOffline(page);
  await page.goto("/");
  await expect(page.locator("#log")).toContainText("Checking Aztec node");

  // Switch to WASM mode
  await page.click("#mode-local");
  await expect(page.locator("#log")).toContainText("Proving mode");
});

test("node error appears in log panel", async ({ page }) => {
  await mockServicesOffline(page);
  await page.goto("/");

  // The log should show an error about the Aztec node not being reachable
  await expect(page.locator("#log")).toContainText("not reachable", {
    timeout: 5000,
  });
});
