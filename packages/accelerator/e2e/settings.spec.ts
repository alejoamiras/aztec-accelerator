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
  await expect(page.getByText("No approved sites yet")).not.toBeVisible();
});

test("shows empty state when no origins", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_config", () => ({
      config_version: 1,
      https_enabled: false,
      approved_origins: [],
      speed: "full",
    }));
  });
  await page.goto("/settings.html");

  await expect(page.getByText("No approved sites yet")).toBeVisible();
  // Regression guard: the message must sit INSIDE the bordered well. It used to render below an
  // unbordered fixed-height list, which read as a black void with a stranded caption.
  await expect(page.locator(".well #origins-empty")).toHaveCount(1);
});

test("remove origin calls invoke and re-renders", async ({ page }) => {
  // After remove, return empty origins on the second get_config call
  await page.addInitScript(() => {
    let callCount = 0;
    (window as any).__TAURI_MOCK__.setHandler("get_config", () => {
      callCount++;
      return {
        config_version: 1,
        https_enabled: false,
        approved_origins: callCount === 1 ? ["https://example.com"] : [],
        speed: "full",
      };
    });
  });
  await page.goto("/settings.html");

  // Origin should be visible initially
  await expect(page.getByText("https://example.com")).toBeVisible();

  // Click Remove
  // Scoped to the list: the certificate action is also named "Remove" (inside a closed disclosure),
  // so an unscoped role query would be ambiguous under Playwright strict mode.
  await page.locator("#origins").getByRole("button", { name: "Remove" }).click();

  // Should call remove_approved_origin then reload settings
  const removeCalls = await callsFor(page, "remove_approved_origin");
  expect(removeCalls.length).toBe(1);
  expect(removeCalls[0].args).toEqual({ origin: "https://example.com" });

  // After re-render, empty state should show
  await expect(page.getByText("No approved sites yet")).toBeVisible();
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

test("Encrypted Connection section visible on macOS", async ({ page }) => {
  await page.goto("/settings.html");
  await expect(page.locator("#https-section")).toBeVisible();
});

test("Encrypted Connection section visible on Linux (NSS trust backend)", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_system_info", () => ({
      platform: "linux",
      cpu_count: 8,
    }));
  });
  await page.goto("/settings.html");

  await expect(page.locator("#https-section")).toBeVisible();
});

test("Encrypted Connection section visible on Windows (CurrentUser Root trust backend)", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_system_info", () => ({
      platform: "windows",
      cpu_count: 8,
    }));
  });
  await page.goto("/settings.html");

  await expect(page.locator("#https-section")).toBeVisible();
});

test("the certificate action is collapsed behind a disclosure, not sitting in the open", async ({
  page,
}) => {
  // It IS a real action (the documented recovery path for a partially-trusted install), but it must
  // not compete with the toggles for attention or invite a stray click.
  await page.goto("/settings.html");
  const details = page.locator("#cert-details");
  await expect(details).toBeVisible();
  await expect(details).not.toHaveAttribute("open", "");
  await expect(page.locator("#remove-trust")).toBeHidden();

  await details.locator("summary").click();
  await expect(page.locator("#remove-trust")).toBeVisible();
});

test("https toggle calls enable/disable commands", async ({ page }) => {
  await page.goto("/settings.html");

  // Enable HTTPS — should call enable_https
  await page.locator("#https").evaluate((el: HTMLInputElement) => {
    el.checked = true;
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });
  const enableCalls = await callsFor(page, "enable_https");
  expect(enableCalls.length).toBe(1);

  // Disable HTTPS — should call disable_https
  await page.locator("#https").evaluate((el: HTMLInputElement) => {
    el.checked = false;
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });
  const disableCalls = await callsFor(page, "disable_https");
  expect(disableCalls.length).toBe(1);
});

test("a failed enable_https shows the persisted state, not the failure", async ({ page }) => {
  // The restart-required path: `enable_https` persists `https_enabled = true` and STILL returns Err
  // ("restart to finish enabling"). An optimistic revert showed the switch OFF while the config — and
  // the next launch — said ON, and re-toggling only reproduced the error. The panel must re-read.
  await page.addInitScript(() => {
    let enabled = false;
    (window as any).__TAURI_MOCK__.setHandler("get_config", () => ({
      config_version: 1,
      https_enabled: enabled,
      approved_origins: [],
      speed: "full",
    }));
    (window as any).__TAURI_MOCK__.setHandler("enable_https", () => {
      enabled = true; // committed...
      throw "HTTPS is set up, but the running server is still using a previous certificate."; // ...then failed
    });
  });
  await page.goto("/settings.html");
  await expect(page.locator("#https")).not.toBeChecked();

  await page.locator("#https").evaluate((el: HTMLInputElement) => {
    el.checked = true;
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });

  // Reflects what the backend actually stored...
  await expect(page.locator("#https")).toBeChecked();
  // ...and says why, in the backend's own words — "Failed — try again" would hide the restart.
  await expect(page.getByText("still using a previous certificate")).toBeVisible();
});

test("the certificate action and the https toggle disable each other while one runs", async ({
  page,
}) => {
  // Both routes into the same Rust lifecycle mutex. It serializes them correctly, but leaving the
  // other control live let the user queue an operation against state they could no longer see.
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler(
      "enable_https",
      () => new Promise((resolve) => setTimeout(() => resolve(null), 300)),
    );
  });
  await page.goto("/settings.html");
  await page.locator("#cert-details summary").click();

  await page.locator("#https").evaluate((el: HTMLInputElement) => {
    el.checked = true;
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });

  await expect(page.locator("#remove-trust")).toBeDisabled();
  await expect(page.locator("#https")).toBeDisabled();
  // Both come back once the operation settles.
  await expect(page.locator("#remove-trust")).toBeEnabled();
  await expect(page.locator("#https")).toBeEnabled();
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

test("bootstrap failure renders page without origins or speed", async ({ page }) => {
  // Make get_config reject — loadSettings() catches and logs
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_config", () => {
      throw new Error("Config unavailable");
    });
  });
  await page.goto("/settings.html");

  // Page should render but with default/empty state — no origins loaded
  await expect(page.locator("body")).toBeVisible();
  // Speed label stays at its HTML default "Full" (loadSettings never ran updateSpeedUI)
  await expect(page.locator("#speed-label")).toHaveText("Full");
  // Origins list should be empty (renderOrigins never called)
  await expect(page.locator(".origin-item")).toHaveCount(0);
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
