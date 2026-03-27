/**
 * Smoke test — verifies the Tauri app launched correctly and is drivable via WebDriver.
 *
 * This is the most basic pre-release gate: if the app can't start, render a window,
 * and serve /health, something is fundamentally broken.
 */
describe("Smoke", () => {
  it("should have the Settings window open", async () => {
    // The bootstrap window (Settings) opens automatically in webdriver mode
    const title = await browser.getTitle();
    expect(title).toBe("Aztec Accelerator Settings");
  });

  it("should render the Settings page with a speed label", async () => {
    const speedLabel = await browser.$("#speed-label");
    await speedLabel.waitForExist({ timeout: 5000 });
    const text = await speedLabel.getText();
    // Speed label shows the current speed setting (e.g. "Full")
    expect(text.length).toBeGreaterThan(0);
  });

  it("should have the HTTP server running on port 59833", async () => {
    // Hit the /health endpoint directly from the test process (not via WebDriver)
    const res = await fetch("http://127.0.0.1:59833/health");
    expect(res.ok).toBe(true);

    const health = (await res.json()) as Record<string, unknown>;
    expect(health.status).toBe("ok");
    expect(health.api_version).toBe(1);
    expect(typeof health.bb_available).toBe("boolean");
    expect(Array.isArray(health.available_versions)).toBe(true);
  });
});
