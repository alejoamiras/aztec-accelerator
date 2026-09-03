import { defineConfig } from "@playwright/test";

export default defineConfig({
  use: {
    baseURL: "http://localhost:5173",
    headless: true,
  },
  // The packaged release gate owns a production preview process so it can build after swapping in the
  // packed SDK and preserve the server log. All other projects keep Playwright's managed dev server.
  webServer: process.env.PLAYWRIGHT_EXTERNAL_WEBSERVER
    ? undefined
    : {
        // --no-orphans: Vite must not outlive the bun wrapper when Playwright tears down.
        command: "bun --no-orphans run dev",
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
      name: "production-smoke",
      testDir: "./e2e",
      testMatch: "*.production-smoke.spec.ts",
      timeout: 30_000,
      use: {
        baseURL: "http://localhost:4173",
      },
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
    {
      // B4 packaged-E2E: the composed native-bb-over-HTTPS proof against the INSTALLED desktop app.
      // The CI harness seeds CA trust out-of-band + launches the app before this runs; the page is
      // loaded with ?httpsOnly=true so a green run positively exercises the browser⇄app TLS path.
      name: "packaged-e2e",
      testDir: "./e2e",
      testMatch: "*.packaged-e2e.spec.ts",
      timeout: 15 * 60 * 1000,
      retries: 1,
      use: {
        baseURL: "http://127.0.0.1:5173",
        actionTimeout: 0,
        trace: "retain-on-failure",
      },
    },
  ],
});
