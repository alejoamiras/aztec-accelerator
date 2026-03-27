import { defineConfig } from "@playwright/test";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  use: {
    baseURL: "http://localhost:3456",
    headless: true,
  },
  webServer: {
    command: "bunx serve -l 3456 --no-clipboard .",
    port: 3456,
    reuseExistingServer: true,
    cwd: __dirname,
  },
  projects: [
    {
      name: "desktop-ui",
      testDir: "./e2e",
      timeout: 10_000,
    },
  ],
});
