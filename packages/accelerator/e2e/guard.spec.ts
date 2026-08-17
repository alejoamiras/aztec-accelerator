import path from "node:path";
import { expect, type Page, test } from "@playwright/test";

// B2 (F-10) click-steal guard coverage. The guard is DEFAULT-ON in `wireButton`, so every consent
// popup's consequential control ignores a click that lands inside the guard window after the control
// became actionable. `guardMs()` reads `window.__CLICK_GUARD_MS__` dynamically, which lets these tests
// be deterministic: set a LARGE window so the immediate click is reliably inside it, assert no IPC, then
// zero the window at runtime and click again — no wall-clock racing for the default-on cases.

const MOCK_PATH = path.join(import.meta.dirname, "tauri-mock.js");

async function callsFor(page: Page, cmd: string) {
  const calls = await page.evaluate(() => (window as any).__TAURI_MOCK__.calls);
  return calls.filter((c: any) => c.cmd === cmd);
}

const jsErrors: string[] = [];

test.beforeEach(async ({ page }) => {
  jsErrors.length = 0;
  page.on("pageerror", (err) => jsErrors.push(err.message));
  await page.addInitScript({ path: MOCK_PATH });
});

test.afterEach(() => {
  expect(jsErrors, "Unexpected JS runtime errors").toEqual([]);
});

// ── Default-on guard on every popup wireButton uses (onboarding / renewal / update-prompt) ──
// These three popups do NOT pass `guard` at all — they inherit the default. Reverting the default-on
// change in wireButton (`opts.guard !== false` → `opts.guard`) makes the first click fire, failing
// the "ignored" assertion below: the mutation-proof for B2's headline fix.

const POPUPS = [
  { name: "onboarding", url: "/onboarding.html", buttonId: "#start", cmd: "complete_onboarding" },
  { name: "renewal", url: "/renewal.html", buttonId: "#renew", cmd: "renew_cert" },
  {
    name: "update-prompt",
    url: "/update-prompt.html?current=1.0.0&version=1.1.0",
    buttonId: "#update",
    cmd: "respond_update_prompt",
  },
];

for (const p of POPUPS) {
  test(`${p.name}: click inside the guard window is ignored, click after it fires`, async ({
    page,
  }) => {
    // Large window so the immediate click is unambiguously inside it (Playwright click overhead is far
    // under 5 s). Set AFTER the mock's init script, which zeroes the guard by default.
    await page.addInitScript(() => {
      (window as any).__CLICK_GUARD_MS__ = 5000;
    });
    await page.goto(p.url);

    await page.locator(p.buttonId).click();
    expect(
      (await callsFor(page, p.cmd)).length,
      `${p.name}: a click inside the guard window must be ignored`,
    ).toBe(0);

    // Clear the guard at runtime (read dynamically by guardMs) — no wall-clock wait.
    await page.evaluate(() => {
      (window as any).__CLICK_GUARD_MS__ = 0;
    });
    await page.locator(p.buttonId).click();
    expect(
      (await callsFor(page, p.cmd)).length,
      `${p.name}: a click after the guard clears must fire`,
    ).toBeGreaterThanOrEqual(1);
  });
}

// ── The default window is a real ~700 ms guard, not disabled ──
// Guards against silently weakening DEFAULT_GUARD_MS to 0 (or blowing it up). Deletes the override so
// the real default applies; brackets it as > click-overhead and < 800 ms.
test("the default guard window (no override) is a real, non-trivial window", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__CLICK_GUARD_MS__ = undefined;
    delete (window as any).__CLICK_GUARD_MS__;
  });
  await page.goto("/update-prompt.html?current=1.0.0&version=1.1.0");

  await page.locator("#update").click();
  expect(
    (await callsFor(page, "respond_update_prompt")).length,
    "the default guard must ignore an immediate click",
  ).toBe(0);

  await page.waitForTimeout(800); // > DEFAULT_GUARD_MS (700)
  await page.locator("#update").click();
  expect(
    (await callsFor(page, "respond_update_prompt")).length,
    "the default guard must clear within 800 ms",
  ).toBeGreaterThanOrEqual(1);
});

// ── F7: the guard re-arms on the queued→active promotion, not just on native focus ──
// A queued auth popup's controls are disabled; when the arbiter promotes it to active, authorize.js
// calls rearmClickGuard() on that exact false→true edge. Let the load-time arming decay first, so the
// ONLY thing that can re-arm the guard by promotion time is the F7 call. Reverting that call lets the
// post-promotion click fire (guard already decayed) → this test fails.
test("authorize: promotion to active re-arms the click-steal guard", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__CLICK_GUARD_MS__ = 1500;
    let active = false;
    (window as any).__promote = () => {
      active = true;
    };
    (window as any).__TAURI_MOCK__.setHandler("get_pending_auth", () => ({
      origin: "https://example.com",
      active,
    }));
  });
  await page.goto("/authorize.html?requestId=r1");

  // Queued: controls disabled.
  await expect(page.locator("#allow")).toBeDisabled();
  // Let the load/pageshow arming fully decay (> guard window) so promotion is the only re-arm.
  await page.waitForTimeout(1700);

  // Promote; the ≤1 s poll sees active:true, fires rearmClickGuard(), and enables the controls.
  await page.evaluate(() => (window as any).__promote());
  await expect(page.locator("#allow")).toBeEnabled();

  // A click landing right as the controls light up is inside the re-armed window → ignored.
  await page.locator("#allow").click();
  expect(
    (await callsFor(page, "respond_auth")).length,
    "a click within the promotion-armed guard must be ignored",
  ).toBe(0);
});
