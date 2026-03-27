/**
 * Settings window — verifies config loads and speed changes persist.
 *
 * The Settings window is auto-opened as the bootstrap window in webdriver mode.
 * Tests interact with the real Tauri IPC bridge (no mocks).
 */
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

const CONFIG_PATH = path.join(os.homedir(), ".aztec-accelerator", "config.json");

function readConfig(): Record<string, unknown> {
  return JSON.parse(fs.readFileSync(CONFIG_PATH, "utf-8"));
}

describe("Settings", () => {
  it("should load the current speed from config", async () => {
    const speedLabel = await browser.$("#speed-label");
    await speedLabel.waitForExist({ timeout: 5000 });
    const text = await speedLabel.getText();
    // Default speed is "Full", but might have been changed — just verify it's a known value
    expect(["Low", "Light", "Balanced", "High", "Full"]).toContain(text);
  });

  it("should change speed via the slider and persist to config", async () => {
    const speedLabel = await browser.$("#speed-label");

    // Read initial state
    const initialConfig = readConfig();
    const initialSpeed = (initialConfig.speed as string) || "full";

    // Set slider to index 2 (Balanced) via JavaScript since range inputs
    // are hard to manipulate via WebDriver click coordinates
    await browser.execute(() => {
      const el = document.getElementById("speed") as HTMLInputElement;
      el.value = "2";
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
    });

    // Wait for the IPC round-trip to complete
    await browser.pause(500);

    // Verify the label updated
    const newLabel = await speedLabel.getText();
    expect(newLabel).toBe("Balanced");

    // Verify config file was updated
    const updatedConfig = readConfig();
    expect(updatedConfig.speed).toBe("balanced");

    // Restore original speed
    const restoreIndex = ["low", "light", "balanced", "high", "full"].indexOf(initialSpeed);
    await browser.execute(
      (idx: number) => {
        const el = document.getElementById("speed") as HTMLInputElement;
        el.value = String(idx);
        el.dispatchEvent(new Event("input", { bubbles: true }));
        el.dispatchEvent(new Event("change", { bubbles: true }));
      },
      restoreIndex >= 0 ? restoreIndex : 4,
    );

    await browser.pause(500);
  });

  it("should display approved origins from config", async () => {
    // Reload to pick up any config changes from prior tests
    await browser.refresh();
    await browser.pause(500);

    const config = readConfig();
    const origins = (config.approved_origins as string[]) || [];

    if (origins.length === 0) {
      // Empty state should be visible
      const emptyState = await browser.$("#origins-empty");
      expect(await emptyState.isDisplayed()).toBe(true);
    } else {
      // Origin list items should be rendered
      const items = await browser.$$(".origin-item");
      expect(items.length).toBe(origins.length);
    }
  });
});
