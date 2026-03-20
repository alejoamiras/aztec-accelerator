/**
 * Deploy-only smoke tests — runs against testnet with real proofs.
 *
 * 2 tests: one deploy per mode (accelerated, local). No token flow,
 * no mode switching — just verifies that each proving mode can deploy
 * an account successfully. Used by deploy pipelines.
 *
 * Usage: bun run --cwd packages/playground test:e2e:smoke
 */
import { expect, type Page, test } from "@playwright/test";
import { deployAndAssert, initSharedPage } from "./fullstack.helpers";

const ACCELERATOR_URL = process.env.ACCELERATOR_URL || "";

let sharedPage: Page;

test.describe.configure({ mode: "serial" });

test.beforeAll(async ({ browser }) => {
  sharedPage = await initSharedPage(browser);
});

test.afterAll(async () => {
  if (sharedPage) await sharedPage.close();
});

// ── Accelerated ──

test.describe("Accelerated", () => {
  test.beforeEach(() => {
    test.skip(!ACCELERATOR_URL, "ACCELERATOR_URL env var not set");
  });

  test("deploys account", async () => {
    const page = sharedPage;
    await page.click("#mode-accelerated");
    await expect(page.locator("#mode-accelerated")).toHaveClass(/mode-active/);
    await deployAndAssert(page, "accelerated");
  });
});

// ── Local ──

test.describe("Local", () => {
  test("deploys account", async () => {
    const page = sharedPage;
    await page.click("#mode-local");
    await expect(page.locator("#mode-local")).toHaveClass(/mode-active/);
    await deployAndAssert(page, "local");
  });
});
