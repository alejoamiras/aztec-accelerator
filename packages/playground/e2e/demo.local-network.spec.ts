/**
 * Comprehensive frontend E2E tests — runs against local Aztec network.
 *
 * On the local network (chain ID 31337), proofsRequired is automatically
 * false, so operations complete in seconds instead of minutes. This gives
 * us full UI coverage (deploy, token flow, mode switching) without the
 * cost of real proof generation.
 *
 * Usage: bun run --cwd packages/playground test:e2e:local-network
 */
import { expect, type Page, test } from "@playwright/test";
import { deployAndAssert, initSharedPage, runTokenFlowAndAssert } from "./fullstack.helpers";

const ACCELERATOR_URL = process.env.ACCELERATOR_URL || "";

let sharedPage: Page;

test.describe.configure({ mode: "serial" });

test.beforeAll(async ({ browser }) => {
  sharedPage = await initSharedPage(browser);
});

test.afterAll(async () => {
  if (sharedPage) await sharedPage.close();
});

// ── Accelerated proving (fastest — run first to minimize stale block headers on live networks) ──

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

  // TODO: re-enable when Aztec nightly WASM perf regression is resolved (token flow takes ~7 min on CI)
  test.skip("runs full token flow", async () => {
    const page = sharedPage;
    await expect(page.locator("#mode-accelerated")).toHaveClass(/mode-active/);
    await runTokenFlowAndAssert(page, "accelerated");
  });

  test("accelerated -> local deploys successfully", async () => {
    const page = sharedPage;
    await expect(page.locator("#mode-accelerated")).toHaveClass(/mode-active/);
    await page.click("#mode-local");
    await expect(page.locator("#mode-local")).toHaveClass(/mode-active/);
    await expect(page.locator("#log")).toContainText("Switched to local proving mode");
    await deployAndAssert(page, "local");
  });
});

// ── Local proving (slowest — run last) ──

test.describe("Local", () => {
  test("deploys account", async () => {
    const page = sharedPage;
    await page.click("#mode-local");
    await expect(page.locator("#mode-local")).toHaveClass(/mode-active/);
    await deployAndAssert(page, "local");
  });

  // TODO: re-enable when Aztec nightly WASM perf regression is resolved (token flow takes ~7 min on CI)
  test.skip("runs full token flow", async () => {
    const page = sharedPage;
    await expect(page.locator("#mode-local")).toHaveClass(/mode-active/);
    await runTokenFlowAndAssert(page, "local");
  });

  test("local -> accelerated deploys successfully", async () => {
    test.skip(!ACCELERATOR_URL, "ACCELERATOR_URL env var not set");
    const page = sharedPage;
    await expect(page.locator("#mode-local")).toHaveClass(/mode-active/);
    await page.click("#mode-accelerated");
    await expect(page.locator("#mode-accelerated")).toHaveClass(/mode-active/);
    await expect(page.locator("#log")).toContainText("Switched to accelerated proving mode");
    await deployAndAssert(page, "accelerated");
  });
});
