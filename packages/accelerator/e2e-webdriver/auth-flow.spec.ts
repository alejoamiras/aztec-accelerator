/**
 * Authorization flow — verifies the full pipeline:
 * HTTP /prove with unknown origin → auth popup appears → user clicks Allow → origin saved.
 *
 * This is the highest-value WebDriver test because it exercises concurrent
 * HTTP + GUI interactions that can't be tested with mocked Playwright.
 */
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

const CONFIG_PATH = path.join(os.homedir(), ".aztec-accelerator", "config.json");
const TEST_ORIGIN = "https://test-e2e-webdriver.example.com";
const PROVE_URL = "http://127.0.0.1:59833/prove";

function readConfig(): Record<string, unknown> {
  return JSON.parse(fs.readFileSync(CONFIG_PATH, "utf-8"));
}

/**
 * Remove the test origin via the Settings UI (Remove button).
 * This triggers the real IPC call which updates both in-memory config and disk.
 * Writing directly to the config file would only update disk, leaving stale in-memory state.
 */
async function removeTestOriginViaUI(): Promise<void> {
  // Make sure we're on the Settings page
  const url = await browser.getUrl();
  if (!url.includes("settings.html")) {
    await browser.navigateTo("tauri://localhost/settings.html");
    await browser.pause(500);
  }

  // Look for origin items and remove any that match our test origin
  const items = await browser.$$(".origin-item");
  for (const item of items) {
    const span = await item.$("span");
    const text = await span.getText();
    if (text === TEST_ORIGIN) {
      const removeBtn = await item.$("button");
      await removeBtn.click();
      await browser.pause(500); // Wait for IPC + re-render
      return;
    }
  }
}

describe("Authorization Flow", () => {
  let settingsHandle: string;

  before(async () => {
    settingsHandle = await browser.getWindowHandle();
    // Remove the test origin through the UI (updates both memory and disk)
    await removeTestOriginViaUI();
  });

  after(async () => {
    // Clean up: switch back to Settings and remove the test origin
    try {
      const handles = await browser.getWindowHandles();
      if (handles.includes(settingsHandle)) {
        await browser.switchToWindow(settingsHandle);
      }
      await removeTestOriginViaUI();
    } catch {
      // Best-effort cleanup
    }
  });

  it("should show auth popup when /prove is called with unknown origin", async () => {
    // Get current window handles (should just be Settings)
    const handlesBefore = await browser.getWindowHandles();

    // Fire a /prove POST with an unknown origin — don't await the response,
    // it blocks until the user responds to the auth popup (or 60s timeout)
    const provePromise = fetch(PROVE_URL, {
      method: "POST",
      headers: {
        "Content-Type": "application/octet-stream",
        Origin: TEST_ORIGIN,
      },
      body: new Uint8Array([0]),
    });

    // Poll for a new window handle (the auth popup)
    let authWindowHandle: string | null = null;
    for (let i = 0; i < 20; i++) {
      await browser.pause(500);
      const handlesNow = await browser.getWindowHandles();
      const newHandle = handlesNow.find((h) => !handlesBefore.includes(h));
      if (newHandle) {
        authWindowHandle = newHandle;
        break;
      }
    }

    expect(authWindowHandle).not.toBeNull();

    // Switch to the auth popup
    await browser.switchToWindow(authWindowHandle!);
    const title = await browser.getTitle();
    expect(title).toBe("Authorize Site");

    // Verify the origin is displayed
    const originText = await browser.$("#origin");
    await originText.waitForExist({ timeout: 3000 });
    expect(await originText.getText()).toBe(TEST_ORIGIN);

    // Verify "Remember" checkbox is checked by default
    const remember = await browser.$("#remember");
    expect(await remember.isSelected()).toBe(true);

    // Click "Allow"
    const allowBtn = await browser.$("#allow");
    await allowBtn.click();

    // Wait for the /prove request to resolve (it should no longer be 403)
    const proveResponse = await provePromise;
    expect(proveResponse.status).not.toBe(403);

    // Verify the origin was saved to config
    await browser.pause(300);
    const config = readConfig();
    const origins = (config.approved_origins as string[]) || [];
    expect(origins).toContain(TEST_ORIGIN);

    // Switch back to Settings
    await browser.switchToWindow(settingsHandle);
  });
});
