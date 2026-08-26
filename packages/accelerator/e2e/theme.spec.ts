/**
 * Dual-theme rendering: the Presto token sheet must resolve per prefers-color-scheme, and the
 * [data-theme] override (the Rust eval fallback channel) must beat the media query in both
 * directions. Chromium's colorScheme emulation stands in for the OS; whether a real webview
 * propagates the OS theme into prefers-color-scheme is outside this suite's reach.
 */
import { expect, test } from "@playwright/test";

const LIGHT_BG = "rgb(255, 250, 241)"; // --bg #fffaf1
const DARK_BG = "rgb(25, 18, 38)"; // --bg #191226

const bodyBg = (page: import("@playwright/test").Page) =>
  page.evaluate(() => getComputedStyle(document.body).backgroundColor);

for (const pageName of ["settings", "onboarding", "authorize", "update-prompt", "renewal"]) {
  test(`${pageName}: light and dark tokens both resolve`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: "light" });
    await page.goto(`/${pageName}.html`);
    expect(await bodyBg(page)).toBe(LIGHT_BG);

    await page.emulateMedia({ colorScheme: "dark" });
    expect(await bodyBg(page)).toBe(DARK_BG);
  });
}

test("data-theme override beats the media query in both directions", async ({ page }) => {
  await page.goto("/settings.html");

  await page.emulateMedia({ colorScheme: "dark" });
  await page.evaluate(() => {
    document.documentElement.dataset.theme = "light";
  });
  expect(await bodyBg(page)).toBe(LIGHT_BG);

  await page.emulateMedia({ colorScheme: "light" });
  await page.evaluate(() => {
    document.documentElement.dataset.theme = "dark";
  });
  expect(await bodyBg(page)).toBe(DARK_BG);
});
