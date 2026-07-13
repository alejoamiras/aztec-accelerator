import path from "node:path";
import { expect, type Page, test } from "@playwright/test";

const MOCK_PATH = path.join(import.meta.dirname, "tauri-mock.js");

async function getCalls(page: Page) {
  return page.evaluate(() => (window as any).__TAURI_MOCK__.calls);
}
async function callsFor(page: Page, cmd: string) {
  const calls = await getCalls(page);
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

test("all three toggles pre-checked; HTTPS follows https_default", async ({ page }) => {
  await page.goto("/onboarding.html");
  await expect(page.locator("#opt-https")).toBeChecked();
  await expect(page.locator("#opt-autostart")).toBeChecked();
  await expect(page.locator("#opt-auto-update")).toBeChecked();
});

test("HTTPS explanation shows the per-OS certificate copy", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_onboarding_state", () => ({
      platform: "linux",
      https_default: true,
      autostart_enabled: false,
      auto_update: null,
      trust_status: { stores: [] },
    }));
  });
  await page.goto("/onboarding.html");
  await expect(page.locator("#https-warn")).toContainText("no separate prompt");
});

test("HTTPS stays pre-checked for an upgrader on any platform (A9)", async ({ page }) => {
  // A9/§2.1: HTTPS is pre-checked for everyone incl. upgraders. The wizard is a recommended setup,
  // so Start-on-Login + Auto-Update also default YES regardless of prior OS/config state.
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_onboarding_state", () => ({
      platform: "linux",
      https_default: true,
    }));
  });
  await page.goto("/onboarding.html");
  await expect(page.locator("#opt-https")).toBeChecked();
  await expect(page.locator("#opt-autostart")).toBeChecked();
  await expect(page.locator("#opt-auto-update")).toBeChecked();
});

test("Start invokes complete_onboarding with the toggle states and closes on success", async ({
  page,
}) => {
  await page.goto("/onboarding.html");
  // The toggle <input> is visually hidden under the slider span, so Playwright's native uncheck()
  // (a real click) times out — set the property directly, as the settings specs do. onboarding.html
  // reads `.checked` at Start time (no change listener), so no dispatchEvent is needed.
  await page.locator("#opt-auto-update").evaluate((el: HTMLInputElement) => {
    el.checked = false;
  });
  await page.locator("#start").click();

  const calls = await callsFor(page, "complete_onboarding");
  expect(calls.length).toBe(1);
  expect(calls[0].args).toEqual({ https: true, autostart: true, autoUpdate: false });
  // completed:true (mock default) → the wizard closes itself.
  expect((await callsFor(page, "__window.close")).length).toBe(1);
});

test("partial cert failure: HTTPS shown off with Retry, other choices still applied", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("complete_onboarding", () => ({
      https: { Err: "certutil not found" },
      autostart: { Ok: null },
      auto_update: { Ok: null },
      completed: false,
    }));
  });
  await page.goto("/onboarding.html");
  await page.locator("#start").click();

  await expect(page.locator("#https-result")).toContainText("certutil not found");
  await expect(page.locator("#opt-https")).not.toBeChecked();
  await expect(page.locator("#https-retry")).toBeVisible();
  await expect(page.locator("#skip")).toHaveText("Continue without HTTPS");
  // The window must NOT have closed (marker not set).
  expect((await callsFor(page, "__window.close")).length).toBe(0);
});

test("Retry re-checks HTTPS and re-enables Start", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("complete_onboarding", () => ({
      https: { Err: "boom" },
      autostart: { Ok: null },
      auto_update: { Ok: null },
      completed: false,
    }));
  });
  await page.goto("/onboarding.html");
  await page.locator("#start").click();
  await expect(page.locator("#https-retry")).toBeVisible();

  await page.locator("#https-retry").click();
  await expect(page.locator("#opt-https")).toBeChecked();
  await expect(page.locator("#https-retry")).not.toBeVisible();
});

test("Skip dismisses onboarding and closes", async ({ page }) => {
  await page.goto("/onboarding.html");
  await page.locator("#skip").click();
  expect((await callsFor(page, "dismiss_onboarding")).length).toBe(1);
  expect((await callsFor(page, "__window.close")).length).toBe(1);
});
