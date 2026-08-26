/**
 * The three popups that size themselves and cannot scroll: whatever does not fit is simply gone.
 * Settings and onboarding own bespoke layout specs (they carry `body.scrollable`, so for them the
 * question is "does the default state fit", not "is the button reachable at all"). These three had
 * no layout coverage, which is how the renewal window shipped with its consent buttons under the
 * bottom edge.
 *
 * Sized to VIEWPORT_SIZES, not WINDOW_SIZES: the title bar's `WEBVIEW_CHROME_HEIGHT` is exactly what
 * hid those buttons, so a spec that grants the page the full window height cannot see the bug.
 */
import path from "node:path";
import { expect, test } from "@playwright/test";
import { VIEWPORT_SIZES } from "./window-sizes.js";

const MOCK_PATH = path.join(import.meta.dirname, "tauri-mock.js");

const POPUPS = [
  { page: "renewal", actions: ["#renew", "#later"] },
  { page: "update-prompt", actions: ["#update", "#later"] },
  { page: "authorize", actions: ["#allow", "#deny"] },
] as const;

for (const { page: name, actions } of POPUPS) {
  test(`${name}: content fits the webview viewport`, async ({ page }) => {
    const viewport = VIEWPORT_SIZES[name];
    await page.setViewportSize(viewport);
    await page.addInitScript({ path: MOCK_PATH });
    await page.goto(`/${name}.html`);

    const content = await page.evaluate(() => document.documentElement.scrollHeight);
    expect(
      content,
      `${name} content is ${content}px in a ${viewport.height}px viewport — raise the height in windows.rs (and window-sizes.ts) or cut a row`,
    ).toBeLessThanOrEqual(viewport.height);
  });

  test(`${name}: both actions are fully visible`, async ({ page }) => {
    // The assertion that matters to a user. A dialog can "fit" by the numbers and still bury its
    // buttons if a later row grows, and none of these three can be scrolled to recover them.
    await page.setViewportSize(VIEWPORT_SIZES[name]);
    await page.addInitScript({ path: MOCK_PATH });
    await page.goto(`/${name}.html`);

    for (const selector of actions) {
      await expect(page.locator(selector)).toBeInViewport({ ratio: 1 });
    }
  });
}
