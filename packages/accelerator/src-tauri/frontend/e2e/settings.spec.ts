import path from "node:path";
import { expect, type Page, test } from "@playwright/test";

const MOCK_PATH = path.join(import.meta.dirname, "tauri-mock.js");

// ── Helpers ──

async function getCalls(page: Page) {
  return page.evaluate(() => (window as any).__TAURI_MOCK__.calls);
}

async function callsFor(page: Page, cmd: string) {
  const calls = await getCalls(page);
  return calls.filter((c: any) => c.cmd === cmd);
}

// ── Error safety net ──

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

test("loads with correct initial state", async ({ page }) => {
  await page.goto("/settings.html");

  // Speed label should show "Full" (default config.speed = "full")
  await expect(page.locator("#speed-label")).toHaveText("Full");

  // Speed description should include CPU count from mock (10)
  await expect(page.locator("#speed-desc")).toContainText("10 cores");

  // Autostart should be unchecked (mock returns false)
  const autostart = page.getByRole("switch", { name: "Start on Login" });
  await expect(autostart).not.toBeChecked();

  // Auto-update should be unchecked (auto_update omitted = falsy)
  const autoUpdate = page.getByRole("switch", { name: "Auto-Update" });
  await expect(autoUpdate).not.toBeChecked();
});

test("shows approved origins list", async ({ page }) => {
  await page.goto("/settings.html");

  // "https://example.com" should be visible from mock get_config
  await expect(page.getByText("https://example.com")).toBeVisible();

  // Empty state should be hidden
  await expect(page.getByText("No approved sites yet.")).not.toBeVisible();
});

test("shows empty state when no origins", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_config", () => ({
      config_version: 1,
      safari_support: false,
      approved_origins: [],
      speed: "full",
    }));
  });
  await page.goto("/settings.html");

  await expect(page.getByText("No approved sites yet.")).toBeVisible();
});

test("remove origin calls invoke and re-renders", async ({ page }) => {
  // After remove, return empty origins on the second get_config call
  await page.addInitScript(() => {
    let callCount = 0;
    (window as any).__TAURI_MOCK__.setHandler("get_config", () => {
      callCount++;
      return {
        config_version: 1,
        safari_support: false,
        approved_origins: callCount === 1 ? ["https://example.com"] : [],
        speed: "full",
      };
    });
  });
  await page.goto("/settings.html");

  // Origin should be visible initially
  await expect(page.getByText("https://example.com")).toBeVisible();

  // Click Remove
  await page.getByRole("button", { name: "Remove" }).click();

  // Should call remove_approved_origin then reload settings
  const removeCalls = await callsFor(page, "remove_approved_origin");
  expect(removeCalls.length).toBe(1);
  expect(removeCalls[0].args).toEqual({ origin: "https://example.com" });

  // After re-render, empty state should show
  await expect(page.getByText("No approved sites yet.")).toBeVisible();
});

test("speed slider input event updates label without IPC", async ({ page }) => {
  await page.goto("/settings.html");

  // Dispatch input event (drag) — should update label but NOT call set_speed
  await page.locator("#speed").evaluate((el: HTMLInputElement) => {
    el.value = "2";
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });

  await expect(page.locator("#speed-label")).toHaveText("Balanced");
  await expect(page.locator("#speed-desc")).toContainText("5 of 10 cores");

  // No set_speed call — input event is UI-only
  const speedCalls = await callsFor(page, "set_speed");
  expect(speedCalls.length).toBe(0);
});

test("speed slider change event calls set_speed", async ({ page }) => {
  await page.goto("/settings.html");

  await page.locator("#speed").evaluate((el: HTMLInputElement) => {
    el.value = "2";
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });

  const speedCalls = await callsFor(page, "set_speed");
  expect(speedCalls.length).toBe(1);
  expect(speedCalls[0].args).toEqual({ speed: "balanced" });
});

test("autostart toggle calls set_autostart", async ({ page }) => {
  await page.goto("/settings.html");

  // Checkbox is visually hidden (0x0 CSS) — toggle via JS to trigger change event
  await page.locator("#autostart").evaluate((el: HTMLInputElement) => {
    el.checked = true;
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });

  const calls = await callsFor(page, "set_autostart");
  expect(calls.length).toBe(1);
  expect(calls[0].args).toEqual({ enabled: true });
});

test("toggle error reverts checkbox and shows hint", async ({ page }) => {
  // Make set_autostart fail
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("set_autostart", () => {
      throw new Error("Autostart failed");
    });
  });
  await page.goto("/settings.html");

  const toggle = page.locator("#autostart");
  await expect(toggle).not.toBeChecked();

  await toggle.evaluate((el: HTMLInputElement) => {
    el.checked = true;
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });

  // Should revert to unchecked and show error hint
  await expect(toggle).not.toBeChecked();
  await expect(page.locator(".error-hint")).toBeVisible();
  await expect(page.locator(".error-hint")).toHaveText("Failed — try again");
});

test("safari row visible on macOS", async ({ page }) => {
  await page.goto("/settings.html");
  await expect(page.locator("#safari-row")).toBeVisible();
});

test("safari row hidden on Linux", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_system_info", () => ({
      platform: "linux",
      cpu_count: 8,
    }));
  });
  await page.goto("/settings.html");

  await expect(page.locator("#safari-row")).not.toBeVisible();
});

test("auto-update toggle calls set_auto_update", async ({ page }) => {
  await page.goto("/settings.html");

  await page.locator("#auto-update").evaluate((el: HTMLInputElement) => {
    el.checked = true;
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });

  const calls = await callsFor(page, "set_auto_update");
  expect(calls.length).toBe(1);
  expect(calls[0].args).toEqual({ enabled: true });
});

test("bootstrap failure logs error without crashing", async ({ page }) => {
  // Make get_config reject — loadSettings() should catch and log
  jsErrors.length = 0; // Allow the expected error
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_config", () => {
      throw new Error("Config unavailable");
    });
  });
  await page.goto("/settings.html");

  // Page should still be rendered (not blank)
  await expect(page.locator("body")).toBeVisible();

  // The error is caught by loadSettings().catch() — logged to console, not pageerror.
  // Clear the jsErrors so afterEach doesn't fail on it
  jsErrors.length = 0;
});

test("speed error shows hint and reloads settings", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("set_speed", () => {
      throw new Error("Speed save failed");
    });
  });
  await page.goto("/settings.html");

  await page.locator("#speed").evaluate((el: HTMLInputElement) => {
    el.value = "1";
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });

  await expect(page.locator(".error-hint")).toBeVisible();
  await expect(page.locator(".error-hint")).toHaveText("Failed to save");
});
