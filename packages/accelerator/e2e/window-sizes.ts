/**
 * The Tauri window sizes, mirrored from `src-tauri/src/windows.rs`. These are what the windows are
 * BUILT with, which is not what the page gets — see `WEBVIEW_CHROME_HEIGHT`.
 *
 * Playwright sets no viewport by default, so the desktop-ui specs ran at 1280x720 while these windows
 * are 500x664 and 520x592. Anything that overflows the real window fits comfortably in a 720px-tall
 * browser viewport, which is structurally why two clipping bugs (the cut-off speed slider, the
 * onboarding height) reached the owner instead of CI. Specs that care about layout must size the page
 * to these values.
 *
 * `scripts/tauri-trust-boundary.test.ts` fails if these drift from windows.rs.
 *
 * These are WINDOW sizes. Layout specs must size the page to `VIEWPORT_SIZES` below instead — the
 * webview gets less height than the window is built with.
 */
export const WINDOW_SIZES = {
  settings: { width: 500, height: 664 },
  onboarding: { width: 520, height: 592 },
  renewal: { width: 420, height: 320 },
  "update-prompt": { width: 420, height: 280 },
  // The auth popup's label is per-request (`auth-<id>`), so the drift guard keys it by its url.
  authorize: { width: 400, height: 300 },
} as const;

/**
 * Height the title bar takes out of the webview viewport, despite `inner_size` naming it "inner".
 *
 * ONE measurement, not a worst case: a real `--features webdriver` build on macOS 26.5 built
 * `inner_size(500, 600)` and reported `innerHeight === 568`. Linux and Windows have never been
 * measured, and nothing here proves 32 is their ceiling — if either spends more, these specs stay
 * green while that platform clips. Closing that needs a per-platform webdriver measurement.
 *
 * It still beats sizing to the window, which passes any page fitting in 600 while the user sees 568
 * — the exact shape of the two clipping bugs that reached the owner instead of CI. Treat a page
 * clearing this by only a few px as unproven rather than safe; `body.scrollable` on settings,
 * onboarding and renewal is what keeps an under-estimate a scroll instead of a clip.
 */
export const WEBVIEW_CHROME_HEIGHT = 32;

/** What a layout spec must size the page to: the window minus its chrome. */
export const VIEWPORT_SIZES = Object.fromEntries(
  Object.entries(WINDOW_SIZES).map(([label, { width, height }]) => [
    label,
    { width, height: height - WEBVIEW_CHROME_HEIGHT },
  ]),
) as { [K in keyof typeof WINDOW_SIZES]: { width: number; height: number } };
