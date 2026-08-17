import { invoke, wireButton } from "./bridge.js";

const params = new URLSearchParams(window.location.search);
const currentVersion = params.get("current") || "unknown";
const newVersion = params.get("version") || "unknown";
document.getElementById("version").textContent = `v${currentVersion}  →  v${newVersion}`;

wireButton("update", {
  disableAlso: "later",
  loadingText: "Updating…",
  onClick: () => {
    const autoUpdate = document.getElementById("auto-update").checked;
    // B2 (F8): echo the version this prompt is DISPLAYING so the backend installs only that exact
    // version — a background re-check that swapped the pending update is refused, not silently installed.
    return invoke("respond_update_prompt", {
      action: "update",
      autoUpdate,
      displayedVersion: newVersion,
    });
  },
});

wireButton("later", {
  disableAlso: "update",
  onClick: () =>
    invoke("respond_update_prompt", {
      action: "later",
      autoUpdate: false,
      displayedVersion: newVersion,
    }),
});
