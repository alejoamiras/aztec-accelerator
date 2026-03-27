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

test("shows decoded origin from URL params", async ({ page }) => {
  await page.goto("/authorize.html?origin=https%3A%2F%2Fexample.com");
  await expect(page.locator("#origin")).toHaveText("https://example.com");
});

test("allow calls respond_auth with correct args", async ({ page }) => {
  await page.goto("/authorize.html?origin=https%3A%2F%2Fexample.com");

  await page.getByRole("button", { name: "Allow" }).click();

  const calls = await callsFor(page, "respond_auth");
  expect(calls.length).toBe(1);
  // "Remember this site" checkbox defaults to checked
  expect(calls[0].args).toEqual({
    origin: "https://example.com",
    allowed: true,
    remember: true,
  });
});

test("deny calls respond_auth with correct args", async ({ page }) => {
  await page.goto("/authorize.html?origin=https%3A%2F%2Fexample.com");

  await page.getByRole("button", { name: "Deny" }).click();

  const calls = await callsFor(page, "respond_auth");
  expect(calls.length).toBe(1);
  // "Remember this site" checkbox defaults to checked — deny still sends remember: true
  expect(calls[0].args).toEqual({
    origin: "https://example.com",
    allowed: false,
    remember: true,
  });
});

test("both buttons disabled after successful click", async ({ page }) => {
  await page.goto("/authorize.html?origin=https%3A%2F%2Fexample.com");

  await page.getByRole("button", { name: "Allow" }).click();

  // wireButton success path: buttons stay disabled (no window close in mock)
  await expect(page.getByRole("button", { name: "Allow" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Deny" })).toBeDisabled();
});

test("uncheck remember changes allow args", async ({ page }) => {
  await page.goto("/authorize.html?origin=https%3A%2F%2Ftest.com");

  // Uncheck "Remember this site"
  await page.locator("#remember").uncheck();
  await page.getByRole("button", { name: "Allow" }).click();

  const calls = await callsFor(page, "respond_auth");
  expect(calls[0].args).toEqual({
    origin: "https://test.com",
    allowed: true,
    remember: false,
  });
});

test("missing origin param shows unknown", async ({ page }) => {
  await page.goto("/authorize.html");
  await expect(page.locator("#origin")).toHaveText("unknown");
});

test("blank origin param shows unknown", async ({ page }) => {
  await page.goto("/authorize.html?origin=");
  await expect(page.locator("#origin")).toHaveText("unknown");
});

test("error on invoke re-enables buttons and shows hint", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("respond_auth", () => {
      throw new Error("Auth failed");
    });
  });
  await page.goto("/authorize.html?origin=https%3A%2F%2Fexample.com");

  await page.getByRole("button", { name: "Allow" }).click();

  // wireButton error path: buttons re-enabled, error hint shown
  await expect(page.getByRole("button", { name: "Allow" })).not.toBeDisabled();
  await expect(page.getByRole("button", { name: "Deny" })).not.toBeDisabled();
  await expect(page.locator(".error-hint")).toBeVisible();
});
