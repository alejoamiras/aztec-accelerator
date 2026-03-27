/**
 * Authorization flow — verifies the full pipeline:
 * HTTP /prove with unknown origin → auth popup appears → user clicks Allow → origin saved.
 *
 * This is the highest-value WebDriver test because it exercises concurrent
 * HTTP + GUI interactions that can't be tested with mocked Playwright.
 *
 * Skipped on Linux: WebKitGTK's WebDriver implementation returns "Unsupported result type"
 * for element clicks in secondary windows. The auth popup appears but can't be interacted
 * with via WebDriver on Linux. Tracked as a tauri-plugin-webdriver limitation.
 */
import * as os from "node:os";

import { readConfig } from "./helpers.ts";

const IS_LINUX = os.platform() === "linux";
const TEST_ORIGIN = "https://test-e2e-webdriver.example.com";
const PROVE_URL = "http://127.0.0.1:59833/prove";

/**
 * Remove the test origin via the Settings UI (Remove button).
 * This triggers the real IPC call which updates both in-memory config and disk.
 * Writing directly to the config file would only update disk, leaving stale in-memory state.
 */
async function removeTestOriginViaUI(): Promise<void> {
  const url = await browser.getUrl();
  if (!url.includes("settings.html")) {
    await browser.navigateTo("tauri://localhost/settings.html");
    await browser.pause(500);
  }

  // Wait for settings page JS to finish loading (speed-label is populated by loadSettings())
  const speedLabel = await browser.$("#speed-label");
  await speedLabel.waitForExist({ timeout: 5000 });

  // Refresh to ensure we see the latest config
  await browser.refresh();
  await browser.pause(500);

  const items = await browser.$$(".origin-item");
  for (const item of items) {
    const span = await item.$("span");
    const text = await span.getText();
    if (text === TEST_ORIGIN) {
      const removeBtn = await item.$("button");
      await removeBtn.click();
      await browser.pause(500);
      return;
    }
  }
}

/** Close all windows except Settings, then switch back to Settings. */
async function closeExtraWindows(settingsHandle: string): Promise<void> {
  const handles = await browser.getWindowHandles();
  for (const h of handles) {
    if (h !== settingsHandle) {
      await browser.switchToWindow(h);
      await browser.closeWindow();
    }
  }
  if (handles.includes(settingsHandle)) {
    await browser.switchToWindow(settingsHandle);
  }
}

/**
 * Fire a /prove POST with an unknown origin. The request blocks until the
 * auth popup is resolved (Allow/Deny) or the 60s server timeout fires.
 * Uses Node fetch — Origin header is allowed outside browser context.
 */
function fireProveRequest(): Promise<Response> {
  return fetch(PROVE_URL, {
    method: "POST",
    headers: {
      "Content-Type": "application/octet-stream",
      Origin: TEST_ORIGIN,
    },
    body: new Uint8Array([0]),
  });
}

/** Poll getWindowHandles() until a new handle appears (up to 15s). */
async function waitForNewWindow(existingHandles: string[]): Promise<string | null> {
  for (let i = 0; i < 30; i++) {
    await browser.pause(500);
    const handlesNow = await browser.getWindowHandles();
    const newHandle = handlesNow.find((h) => !existingHandles.includes(h));
    if (newHandle) return newHandle;
  }
  return null;
}

// Skip on Linux: WebKitGTK WebDriver can't click elements in secondary windows
(IS_LINUX ? describe.skip : describe)("Authorization Flow", () => {
  let settingsHandle: string;
  let pendingProve: Promise<Response> | null = null;

  before(async () => {
    settingsHandle = await browser.getWindowHandle();
    await removeTestOriginViaUI();
  });

  beforeEach(async () => {
    // Ensure clean state: close extra windows, switch to Settings
    await closeExtraWindows(settingsHandle);
    await browser.pause(300);
  });

  afterEach(async () => {
    // Consume any dangling /prove request so it doesn't leak into the next test
    if (pendingProve) {
      await pendingProve.catch(() => {});
      pendingProve = null;
    }
  });

  after(async () => {
    try {
      await closeExtraWindows(settingsHandle);
      await removeTestOriginViaUI();
    } catch (e) {
      console.error("Auth flow cleanup failed:", e);
    }
  });

  it("should show auth popup and allow with remember", async () => {
    const handlesBefore = await browser.getWindowHandles();

    pendingProve = fireProveRequest();

    const authWindowHandle = await waitForNewWindow(handlesBefore);
    expect(authWindowHandle).not.toBeNull();

    await browser.switchToWindow(authWindowHandle!);
    expect(await browser.getTitle()).toBe("Authorize Site");

    const originText = await browser.$("#origin");
    await originText.waitForExist({ timeout: 3000 });
    expect(await originText.getText()).toBe(TEST_ORIGIN);

    // Remember is checked by default
    const remember = await browser.$("#remember");
    expect(await remember.isSelected()).toBe(true);

    // Click Allow
    const allowBtn = await browser.$("#allow");
    await allowBtn.click();

    // /prove should resolve with non-403 (allowed, but proof data is invalid → 500 or similar)
    const proveResponse = await pendingProve;
    pendingProve = null;
    expect(proveResponse.status).not.toBe(403);

    // Origin saved to config (Remember was checked)
    const config = readConfig();
    const origins = (config.approved_origins as string[]) || [];
    expect(origins).toContain(TEST_ORIGIN);

    await browser.switchToWindow(settingsHandle);

    // Clean up for next test: remove the origin via UI
    await browser.refresh();
    await browser.pause(500);
    await removeTestOriginViaUI();
  });

  it("should deny and return 403 when Deny is clicked", async () => {
    const handlesBefore = await browser.getWindowHandles();

    pendingProve = fireProveRequest();

    const authWindowHandle = await waitForNewWindow(handlesBefore);
    expect(authWindowHandle).not.toBeNull();

    await browser.switchToWindow(authWindowHandle!);

    // Click Deny
    const denyBtn = await browser.$("#deny");
    await denyBtn.click();

    // /prove should return 403 (denied)
    const proveResponse = await pendingProve;
    pendingProve = null;
    expect(proveResponse.status).toBe(403);

    // Origin should NOT be in config
    const config = readConfig();
    const origins = (config.approved_origins as string[]) || [];
    expect(origins).not.toContain(TEST_ORIGIN);

    await browser.switchToWindow(settingsHandle);
  });

  it("should allow without remembering when Remember is unchecked", async () => {
    const handlesBefore = await browser.getWindowHandles();

    pendingProve = fireProveRequest();

    const authWindowHandle = await waitForNewWindow(handlesBefore);
    expect(authWindowHandle).not.toBeNull();

    await browser.switchToWindow(authWindowHandle!);

    // Uncheck Remember
    const remember = await browser.$("#remember");
    await remember.click();
    expect(await remember.isSelected()).toBe(false);

    // Click Allow
    const allowBtn = await browser.$("#allow");
    await allowBtn.click();

    // /prove should resolve with non-403 (allowed for this request)
    const proveResponse = await pendingProve;
    pendingProve = null;
    expect(proveResponse.status).not.toBe(403);

    // Origin should NOT be saved to config (Remember was unchecked)
    const config = readConfig();
    const origins = (config.approved_origins as string[]) || [];
    expect(origins).not.toContain(TEST_ORIGIN);

    await browser.switchToWindow(settingsHandle);
  });
});
