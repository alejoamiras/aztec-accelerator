import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  testMatch: "lna.real.spec.ts",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 30_000,
  expect: { timeout: 10_000 },
  use: {
    ...devices["Desktop Chrome"],
    baseURL: "http://127.0.0.1:5173",
    headless: true,
    trace: "retain-on-failure",
    launchOptions: {
      args: [
        // Only the two source origins are classified as public. The health responder remains
        // loopback, which creates the real public→loopback LNA boundary on one CI machine.
        "--ip-address-space-overrides=127.0.0.1:5173=public,127.0.0.1:5174=public",
      ],
    },
  },
  webServer: [
    {
      command: "bun --no-orphans run dev -- --host 127.0.0.1 --port 5173 --strictPort",
      url: "http://127.0.0.1:5173",
      reuseExistingServer: false,
      timeout: 120_000,
    },
    {
      command:
        "bun --no-orphans run --cwd ../landing dev -- --host 127.0.0.1 --port 5174 --strictPort",
      url: "http://127.0.0.1:5174",
      reuseExistingServer: false,
      timeout: 120_000,
    },
    {
      command: "bun --no-orphans e2e/lna-health-server.ts",
      url: "http://127.0.0.1:59833/ready",
      reuseExistingServer: false,
      timeout: 30_000,
    },
  ],
});
