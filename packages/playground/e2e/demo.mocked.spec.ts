import { expect, test } from "@playwright/test";

// ── Helpers ──

/** Block all service requests so the app stays in "services unavailable" state. */
async function mockServicesOffline(page: import("@playwright/test").Page) {
  await page.route("**/aztec/status", (route) =>
    route.fulfill({ status: 503, body: "Service Unavailable" }),
  );
}

/** Mock Aztec node as healthy. Wallet init will still fail (no real Aztec node). */
async function mockServicesOnline(page: import("@playwright/test").Page) {
  await page.route("**/aztec/status", (route) => route.fulfill({ status: 200, body: "OK" }));
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
  await mockServicesOnline(page);

  // Block all further RPC calls so wallet init fails gracefully
  await page.route("**/aztec", (route) => {
    if (route.request().method() === "POST") {
      return route.fulfill({ status: 500, body: "not a real node" });
    }
    return route.continue();
  });

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
