import { expect, type Page } from "@playwright/test";
import { assertServicesAvailable } from "./fullstack.fixture";

/**
 * Shared helpers for fullstack E2E tests (local-network, smoke).
 * Adapted from tee-rex for the accelerator's 2-mode architecture.
 */

/**
 * Initialize a shared page with wallet ready. Retries up to 5 times
 * to work around Aztec PXE IndexedDB flakiness in real browsers.
 */
export async function initSharedPage(browser: { newPage: () => Promise<Page> }): Promise<Page> {
  await assertServicesAvailable();

  const page = await browser.newPage();

  page.on("console", (msg) => {
    const text = msg.text();
    if (msg.type() === "error" || msg.type() === "warning") {
      console.log(`[browser:${msg.type()}] ${text}`);
    }
  });
  page.on("pageerror", (err) => {
    console.log(`[browser:pageerror] ${err.message}`);
  });

  const MAX_INIT_ATTEMPTS = 5;
  for (let attempt = 1; attempt <= MAX_INIT_ATTEMPTS; attempt++) {
    await page.goto("/");

    if (attempt > 1) {
      await page.evaluate(async () => {
        const dbs = await indexedDB.databases();
        await Promise.all(
          dbs.filter((db) => db.name).map((db) => indexedDB.deleteDatabase(db.name!)),
        );
      });
      await page.reload();
    }

    const walletState = page.locator("#wallet-state");
    await expect(walletState).not.toHaveText("not initialized", { timeout: 30_000 });
    await expect(walletState).not.toHaveText("initializing...", { timeout: 3 * 60 * 1000 });

    const text = await walletState.textContent();
    if (text === "ready") break;

    if (attempt === MAX_INIT_ATTEMPTS) {
      throw new Error(`Wallet initialization failed after ${MAX_INIT_ATTEMPTS} attempts`);
    }
  }

  await expect(page.locator("#deploy-btn")).toBeEnabled();
  await expect(page.locator("#token-flow-btn")).toBeEnabled();

  return page;
}

/** Deploy a test account and assert all UI state transitions. */
export async function deployAndAssert(page: Page, mode: "local" | "accelerated"): Promise<void> {
  await page.click("#deploy-btn");

  await expect(page.locator("#progress")).not.toHaveClass(/hidden/);
  await expect(page.locator("#deploy-btn")).toHaveText("Proving...");

  await expect(page.locator("#deploy-btn")).toHaveText("Deploy Test Account", {
    timeout: 10 * 60 * 1000,
  });

  const deployLog = await page.locator("#log").textContent();
  expect(deployLog, "Deploy should not have failed — check browser console above").not.toContain(
    "Deploy failed:",
  );

  await expect(page.locator("#progress")).toHaveClass(/hidden/);
  await expect(page.locator("#results")).not.toHaveClass(/hidden/);

  const timeText = await page.locator(`#time-${mode}`).textContent();
  expect(timeText).not.toBe("—");
  expect(timeText).toMatch(/^\d+\.\d+s$/);

  await expect(page.locator(`#result-${mode}`)).toHaveClass(/result-filled/);
  await expect(page.locator("#log")).toContainText("step breakdown");

  const steps = page.locator(`#steps-${mode}`);
  await expect(steps).not.toHaveClass(/hidden/);
  await expect(steps.locator("details")).toHaveCount(1);
  await expect(steps.locator("details summary")).toContainText("steps");

  await expect(page.locator("#deploy-btn")).toBeEnabled();
  await expect(page.locator("#token-flow-btn")).toBeEnabled();
}

/** Run token flow and assert all UI state transitions. */
export async function runTokenFlowAndAssert(
  page: Page,
  mode: "local" | "accelerated",
): Promise<void> {
  await page.click("#token-flow-btn");

  await expect(page.locator("#progress")).not.toHaveClass(/hidden/);
  await expect(page.locator("#token-flow-btn")).toHaveText("Running...");

  await expect(page.locator("#token-flow-btn")).toHaveText("Run Token Flow", {
    timeout: 10 * 60 * 1000,
  });

  const flowLog = await page.locator("#log").textContent();
  expect(flowLog, "Token flow should not have failed — check browser console above").not.toContain(
    "Token flow failed:",
  );

  await expect(page.locator("#progress")).toHaveClass(/hidden/);
  await expect(page.locator("#results")).not.toHaveClass(/hidden/);

  const timeText = await page.locator(`#time-${mode}`).textContent();
  expect(timeText).not.toBe("—");
  expect(timeText).toMatch(/^\d+\.\d+s$/);

  await expect(page.locator(`#tag-${mode}`)).toHaveText("token flow");

  await expect(page.locator("#log")).toContainText("step breakdown");
  await expect(page.locator("#log")).toContainText("Token flow complete");
  await expect(page.locator("#log")).toContainText("Alice: 500");
  await expect(page.locator("#log")).toContainText("Bob: 500");

  const steps = page.locator(`#steps-${mode}`);
  await expect(steps).not.toHaveClass(/hidden/);
  await expect(steps.locator("details")).toHaveCount(1);
  await expect(steps.locator("details summary")).toContainText("steps");

  await expect(page.locator("#deploy-btn")).toBeEnabled();
  await expect(page.locator("#token-flow-btn")).toBeEnabled();
}
