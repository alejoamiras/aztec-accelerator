/**
 * Appearance control against the real IPC bridge and a real webview.
 *
 * The desktop-ui suite drives this control too, but against static HTML in Chromium with a mocked
 * `invoke` — it can prove the page calls `set_theme` and never that anything happens as a result.
 * Everything that makes the feature work lives on the Rust side of that boundary: the command
 * writes config, then evaluates a script in every open window. These tests assert the observable
 * end of it, `data-theme` on a live `<html>`, which is the only thing the CSS actually keys off.
 *
 * System is the case worth having an E2E for at all: it must REMOVE the attribute rather than set
 * it to "system", and a unit test on the generated string cannot prove the removal reached a
 * document.
 */
import { ensureSettingsWindow, readConfig } from "./helpers.ts";

type Choice = "system" | "light" | "dark";

/**
 * The control ships disabled and settings.js enables it once the stored value is known, so every
 * read and every click has to wait for that. Waiting on existence alone would let assertions run
 * against the pre-hydration default and let a click no-op, which passes accidentally for System.
 */
async function waitForHydration(): Promise<void> {
  await browser.waitUntil(
    async () =>
      await browser.execute(() => {
        const group = document.querySelector<HTMLFieldSetElement>("#theme");
        // `!undefined` is true, so an optional-chained read would report hydration before the
        // control exists at all.
        return group !== null && !group.disabled;
      }),
    { timeout: 5000, timeoutMsg: "the appearance control never became enabled" },
  );
}

/**
 * Click the radio and wait for the round trip to land, rather than sleeping a guessed interval:
 * the click goes to Rust, which persists config and then evaluates the repaint script back into
 * this document.
 */
async function pick(choice: Choice): Promise<void> {
  await waitForHydration();
  await browser.execute((value: string) => {
    const input = document.querySelector<HTMLInputElement>(`#theme input[value="${value}"]`);
    if (!input) throw new Error(`no appearance radio for "${value}"`);
    input.click();
  }, choice);
  const expected = choice === "system" ? null : choice;
  await browser.waitUntil(
    async () => (await dataTheme()) === expected && readConfig().theme === choice,
    { timeout: 5000, timeoutMsg: `appearance never settled on "${choice}"` },
  );
}

const dataTheme = () => browser.execute(() => document.documentElement.getAttribute("data-theme"));

describe("Appearance", () => {
  let original: Choice;

  before(async () => {
    await ensureSettingsWindow();
    await waitForHydration();
    original = ((readConfig().theme as Choice) ?? "system") as Choice;
  });

  after(async () => {
    try {
      await pick(original);
    } catch (e) {
      console.error("Appearance cleanup failed:", e);
    }
  });

  it("should hydrate the control from the stored config", async () => {
    const checked = await browser.execute(
      () => document.querySelector<HTMLInputElement>("#theme input:checked")?.value ?? null,
    );
    expect(["system", "light", "dark"]).toContain(checked);
    expect(checked).toBe((readConfig().theme as string) ?? "system");
  });

  it("should apply data-theme to the live document and persist it", async () => {
    await pick("dark");
    expect(await dataTheme()).toBe("dark");
    expect(readConfig().theme).toBe("dark");

    await pick("light");
    expect(await dataTheme()).toBe("light");
    expect(readConfig().theme).toBe("light");
  });

  it("should clear data-theme for System rather than setting it", async () => {
    // Writing data-theme="system" would match no CSS override and strand the window on the light
    // palette, so the absence of the attribute IS the feature.
    await pick("dark");
    expect(await dataTheme()).toBe("dark");

    await pick("system");
    expect(await dataTheme()).toBe(null);
    expect(readConfig().theme).toBe("system");
  });

  it("should cache the choice where the init script can read it before first paint", async () => {
    // The init script is frozen when the window is built and replayed on every navigation, so it
    // resolves against this key instead of its baked value. Asserting the cache is the honest test:
    // the property it buys is the ABSENCE of a wrong-theme frame, and a WebDriver poll samples long
    // after any such frame is gone.
    await pick("dark");
    expect(await browser.execute(() => localStorage.getItem("presto.theme"))).toBe("dark");

    await pick("system");
    expect(await browser.execute(() => localStorage.getItem("presto.theme"))).toBe("system");
  });

  it("should still show the chosen theme after a reload", async () => {
    // End state only. The post-load re-assert would produce this even with a stale init script, so
    // this does NOT by itself prove the cache is doing its job — see the test above for that.
    // Neither test can prove the real property, the absence of a wrong-theme frame: WebDriver
    // samples long after any such frame is gone.
    await pick("dark");
    await browser.refresh();
    await waitForHydration();
    expect(await dataTheme()).toBe("dark");
  });
});
