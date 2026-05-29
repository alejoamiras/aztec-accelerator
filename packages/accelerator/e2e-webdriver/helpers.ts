/**
 * Shared helpers for WebDriver E2E tests.
 */
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

export const CONFIG_PATH = path.join(os.homedir(), ".aztec-accelerator", "config.json");

/** Read the accelerator config. Returns default shape if file doesn't exist (fresh CI). */
export function readConfig(): Record<string, unknown> {
  try {
    return JSON.parse(fs.readFileSync(CONFIG_PATH, "utf-8"));
  } catch {
    return { config_version: 1, safari_support: false, approved_origins: [], speed: "full" };
  }
}

const SETTINGS_TITLE = "Aztec Accelerator Settings";

/**
 * Anchor the session on the Settings window before a spec interacts with it.
 *
 * `smoke.spec` and `settings.spec` assume the active window IS Settings — but
 * any stray window (an update prompt, a future dialog, a leftover popup) can
 * leave WebDriver's active context pointing elsewhere, making static elements
 * like `#speed-label` appear "not found". This switches to the Settings window
 * explicitly and waits for it to be ready.
 *
 * It deliberately does NOT navigate/create a Settings window if none exists —
 * the bootstrap window is supposed to be there, so its absence is a real
 * regression and we fail loudly rather than masking it.
 */
export async function ensureSettingsWindow(): Promise<void> {
  const handles = await browser.getWindowHandles();
  for (const handle of handles) {
    await browser.switchToWindow(handle);
    if ((await browser.getTitle()) === SETTINGS_TITLE) {
      await browser.$("#speed-label").waitForExist({ timeout: 5000 });
      return;
    }
  }
  throw new Error(
    `Settings window not found among ${handles.length} window(s) — expected the bootstrap "${SETTINGS_TITLE}" window to be open.`,
  );
}
