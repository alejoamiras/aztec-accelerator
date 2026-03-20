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

// ── Tests ──

test("page loads with correct initial state", async ({ page }) => {
  await mockServicesOffline(page);
  await page.goto("/");

  // Embedded UI is visible
  await expect(page.locator("#embedded-ui")).not.toHaveClass(/hidden/);

  // Accelerated mode button is active by default
  const accelBtn = page.locator("#mode-accelerated");
  await expect(accelBtn).toHaveClass(/mode-active/);

  // Local mode button is not active
  const localBtn = page.locator("#mode-local");
  await expect(localBtn).not.toHaveClass(/mode-active/);

  // Action buttons are disabled
  await expect(page.locator("#deploy-btn")).toBeDisabled();
  await expect(page.locator("#token-flow-btn")).toBeDisabled();
});

test("mode buttons toggle active class", async ({ page }) => {
  await mockServicesOffline(page);
  await page.goto("/");

  // Wait for init to settle
  await expect(page.locator("#log")).toContainText("Checking Aztec node");

  // Click Local
  await page.click("#mode-local");
  await expect(page.locator("#mode-local")).toHaveClass(/mode-active/);
  await expect(page.locator("#mode-accelerated")).not.toHaveClass(/mode-active/);

  // Click Accelerated
  await page.click("#mode-accelerated");
  await expect(page.locator("#mode-accelerated")).toHaveClass(/mode-active/);
  await expect(page.locator("#mode-local")).not.toHaveClass(/mode-active/);
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

  // Aztec dot should turn green
  await expect(page.locator("#aztec-status")).toHaveClass(/status-online/);
});

test("service dots show offline when Aztec node fails", async ({ page }) => {
  await mockServicesOffline(page);
  await page.goto("/");

  // Aztec dot should be red
  await expect(page.locator("#aztec-status")).toHaveClass(/status-offline/);
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
