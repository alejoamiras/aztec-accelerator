import path from "node:path";
import { expect, type Page, test } from "@playwright/test";
import { WINDOW_SIZES } from "./window-sizes.js";

const MOCK_PATH = path.join(import.meta.dirname, "tauri-mock.js");

async function getCalls(page: Page) {
  return page.evaluate(() => (window as any).__TAURI_MOCK__.calls);
}
async function callsFor(page: Page, cmd: string) {
  const calls = await getCalls(page);
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

test("all three toggles pre-checked; HTTPS follows https_default", async ({ page }) => {
  await page.goto("/onboarding.html");
  await expect(page.locator("#opt-https")).toBeChecked();
  await expect(page.locator("#opt-autostart")).toBeChecked();
  await expect(page.locator("#opt-auto-update")).toBeChecked();
});

test("HTTPS explanation shows the per-OS certificate copy", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_onboarding_state", () => ({
      platform: "linux",
      https_default: true,
      autostart_enabled: false,
      auto_update: null,
      trust_status: { stores: [] },
    }));
  });
  await page.goto("/onboarding.html");
  // Substring only (not the full sentence) so wording tweaks don't break the per-OS-copy assertion.
  await expect(page.locator("#https-warn")).toContainText("separate prompt");
});

test("HTTPS stays pre-checked for an upgrader on any platform (A9)", async ({ page }) => {
  // A9/§2.1: HTTPS is pre-checked for everyone incl. upgraders. The wizard is a recommended setup,
  // so Start-on-Login + Auto-Update also default YES regardless of prior OS/config state.
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_onboarding_state", () => ({
      platform: "linux",
      https_default: true,
    }));
  });
  await page.goto("/onboarding.html");
  await expect(page.locator("#opt-https")).toBeChecked();
  await expect(page.locator("#opt-autostart")).toBeChecked();
  await expect(page.locator("#opt-auto-update")).toBeChecked();
});

test("Start invokes complete_onboarding with the toggle states; on success the page waits for the Rust-side close", async ({
  page,
}) => {
  await page.goto("/onboarding.html");
  // The toggle <input> is visually hidden under the slider span, so Playwright's native uncheck()
  // (a real click) times out — set the property directly, as the settings specs do, and dispatch
  // `change` so the adaptive primary label updates too.
  await page.locator("#opt-auto-update").evaluate((el: HTMLInputElement) => {
    el.checked = false;
    el.dispatchEvent(new Event("change"));
  });
  await page.locator("#start").click();

  const calls = await callsFor(page, "complete_onboarding");
  expect(calls.length).toBe(1);
  expect(calls[0].args).toEqual({ https: true, autostart: true, autoUpdate: false });
  // completed:true (mock default) → Rust closes the window (F-012 — the page can't close itself);
  // the frontend leaves the button disabled while it waits for that close.
  await expect(page.locator("#start")).toBeDisabled();
});

test("a declined option is confirmed back as Off, not On", async ({ page }) => {
  // `complete_onboarding` returns Ok for "set it off" exactly as for "set it on", so a hardcoded "On"
  // told a user who had just unchecked Start-on-Login that it was enabled (post-impl codex Low).
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("complete_onboarding", () => ({
      https: { Ok: null },
      autostart: { Ok: null },
      auto_update: { Ok: null },
      completed: false, // keeps the window open so the result rows stay readable
    }));
  });
  await page.goto("/onboarding.html");
  await page.locator("#opt-autostart").evaluate((el: HTMLInputElement) => {
    el.checked = false;
    el.dispatchEvent(new Event("change"));
  });
  await page.locator("#start").click();

  await expect(page.locator("#autostart-result")).toHaveText("Off");
  await expect(page.locator("#auto-update-result")).toHaveText("On");
});

test("partial cert failure: HTTPS shown off with Retry, other choices still applied", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("complete_onboarding", () => ({
      https: { Err: "certutil not found" },
      autostart: { Ok: null },
      auto_update: { Ok: null },
      completed: false,
    }));
  });
  await page.goto("/onboarding.html");
  await page.locator("#start").click();

  await expect(page.locator("#https-result")).toContainText("certutil not found");
  await expect(page.locator("#opt-https")).not.toBeChecked();
  await expect(page.locator("#https-retry")).toBeVisible();
  // completed:false → Rust does NOT close the window; the primary button is re-enabled so the user can
  // Retry, or press it again (HTTPS now unchecked) to continue without HTTPS. With HTTPS off but the
  // other two still on, the label stays "Start".
  await expect(page.locator("#start")).toBeEnabled();
  await expect(page.locator("#start")).toHaveText("Start");
});

test("Retry re-checks HTTPS and re-enables Start", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("complete_onboarding", () => ({
      https: { Err: "boom" },
      autostart: { Ok: null },
      auto_update: { Ok: null },
      completed: false,
    }));
  });
  await page.goto("/onboarding.html");
  await page.locator("#start").click();
  await expect(page.locator("#https-retry")).toBeVisible();

  await page.locator("#https-retry").click();
  await expect(page.locator("#opt-https")).toBeChecked();
  await expect(page.locator("#https-retry")).not.toBeVisible();
});

/** Set a visually-hidden toggle and fire `change` (what the adaptive label listens for). */
async function setToggle(page: Page, id: string, checked: boolean) {
  await page.locator(id).evaluate((el: HTMLInputElement, value: boolean) => {
    el.checked = value;
    el.dispatchEvent(new Event("change"));
  }, checked);
}

const ALL_TOGGLES = ["#opt-https", "#opt-autostart", "#opt-auto-update"];

test("the primary label follows the toggles (Start when any is on, Continue when all are off)", async ({
  page,
}) => {
  // There is no Skip button: unchecking everything IS the decline. "Start" would read wrong for that,
  // so the label adapts.
  await page.goto("/onboarding.html");
  await expect(page.locator("#start")).toHaveText("Start");

  for (const id of ALL_TOGGLES) await setToggle(page, id, false);
  await expect(page.locator("#start")).toHaveText("Continue");

  // Any single toggle back on flips it back.
  await setToggle(page, "#opt-autostart", true);
  await expect(page.locator("#start")).toHaveText("Start");
});

test("declining everything is an explicit, recorded choice", async ({ page }) => {
  // The old Skip button set the marker while changing nothing. Now the same outcome goes through
  // complete_onboarding with all three false, so the decision is explicit AND persisted.
  await page.goto("/onboarding.html");
  for (const id of ALL_TOGGLES) await setToggle(page, id, false);
  await page.locator("#start").click();

  const calls = await callsFor(page, "complete_onboarding");
  expect(calls.length).toBe(1);
  expect(calls[0].args).toEqual({ https: false, autostart: false, autoUpdate: false });
});

test("the wizard offers no Skip affordance", async ({ page }) => {
  // Guards the decision: a second route to "no" beside the primary action invites reflexive
  // dismissal, and closing the window (marker unset → wizard returns) is the intended escape.
  await page.goto("/onboarding.html");
  await expect(page.locator("#skip")).toHaveCount(0);
});

// ── Layout at the REAL window size ──
//
// The wizard's height was set twice by owner feedback ("huge margin" at 600, "too short" at 510)
// because it could not be measured here: Playwright ran at 1280x720, where a 560px-tall window's
// overflow does not exist. Sized to the real window, the clipping is observable.
async function overflow(page: Page) {
  return page.evaluate(() => ({
    content: document.documentElement.scrollHeight,
    window: document.documentElement.clientHeight,
    canScroll: getComputedStyle(document.body).overflowY !== "hidden",
  }));
}

test("the pre-Start wizard fits its window without needing to scroll", async ({ page }) => {
  // The state the user sees on first launch, and the one the 560px height was chosen for. If this
  // starts overflowing, the height is wrong again — or a row was added without revisiting it.
  await page.setViewportSize(WINDOW_SIZES.onboarding);
  await page.goto("/onboarding.html");

  const { content, window } = await overflow(page);
  expect(content, `pre-Start content is ${content}px in a ${window}px window`).toBeLessThanOrEqual(
    window,
  );
  await expect(page.locator("#start")).toBeInViewport({ ratio: 1 });
});

test("the taller post-Start state stays reachable by scrolling", async ({ page }) => {
  // Three result lines plus Retry legitimately exceed the window. That is handled by scrolling
  // rather than by sizing the window for a state it holds for seconds — but it MUST scroll.
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("complete_onboarding", () => ({
      https: { Err: "certutil not found" },
      autostart: { Ok: null },
      auto_update: { Ok: null },
      completed: false,
    }));
  });
  await page.setViewportSize(WINDOW_SIZES.onboarding);
  await page.goto("/onboarding.html");
  await page.locator("#start").click();
  await expect(page.locator("#https-retry")).toBeVisible();

  const { content, window, canScroll } = await overflow(page);
  if (content > window) {
    expect(canScroll, "post-Start content overflows but the page cannot scroll").toBe(true);
  }
  await page.locator("#start").scrollIntoViewIfNeeded();
  await expect(page.locator("#start")).toBeInViewport({ ratio: 1 });
});
