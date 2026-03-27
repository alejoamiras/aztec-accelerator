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
