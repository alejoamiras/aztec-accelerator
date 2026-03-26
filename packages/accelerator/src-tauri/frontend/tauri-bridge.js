/**
 * Shared utilities for Tauri IPC in accelerator frontend pages.
 * Provides consistent error handling and loading states.
 *
 * Loaded via <script src="tauri-bridge.js"> in each HTML page.
 * Functions are used globally — biome can't see cross-file usage.
 */

const { invoke } = window.__TAURI__.core;

/**
 * Show a brief error hint near a control. Disappears after 3 seconds.
 * @param {HTMLElement} anchor — element to show the error near
 * @param {string} message
 */
function showErrorHint(anchor, message) {
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
// biome-ignore lint/correctness/noUnusedVariables: used by HTML pages
function wireToggle(id, handler) {
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
 * @param {() => Promise<void>} opts.onClick — async handler
 */
// biome-ignore lint/correctness/noUnusedVariables: used by HTML pages
function wireButton(id, opts) {
  const btn = document.getElementById(id);
  btn.addEventListener("click", async () => {
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
