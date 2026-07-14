import path from "node:path";
import { expect, type Page, test } from "@playwright/test";

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

// ── Tests ──

test("shows decoded origin from URL params", async ({ page }) => {
  await page.goto("/authorize.html?origin=https%3A%2F%2Fexample.com");
  await expect(page.locator("#origin")).toHaveText("https://example.com");
});

test("allow calls respond_auth with correct args", async ({ page }) => {
  await page.goto("/authorize.html?origin=https%3A%2F%2Fexample.com&requestId=req-abc");

  await page.getByRole("button", { name: "Allow" }).click();

  const calls = await callsFor(page, "respond_auth");
  expect(calls.length).toBe(1);
  // SEC-06: the opaque requestId from the URL is echoed back so the server resolves by id.
  // F-014: "Always allow this site" defaults to UNCHECKED — a plain Allow is ephemeral ("Allow once").
  expect(calls[0].args).toEqual({
    requestId: "req-abc",
    origin: "https://example.com",
    allowed: true,
    remember: false,
  });
});

test("deny calls respond_auth with correct args", async ({ page }) => {
  await page.goto("/authorize.html?origin=https%3A%2F%2Fexample.com&requestId=req-abc");

  await page.getByRole("button", { name: "Deny" }).click();

  const calls = await callsFor(page, "respond_auth");
  expect(calls.length).toBe(1);
  // F-014: default-unchecked — deny sends remember: false.
  expect(calls[0].args).toEqual({
    requestId: "req-abc",
    origin: "https://example.com",
    allowed: false,
    remember: false,
  });
});

test("buttons disabled after invoke prevents double-click", async ({ page }) => {
  await page.goto("/authorize.html?origin=https%3A%2F%2Fexample.com");

  await page.getByRole("button", { name: "Allow" }).click();

  // wireButton disables both buttons on success. In production, Rust closes the
  // window immediately — here we verify the frontend prevents double-clicks.
  await expect(page.getByRole("button", { name: "Allow" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Deny" })).toBeDisabled();
});

test("remember defaults unchecked; checking 'Always allow' sends remember: true", async ({
  page,
}) => {
  await page.goto("/authorize.html?origin=https%3A%2F%2Ftest.com&requestId=req-xyz");

  // F-014: the checkbox is UNCHECKED by default (deliberate opt-in to persistent trust).
  await expect(page.locator("#remember")).not.toBeChecked();

  // Opting in ("Always allow this site") sends remember: true.
  await page.locator("#remember").check();
  await page.getByRole("button", { name: "Allow" }).click();

  const calls = await callsFor(page, "respond_auth");
  expect(calls[0].args).toEqual({
    requestId: "req-xyz",
    origin: "https://test.com",
    allowed: true,
    remember: true,
  });
});

test("full origin untruncated + start reachable + buttons in the 400x300 window (F-014)", async ({
  page,
}) => {
  // The real Tauri popup window is 400x300; a very long look-alike origin must show its FULL text AND
  // its BEGINNING (scheme/leading labels) must be reachable — a centered overflow would hide the start.
  await page.setViewportSize({ width: 400, height: 300 });
  const long = `https://${"sub.".repeat(40)}trusted.example`;
  await page.goto(`/authorize.html?origin=${encodeURIComponent(long)}`);
  const origin = page.locator("#origin");
  await expect(origin).toHaveText(long); // full text, no ellipsis/truncation
  await expect(origin).toHaveAttribute("dir", "ltr");

  // HIGH regression guard: scroll the region to the top and assert the origin's top edge is not clipped
  // ABOVE the scrollport (centered overflow would push the scheme into unreachable negative scroll).
  const startReachable = await page.evaluate(() => {
    const scroll = document.querySelector(".popup-scroll");
    const o = document.getElementById("origin");
    if (!scroll || !o) return false;
    scroll.scrollTop = 0;
    return o.getBoundingClientRect().top >= scroll.getBoundingClientRect().top - 1;
  });
  expect(startReachable).toBe(true);

  // Allow/Deny stay inside the 300px-tall window (reachable footer, not pushed off).
  for (const name of ["Allow", "Deny"]) {
    const box = await page.getByRole("button", { name }).boundingBox();
    expect(box, name).not.toBeNull();
    expect(box!.y + box!.height, name).toBeLessThanOrEqual(300);
  }

  // Bidi isolation + selectability are actually applied (defeat visual reordering; allow inspect/copy).
  const style = await origin.evaluate((el) => {
    const s = getComputedStyle(el);
    return {
      bidi: s.unicodeBidi,
      select: s.userSelect || (s as unknown as { webkitUserSelect: string }).webkitUserSelect,
    };
  });
  expect(style.bidi).toContain("isolate");
  expect(style.select).toBe("text");
});

test("missing origin param shows unknown", async ({ page }) => {
  await page.goto("/authorize.html");
  await expect(page.locator("#origin")).toHaveText("unknown");
});

test("blank origin param shows unknown", async ({ page }) => {
  await page.goto("/authorize.html?origin=");
  await expect(page.locator("#origin")).toHaveText("unknown");
});

test("error on invoke re-enables buttons and shows hint", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("respond_auth", () => {
      throw new Error("Auth failed");
    });
  });
  await page.goto("/authorize.html?origin=https%3A%2F%2Fexample.com");

  await page.getByRole("button", { name: "Allow" }).click();

  // wireButton error path: buttons re-enabled, error hint shown
  await expect(page.getByRole("button", { name: "Allow" })).not.toBeDisabled();
  await expect(page.getByRole("button", { name: "Deny" })).not.toBeDisabled();
  await expect(page.locator(".error-hint")).toBeVisible();
});

// ── Recognized-site badge ─────────────────────────────────────────────

test("recognized origin renders friendly name + verified check + raw origin", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_verified_info", () => ({
      display_name: "Nulo Wallet",
    }));
  });
  await page.goto("/authorize.html?origin=https%3A%2F%2Fnulo.sh");

  await expect(page.locator("#recognized")).toBeVisible();
  await expect(page.locator(".recognized-name")).toHaveText("Nulo Wallet");
  await expect(page.locator(".verified-check")).toBeVisible();
  // Raw origin still shown — auditor requirement: don't bury the URL the user must verify.
  await expect(page.locator("#origin")).toHaveText("https://nulo.sh");
});

test("unrecognized origin hides badge and shows only raw origin", async ({ page }) => {
  // Default mock for get_verified_info returns null (defined in tauri-mock.js defaults).
  await page.goto("/authorize.html?origin=https%3A%2F%2Funknown.example.com");

  await expect(page.locator("#recognized")).toBeHidden();
  await expect(page.locator(".verified-check")).toBeHidden();
  await expect(page.locator("#origin")).toHaveText("https://unknown.example.com");
});

test("get_verified_info is called with the raw origin", async ({ page }) => {
  await page.goto("/authorize.html?origin=https%3A%2F%2Fnulo.sh");

  const calls = await callsFor(page, "get_verified_info");
  expect(calls.length).toBe(1);
  expect(calls[0].args).toEqual({ origin: "https://nulo.sh" });
});

test("IPC failure on get_verified_info falls back to unrecognized rendering", async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_verified_info", () => {
      throw new Error("IPC down");
    });
  });
  await page.goto("/authorize.html?origin=https%3A%2F%2Fnulo.sh");

  await expect(page.locator("#recognized")).toBeHidden();
  await expect(page.locator("#origin")).toHaveText("https://nulo.sh");
});
