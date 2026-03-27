import { defineConfig } from "@playwright/test";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const frontendDir = join(__dirname, "src-tauri", "frontend");

export default defineConfig({
  use: {
    baseURL: "http://localhost:3456",
    headless: true,
  },
  webServer: {
    command: "bunx serve -l 3456 --no-clipboard .",
    port: 3456,
    reuseExistingServer: true,
    cwd: frontendDir,
  },
  projects: [
    {
      name: "desktop-ui",
      testDir: "./e2e",
      timeout: 10_000,
    },
  ],
});
