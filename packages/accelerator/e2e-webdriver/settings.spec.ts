/**
 * Settings window — verifies config loads and speed changes persist.
 *
 * The Settings window is auto-opened as the bootstrap window in webdriver mode.
 * Tests interact with the real Tauri IPC bridge (no mocks).
 */
import { readConfig } from "./helpers.ts";

const SPEED_LEVELS = ["low", "light", "balanced", "high", "full"];

describe("Settings", () => {
  let originalSpeedIndex: number;

  before(async () => {
    const config = readConfig();
    const speed = (config.speed as string) || "full";
    originalSpeedIndex = Math.max(SPEED_LEVELS.indexOf(speed), 0);
  });

  after(async () => {
    // Always restore the original speed, even if a test assertion fails
    try {
      await browser.execute((idx: number) => {
        const el = document.getElementById("speed") as HTMLInputElement;
        el.value = String(idx);
        el.dispatchEvent(new Event("input", { bubbles: true }));
        el.dispatchEvent(new Event("change", { bubbles: true }));
      }, originalSpeedIndex);
      await browser.pause(500);
    } catch (e) {
      console.error("Settings cleanup failed:", e);
    }
  });

  it("should load the current speed from config", async () => {
    const speedLabel = await browser.$("#speed-label");
    await speedLabel.waitForExist({ timeout: 5000 });
    const text = await speedLabel.getText();
    expect(["Low", "Light", "Balanced", "High", "Full"]).toContain(text);
  });

  it("should change speed via the slider and persist to config", async () => {
    const speedLabel = await browser.$("#speed-label");

    // Set slider to index 2 (Balanced) via JavaScript since range inputs
    // are hard to manipulate via WebDriver click coordinates
    await browser.execute(() => {
      const el = document.getElementById("speed") as HTMLInputElement;
      el.value = "2";
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
    });

    await browser.pause(500);

    const newLabel = await speedLabel.getText();
    expect(newLabel).toBe("Balanced");

    const updatedConfig = readConfig();
    expect(updatedConfig.speed).toBe("balanced");
  });

  it("should display approved origins from config", async () => {
    await browser.refresh();
    await browser.pause(500);

    const config = readConfig();
    const origins = (config.approved_origins as string[]) || [];

    if (origins.length === 0) {
      const emptyState = await browser.$("#origins-empty");
      expect(await emptyState.isDisplayed()).toBe(true);
    } else {
      const items = await browser.$$(".origin-item");
      expect(items.length).toBe(origins.length);
    }
  });
});
