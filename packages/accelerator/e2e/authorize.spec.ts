import path from "node:path";
import { expect, type Page, test } from "@playwright/test";

const MOCK_PATH = path.join(import.meta.dirname, "tauri-mock.js");

async function callsFor(page: Page, cmd: string) {
  const calls = await page.evaluate(() => (window as any).__TAURI_MOCK__.calls);
  return calls.filter((c: any) => c.cmd === cmd);
}

/** Override get_pending_auth BEFORE navigation (the popup fetches its origin from the server, C9 D8). */
async function mockPending(page: Page, origin: string, active = true) {
  await page.addInitScript(
    ([o, a]) => {
      (window as any).__TAURI_MOCK__.setHandler("get_pending_auth", () => ({
        origin: o,
        active: a,
      }));
    },
    [origin, active] as const,
  );
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

// ── Origin is SERVER-authoritative (C9 D8), not from the URL param ──

test("shows the server origin from get_pending_auth, ignoring the URL param", async ({ page }) => {
  await mockPending(page, "https://example.com");
  // A different (attacker-controlled) URL origin must NOT be what renders.
  await page.goto("/authorize.html?origin=https%3A%2F%2Fevil.example&requestId=req-abc");
  await expect(page.locator("#origin")).toHaveText("https://example.com");
});

test("allow calls respond_auth with the SERVER origin + URL requestId", async ({ page }) => {
  await mockPending(page, "https://example.com");
  await page.goto("/authorize.html?origin=https%3A%2F%2Fevil.example&requestId=req-abc");

  await page.getByRole("button", { name: "Allow" }).click();

  const calls = await callsFor(page, "respond_auth");
  expect(calls.length).toBe(1);
  // requestId from the URL (SEC-06 opaque id); origin from the server (D8). remember default-UNCHECKED (F-014).
  expect(calls[0].args).toEqual({
    requestId: "req-abc",
    origin: "https://example.com",
    allowed: true,
    remember: false,
  });
});

test("deny calls respond_auth with the server origin", async ({ page }) => {
  await mockPending(page, "https://example.com");
  await page.goto("/authorize.html?origin=https%3A%2F%2Fexample.com&requestId=req-abc");

  await page.getByRole("button", { name: "Deny" }).click();

  const calls = await callsFor(page, "respond_auth");
  expect(calls[0].args).toEqual({
    requestId: "req-abc",
    origin: "https://example.com",
    allowed: false,
    remember: false,
  });
});

test("remember defaults unchecked; checking 'Always allow' sends remember: true", async ({
  page,
}) => {
  await mockPending(page, "https://test.com");
  await page.goto("/authorize.html?requestId=req-xyz");

  await expect(page.locator("#remember")).not.toBeChecked();
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

// ── Arbiter: active vs queued (C9 D15/D19) ──

test("an ACTIVE popup enables its buttons", async ({ page }) => {
  await mockPending(page, "https://example.com", true);
  await page.goto("/authorize.html?requestId=req-abc");
  await expect(page.getByRole("button", { name: "Allow" })).toBeEnabled();
  await expect(page.getByRole("button", { name: "Deny" })).toBeEnabled();
});

test("a QUEUED popup keeps its buttons disabled until promoted", async ({ page }) => {
  await mockPending(page, "https://example.com", false); // active:false ⇒ queued
  await page.goto("/authorize.html?requestId=req-abc");
  await expect(page.getByRole("button", { name: "Allow" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Deny" })).toBeDisabled();
});

test("buttons disabled after invoke prevents double-click", async ({ page }) => {
  await mockPending(page, "https://example.com");
  await page.goto("/authorize.html?requestId=req-abc");

  await page.getByRole("button", { name: "Allow" }).click();

  await expect(page.getByRole("button", { name: "Allow" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Deny" })).toBeDisabled();
});

test("full origin untruncated + start reachable + buttons in the 400x300 window (F-014)", async ({
  page,
}) => {
  await page.setViewportSize({ width: 400, height: 300 });
  const long = `https://${"sub.".repeat(40)}trusted.example`;
  await mockPending(page, long);
  await page.goto("/authorize.html?requestId=req-abc");
  const origin = page.locator("#origin");
  await expect(origin).toHaveText(long); // full text, no ellipsis/truncation
  await expect(origin).toHaveAttribute("dir", "ltr");

  const startReachable = await page.evaluate(() => {
    const scroll = document.querySelector(".popup-scroll");
    const o = document.getElementById("origin");
    if (!scroll || !o) return false;
    scroll.scrollTop = 0;
    return o.getBoundingClientRect().top >= scroll.getBoundingClientRect().top - 1;
  });
  expect(startReachable).toBe(true);

  for (const name of ["Allow", "Deny"]) {
    const box = await page.getByRole("button", { name }).boundingBox();
    expect(box, name).not.toBeNull();
    expect(box!.y + box!.height, name).toBeLessThanOrEqual(300);
  }

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

test("error on respond_auth re-enables buttons and shows hint", async ({ page }) => {
  await mockPending(page, "https://example.com");
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("respond_auth", () => {
      throw new Error("Auth failed");
    });
  });
  await page.goto("/authorize.html?requestId=req-abc");

  await page.getByRole("button", { name: "Allow" }).click();

  await expect(page.getByRole("button", { name: "Allow" })).not.toBeDisabled();
  await expect(page.getByRole("button", { name: "Deny" })).not.toBeDisabled();
  await expect(page.locator(".error-hint")).toBeVisible();
});

// ── Recognized-site badge (keyed on the SERVER origin, C9 D8) ──

test("recognized origin renders friendly name + verified check + raw origin", async ({ page }) => {
  await mockPending(page, "https://nulo.sh");
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_verified_info", () => ({
      display_name: "Nulo Wallet",
    }));
  });
  await page.goto("/authorize.html?requestId=req-abc");

  await expect(page.locator("#recognized")).toBeVisible();
  await expect(page.locator(".recognized-name")).toHaveText("Nulo Wallet");
  await expect(page.locator(".verified-check")).toBeVisible();
  await expect(page.locator("#origin")).toHaveText("https://nulo.sh");
});

test("unrecognized origin hides badge and shows only raw origin", async ({ page }) => {
  await mockPending(page, "https://unknown.example.com");
  await page.goto("/authorize.html?requestId=req-abc");

  await expect(page.locator("#recognized")).toBeHidden();
  await expect(page.locator(".verified-check")).toBeHidden();
  await expect(page.locator("#origin")).toHaveText("https://unknown.example.com");
});

test("get_verified_info is called with the SERVER origin", async ({ page }) => {
  await mockPending(page, "https://nulo.sh");
  await page.goto("/authorize.html?origin=https%3A%2F%2Fevil.example&requestId=req-abc");

  await expect(page.locator("#origin")).toHaveText("https://nulo.sh");
  const calls = await callsFor(page, "get_verified_info");
  expect(calls.length).toBeGreaterThanOrEqual(1);
  expect(calls[0].args).toEqual({ origin: "https://nulo.sh" });
});

test("IPC failure on get_verified_info falls back to unrecognized rendering", async ({ page }) => {
  await mockPending(page, "https://nulo.sh");
  await page.addInitScript(() => {
    (window as any).__TAURI_MOCK__.setHandler("get_verified_info", () => {
      throw new Error("IPC down");
    });
  });
  await page.goto("/authorize.html?requestId=req-abc");

  await expect(page.locator("#origin")).toHaveText("https://nulo.sh");
  await expect(page.locator("#recognized")).toBeHidden();
});
