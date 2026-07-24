/**
 * Shared utilities for Tauri IPC in the accelerator frontend pages.
 *
 * F-012: this is an ESM module bundled (per page) into `frontend/assets/*.js` by
 * `scripts/build-frontend.ts`. `invoke` comes from the official `@tauri-apps/api/core`
 * package — NOT `window.__TAURI__` — so the app runs with `withGlobalTauri: false`
 * (the global back-door is removed; only `window.__TAURI_INTERNALS__` remains, which
 * the API's `invoke` delegates to). Loaded via a single `<script type="module">` per page.
 */

import { invoke } from "@tauri-apps/api/core";

export { invoke };

// C9 (A / D9): click-steal guard. A button wired with `guard: true` ignores activation for GUARD_MS after
// the window last gained focus — reset on EVERY native focus/show, not just first paint — so a popup
// popped under the cursor (or promoted into the active slot) can't catch a click meant for another window.
// Gating at click ENTRY also covers keyboard Enter/Space (which dispatch a click event).
const DEFAULT_GUARD_MS = 700;
function guardMs() {
  // Overridable ONLY for tests (Playwright mock sets it to 0). Production never sets this global, so the
  // real 700 ms guard always applies. Read dynamically so an init-script override takes effect.
  return typeof window !== "undefined" && typeof window.__CLICK_GUARD_MS__ === "number"
    ? window.__CLICK_GUARD_MS__
    : DEFAULT_GUARD_MS;
}
function now() {
  return typeof performance !== "undefined" ? performance.now() : Date.now();
}
let inputArmedAt = now();
function rearmInputGuard() {
  inputArmedAt = now();
}
if (typeof window !== "undefined") {
  window.addEventListener("focus", rearmInputGuard);
  window.addEventListener("pageshow", rearmInputGuard);
}
function inputGuardActive() {
  return now() - inputArmedAt < guardMs();
}

/**
 * Show a brief error hint near a control. Disappears after 3 seconds.
 * @param {HTMLElement} anchor — element to show the error near
 * @param {string} message
 */
export function showErrorHint(anchor, message) {
  // Remove any existing hint on this anchor
  const existing = anchor.parentElement?.querySelector(".error-hint");
  if (existing) existing.remove();

  const hint = document.createElement("span");
  hint.className = "error-hint";
  hint.textContent = message;
  anchor.closest(".row, .speed-section, .popup-container")?.appendChild(hint);
  setTimeout(() => hint.remove(), 3000);
}

/**
 * Wire a checkbox toggle to a Tauri command.
 * Disables during operation, reverts on error with visible feedback.
 *
 * @param {string} id — element ID of the checkbox input
 * @param {(checked: boolean) => {cmd: string, args?: object}} handler
 *   Function that returns the command name and args based on checked state.
 */
export function wireToggle(id, handler) {
  document.getElementById(id).addEventListener("change", (e) => {
    const el = e.target;
    el.disabled = true;
    const { cmd, args } = handler(el.checked);
    invoke(cmd, args)
      .catch((err) => {
        el.checked = !el.checked;
        console.error(`Failed to invoke ${cmd}:`, err);
        showErrorHint(el, "Failed — try again");
      })
      .finally(() => {
        el.disabled = false;
      });
  });
}

/**
 * Wire a button to a Tauri command.
 * Disables the button (and an optional second button) during operation.
 *
 * @param {string} id — element ID of the button
 * @param {object} opts
 * @param {string} [opts.disableAlso] — ID of another button to disable during operation
 * @param {string} [opts.loadingText] — text to show while loading (restores original on error)
 * @param {boolean} [opts.guard] — apply the click-steal guard (ignore activation within GUARD_MS of focus)
 * @param {() => Promise<void>} opts.onClick — async handler
 */
export function wireButton(id, opts) {
  const btn = document.getElementById(id);
  btn.addEventListener("click", async () => {
    // C9 (A): ignore a click that lands within the guard window after focus (click-steal defense).
    if (opts.guard && inputGuardActive()) return;
    btn.disabled = true;
    const originalText = btn.textContent;
    if (opts.loadingText) btn.textContent = opts.loadingText;

    const otherBtn = opts.disableAlso ? document.getElementById(opts.disableAlso) : null;
    if (otherBtn) otherBtn.disabled = true;

    try {
      await opts.onClick();
    } catch (err) {
      console.error(`Button ${id} action failed:`, err);
      btn.textContent = originalText;
      btn.disabled = false;
      if (otherBtn) otherBtn.disabled = false;
      showErrorHint(btn, "Failed — try again");
    }
  });
}
