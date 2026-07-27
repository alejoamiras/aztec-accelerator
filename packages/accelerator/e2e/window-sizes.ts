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
 * Caveat worth knowing: this is the window INNER size. The webview viewport can be a few pixels
 * shorter on some platforms, so a page that only just fits here is not proven to fit everywhere — the
 * value of the check is catching content that overflows by rows, which is how these bugs actually show
 * up, not by a hairline.
 */
export const WINDOW_SIZES = {
  settings: { width: 500, height: 600 },
  onboarding: { width: 520, height: 560 },
  renewal: { width: 420, height: 260 },
  "update-prompt": { width: 420, height: 280 },
  // The auth popup's label is per-request (`auth-<id>`), so the drift guard keys it by its url.
  authorize: { width: 400, height: 300 },
} as const;
