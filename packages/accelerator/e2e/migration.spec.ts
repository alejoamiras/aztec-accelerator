import path from "node:path";
import { expect, test } from "@playwright/test";
import { WINDOW_SIZES } from "./window-sizes.js";

test.beforeEach(async ({ page }) => {
  await page.setViewportSize(WINDOW_SIZES.migration);
  await page.addInitScript({ path: path.join(import.meta.dirname, "tauri-mock.js") });
});

for (const [button, openPresto] of [
  ["visit-presto", true],
  ["dismiss", false],
] as const) {
  test(`migration choice ${button} sends only the boolean decision and fits the window`, async ({
    page,
  }) => {
    await page.goto("/migration.html");
    await expect(page.getByRole("heading", { name: "Meet Presto" })).toBeVisible();
    const control = page.locator(`#${button}`);
    const box = await control.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.y + box!.height).toBeLessThanOrEqual(WINDOW_SIZES.migration.height);
    await control.click();
    const calls = await page.evaluate(() => (window as any).__TAURI_MOCK__.calls);
    expect(calls.map(({ cmd, args }: { cmd: string; args: unknown }) => ({ cmd, args }))).toEqual([
      { cmd: "respond_migration_notice", args: { openPresto } },
    ]);
    await expect(control).toBeDisabled();
  });
}

test("a failed dismissal stays actionable and reports the error", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("respond_migration_notice", () => {
      throw new Error("Could not save dismissal");
    });
  });
  await page.goto("/migration.html");
  await page.locator("#dismiss").click();
  await expect(page.locator(".error-hint")).toHaveText("Failed — try again");
  await expect(page.locator("#dismiss")).toBeEnabled();
  await expect(page.locator("#visit-presto")).toBeEnabled();
});
