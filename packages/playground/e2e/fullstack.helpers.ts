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
  // The token flow needs a session-deployed sender, so its button stays disabled
  // until the first deploy of the session (ensureSessionAccount handles that).
  await expect(page.locator("#token-flow-btn")).toBeDisabled();

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
  await expect(page.locator("#log")).toContainText("total:");

  const steps = page.locator(`#steps-${mode}`);
  await expect(steps).not.toHaveClass(/hidden/);
  await expect(steps.locator("details")).toHaveCount(1);
  await expect(steps.locator("details summary")).toContainText("steps");

  await expect(page.locator("#deploy-btn")).toBeEnabled();
  await expect(page.locator("#token-flow-btn")).toBeEnabled();
}

/**
 * Ensure a session-deployed account exists (the token flow's sender requirement).
 * The token-flow button stays disabled until one does, so its state is the signal.
 */
export async function ensureSessionAccount(page: Page, mode: "local" | "accelerated") {
  if (await page.locator("#token-flow-btn").isDisabled()) {
    await deployAndAssert(page, mode);
  }
}

/** Run token flow and assert all UI state transitions. */
export async function runTokenFlowAndAssert(
  page: Page,
  mode: "local" | "accelerated",
): Promise<void> {
  await ensureSessionAccount(page, mode);
  // Snapshot the shared #log BEFORE the run: the balance assertion below must match a NEW
  // occurrence, not a stale line from an earlier token flow on the same page.
  const priorMatches =
    (await page.locator("#log").textContent())?.match(/Balances — Alice: 500, Bob: 500/g)?.length ??
    0;
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
  // Behavioral assertion, not just flow completion: mint 1000 → transfer 500 must land
  // exactly 500/500 (guards the standards-token semantics, not only the UI plumbing).
  // Occurrence-counted so a stale line from an earlier run can't satisfy it.
  const nowMatches = flowLog?.match(/Balances — Alice: 500, Bob: 500/g)?.length ?? 0;
  expect(nowMatches, "a NEW 500/500 balance line must appear for THIS run").toBe(priorMatches + 1);

  await expect(page.locator("#progress")).toHaveClass(/hidden/);
  await expect(page.locator("#results")).not.toHaveClass(/hidden/);

  const timeText = await page.locator(`#time-${mode}`).textContent();
  expect(timeText).not.toBe("—");
  expect(timeText).toMatch(/^\d+\.\d+s$/);

  await expect(page.locator(`#tag-${mode}`)).toHaveText("token flow");

  await expect(page.locator("#log")).toContainText("total:");
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
