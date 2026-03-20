import { defineConfig } from "@playwright/test";

export default defineConfig({
  use: {
    baseURL: "http://localhost:5173",
    headless: true,
  },
  webServer: {
    command: "bun run dev",
    port: 5173,
    reuseExistingServer: true,
  },
  projects: [
    {
      name: "mocked",
      testDir: "./e2e",
      testMatch: "*.mocked.spec.ts",
      timeout: 30_000,
    },
    {
      name: "local-network",
      testDir: "./e2e",
      testMatch: "*.local-network.spec.ts",
      timeout: 10 * 60 * 1000,
      retries: 1,
      use: {
        actionTimeout: 0,
        trace: "retain-on-failure",
      },
    },
    {
      name: "smoke",
      testDir: "./e2e",
      testMatch: "*.smoke.spec.ts",
      timeout: 15 * 60 * 1000,
      retries: 2,
      use: {
        actionTimeout: 0,
        trace: "retain-on-failure",
      },
    },
  ],
});
