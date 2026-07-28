/**
 * WebdriverIO configuration for Tauri E2E tests.
 *
 * Connects to the tauri-plugin-webdriver server embedded in the app (port 4445).
 * The Tauri app must already be running with `--features webdriver` before tests start.
 */
export const config: WebdriverIO.Config = {
  runner: "local",
  hostname: "127.0.0.1",
  port: 4445,
  path: "/",

  framework: "mocha",
  reporters: ["spec"],
  // Explicit order: smoke first (basic health), settings, trust-boundary (F-012), auth-flow, then
  // autostart LAST — it seeds a broken on-disk artifact and refreshes the Settings window, so it
  // must never run before the specs that assert the pristine unchecked state.
  specs: [
    "./e2e-webdriver/smoke.spec.ts",
    "./e2e-webdriver/settings.spec.ts",
    "./e2e-webdriver/trust-boundary.spec.ts",
    "./e2e-webdriver/auth-flow.spec.ts",
    "./e2e-webdriver/autostart.spec.ts",
  ],

  // Tests share a single app instance + config file — run sequentially
  maxInstances: 1,
  capabilities: [
    {
      // The embedded WebDriver plugin reports "webkit" for WKWebView
      browserName: "webkit",
    },
  ],

  // Timeouts
  waitforTimeout: 10_000,
  connectionRetryTimeout: 30_000,
  connectionRetryCount: 3,
  mochaOpts: {
    timeout: 30_000,
  },
};
