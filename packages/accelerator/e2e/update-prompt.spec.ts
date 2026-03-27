import path from "node:path";
import { expect, type Page, test } from "@playwright/test";

const MOCK_PATH = path.join(import.meta.dirname, "tauri-mock.js");

async function callsFor(page: Page, cmd: string) {
  const calls = await page.evaluate(() => (window as any).__TAURI_MOCK__.calls);
  return calls.filter((c: any) => c.cmd === cmd);
}

const jsErrors: string[] = [];

test.beforeEach(async ({ page }) => {
  jsErrors.length = 0;
  page.on("pageerror", (err) => jsErrors.push(err.message));
  await page.addInitScript({ path: MOCK_PATH });
});

test.afterEach(() => {
  expect(jsErrors, "Unexpected JS runtime errors").toEqual([]);
});

// ── Tests ──

test("shows version transition", async ({ page }) => {
  await page.goto("/update-prompt.html?current=1.0.0&version=1.1.0");
  // Unicode arrow: \u2192
  await expect(page.locator("#version")).toContainText("v1.0.0");
  await expect(page.locator("#version")).toContainText("v1.1.0");
});

test("update now calls respond_update_prompt", async ({ page }) => {
  await page.goto("/update-prompt.html?current=1.0.0&version=1.1.0");

  await page.getByRole("button", { name: "Update Now" }).click();

  const calls = await callsFor(page, "respond_update_prompt");
  expect(calls.length).toBe(1);
  // "Keep me updated automatically" checkbox defaults to checked
  expect(calls[0].args).toEqual({ action: "update", autoUpdate: true });
});

test("remind me later calls respond_update_prompt", async ({ page }) => {
  await page.goto("/update-prompt.html?current=1.0.0&version=1.1.0");

  await page.getByRole("button", { name: "Remind Me Later" }).click();

  const calls = await callsFor(page, "respond_update_prompt");
  expect(calls.length).toBe(1);
  // "later" always sends autoUpdate: false (hardcoded, not from checkbox)
  expect(calls[0].args).toEqual({ action: "later", autoUpdate: false });
});

test("update now shows loading text", async ({ page }) => {
  await page.goto("/update-prompt.html?current=1.0.0&version=1.1.0");

  await page.getByRole("button", { name: "Update Now" }).click();

  // wireButton loadingText: "Updating…" (\u2026)
  await expect(page.getByRole("button", { name: /Updating/ })).toBeVisible();
});

test("both buttons disabled during operation", async ({ page }) => {
  await page.goto("/update-prompt.html?current=1.0.0&version=1.1.0");

  await page.getByRole("button", { name: "Update Now" }).click();

  await expect(page.locator("#update")).toBeDisabled();
  await expect(page.locator("#later")).toBeDisabled();
});

test("uncheck auto-update sends autoUpdate false on Update Now", async ({ page }) => {
  await page.goto("/update-prompt.html?current=1.0.0&version=1.1.0");

  // Uncheck "Keep me updated automatically"
  await page.locator("#auto-update").uncheck();
  await page.getByRole("button", { name: "Update Now" }).click();

  const calls = await callsFor(page, "respond_update_prompt");
  expect(calls[0].args).toEqual({ action: "update", autoUpdate: false });
});

test("missing version params shows unknown", async ({ page }) => {
  await page.goto("/update-prompt.html");
  await expect(page.locator("#version")).toContainText("vunknown");
});

test("error on invoke re-enables buttons and shows hint", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("respond_update_prompt", () => {
      throw new Error("Update failed");
    });
  });
  await page.goto("/update-prompt.html?current=1.0.0&version=1.1.0");

  await page.getByRole("button", { name: "Update Now" }).click();

  // wireButton error path: buttons re-enabled, original text restored, hint shown
  await expect(page.locator("#update")).not.toBeDisabled();
  await expect(page.locator("#later")).not.toBeDisabled();
  await expect(page.locator("#update")).toHaveText("Update Now");
  await expect(page.locator(".error-hint")).toBeVisible();
});
