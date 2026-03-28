/**
 * Authorization flow — verifies the full pipeline:
 * HTTP /prove with unknown origin → auth popup appears → user clicks Allow → origin saved.
 *
 * This is the highest-value WebDriver test because it exercises concurrent
 * HTTP + GUI interactions that can't be tested with mocked Playwright.
 *
 * On Linux (WebKitGTK), native WebDriver elementClick() returns "Unsupported result type"
 * even though the click fires successfully. We work around this by using JavaScript clicks
 * on Linux while keeping native clicks on macOS.
 */
import * as os from "node:os";

import { readConfig } from "./helpers.ts";

const IS_LINUX = os.platform() === "linux";
const TEST_ORIGIN = "https://test-e2e-webdriver.example.com";
const PROVE_URL = "http://127.0.0.1:59833/prove";

/**
 * Click an element by CSS selector. Uses JS click on Linux (WebKitGTK native
 * elementClick returns malformed response), native click on macOS.
 */
async function clickBy(selector: string): Promise<void> {
  if (IS_LINUX) {
    await browser.execute((sel: string) => {
      (document.querySelector(sel) as HTMLElement)?.click();
    }, selector);
    // Small pause — JS click is async, give the IPC time to process
    await browser.pause(200);
  } else {
    const el = await browser.$(selector);
    await el.click();
  }
}

/**
 * Remove the test origin via the Settings UI (Remove button).
 * This triggers the real IPC call which updates both in-memory config and disk.
 */
async function removeTestOriginViaUI(): Promise<void> {
  const url = await browser.getUrl();
  if (!url.includes("settings.html")) {
    await browser.navigateTo("tauri://localhost/settings.html");
    await browser.pause(500);
  }

  const speedLabel = await browser.$("#speed-label");
  await speedLabel.waitForExist({ timeout: 5000 });

  await browser.refresh();
  await browser.pause(500);

  const items = await browser.$$(".origin-item");
  for (const item of items) {
    const span = await item.$("span");
    const text = await span.getText();
    if (text === TEST_ORIGIN) {
      if (IS_LINUX) {
        const btn = await item.$("button");
        const btnId = await btn.getProperty("id");
        await browser.execute((id: string) => {
          document.getElementById(id)?.click();
        }, btnId as string);
        await browser.pause(500);
      } else {
        const removeBtn = await item.$("button");
        await removeBtn.click();
        await browser.pause(500);
      }
      return;
    }
  }
}

/** Close all windows except Settings, then switch back to Settings. */
async function closeExtraWindows(settingsHandle: string): Promise<void> {
  const handles = await browser.getWindowHandles();
  for (const h of handles) {
    if (h !== settingsHandle) {
      await browser.switchToWindow(h);
      await browser.closeWindow();
    }
  }
  if (handles.includes(settingsHandle)) {
    await browser.switchToWindow(settingsHandle);
  }
}

/**
 * Fire a /prove POST with an unknown origin. The request blocks until the
 * auth popup is resolved (Allow/Deny) or the 60s server timeout fires.
 */
function fireProveRequest(): Promise<Response> {
  return fetch(PROVE_URL, {
    method: "POST",
    headers: {
      "Content-Type": "application/octet-stream",
      Origin: TEST_ORIGIN,
    },
    body: new Uint8Array([0]),
  });
}

/** Poll getWindowHandles() until a new handle appears (up to 15s). */
async function waitForNewWindow(existingHandles: string[]): Promise<string | null> {
  for (let i = 0; i < 30; i++) {
    await browser.pause(500);
    const handlesNow = await browser.getWindowHandles();
    const newHandle = handlesNow.find((h) => !existingHandles.includes(h));
    if (newHandle) return newHandle;
  }
  return null;
}

describe("Authorization Flow", () => {
  let settingsHandle: string;
  let pendingProve: Promise<Response> | null = null;

  before(async () => {
    settingsHandle = await browser.getWindowHandle();
    await removeTestOriginViaUI();
  });

  beforeEach(async () => {
    await closeExtraWindows(settingsHandle);
    await browser.pause(300);
  });

  afterEach(async () => {
    if (pendingProve) {
      await pendingProve.catch(() => {});
      pendingProve = null;
    }
  });

  after(async () => {
    try {
      await closeExtraWindows(settingsHandle);
      await removeTestOriginViaUI();
    } catch (e) {
      console.error("Auth flow cleanup failed:", e);
    }
  });

  it("should show auth popup and allow with remember", async () => {
    const handlesBefore = await browser.getWindowHandles();

    pendingProve = fireProveRequest();

    const authWindowHandle = await waitForNewWindow(handlesBefore);
    expect(authWindowHandle).not.toBeNull();

    await browser.switchToWindow(authWindowHandle!);
    expect(await browser.getTitle()).toBe("Authorize Site");

    const originText = await browser.$("#origin");
    await originText.waitForExist({ timeout: 3000 });
    expect(await originText.getText()).toBe(TEST_ORIGIN);

    const remember = await browser.$("#remember");
    expect(await remember.isSelected()).toBe(true);

    // Click Allow — use JS click on Linux (WebKitGTK elementClick returns malformed response)
    await clickBy("#allow");

    const proveResponse = await pendingProve;
    pendingProve = null;
    expect(proveResponse.status).not.toBe(403);

    const config = readConfig();
    const origins = (config.approved_origins as string[]) || [];
    expect(origins).toContain(TEST_ORIGIN);

    await browser.switchToWindow(settingsHandle);

    await browser.refresh();
    await browser.pause(500);
    await removeTestOriginViaUI();
  });

  it("should deny and return 403 when Deny is clicked", async () => {
    const handlesBefore = await browser.getWindowHandles();

    pendingProve = fireProveRequest();

    const authWindowHandle = await waitForNewWindow(handlesBefore);
    expect(authWindowHandle).not.toBeNull();

    await browser.switchToWindow(authWindowHandle!);

    await clickBy("#deny");

    const proveResponse = await pendingProve;
    pendingProve = null;
    expect(proveResponse.status).toBe(403);

    const config = readConfig();
    const origins = (config.approved_origins as string[]) || [];
    expect(origins).not.toContain(TEST_ORIGIN);

    await browser.switchToWindow(settingsHandle);
  });

  it("should allow without remembering when Remember is unchecked", async () => {
    const handlesBefore = await browser.getWindowHandles();

    pendingProve = fireProveRequest();

    const authWindowHandle = await waitForNewWindow(handlesBefore);
    expect(authWindowHandle).not.toBeNull();

    await browser.switchToWindow(authWindowHandle!);

    // Uncheck Remember via JS click
    await clickBy("#remember");
    // Verify it's unchecked (native isSelected works fine on Linux)
    const remember = await browser.$("#remember");
    expect(await remember.isSelected()).toBe(false);

    await clickBy("#allow");

    const proveResponse = await pendingProve;
    pendingProve = null;
    expect(proveResponse.status).not.toBe(403);

    const config = readConfig();
    const origins = (config.approved_origins as string[]) || [];
    expect(origins).not.toContain(TEST_ORIGIN);

    await browser.switchToWindow(settingsHandle);
  });
});
