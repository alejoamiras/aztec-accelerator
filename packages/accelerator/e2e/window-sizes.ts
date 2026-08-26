/**
 * The REAL Tauri window inner sizes, mirrored from `src-tauri/src/windows.rs`.
 *
 * Playwright sets no viewport by default, so the desktop-ui specs ran at 1280x720 while these windows
 * are 500x600 and 520x560. Anything that overflows the real window fits comfortably in a 720px-tall
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
 * Measured against a real `--features webdriver` build on macOS 26.5: a window built
 * `inner_size(500, 600)` reports `innerHeight === 568`. Linux and Windows are unmeasured, so this is
 * the worst case there is evidence for rather than a per-platform model.
 *
 * It is a row of content, not a hairline. Sizing a layout spec to the WINDOW height passes any page
 * that fits in 600 while the user sees 568, which is the exact shape of the two clipping bugs that
 * reached the owner instead of CI.
 */
export const WEBVIEW_CHROME_HEIGHT = 32;

/** What a layout spec must size the page to: the window minus its chrome. */
export const VIEWPORT_SIZES = Object.fromEntries(
  Object.entries(WINDOW_SIZES).map(([label, { width, height }]) => [
    label,
    { width, height: height - WEBVIEW_CHROME_HEIGHT },
  ]),
) as { [K in keyof typeof WINDOW_SIZES]: { width: number; height: number } };
