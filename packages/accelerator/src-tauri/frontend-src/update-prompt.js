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
    return invoke("respond_update_prompt", { action: "update", autoUpdate });
  },
});

wireButton("later", {
  disableAlso: "update",
  onClick: () => invoke("respond_update_prompt", { action: "later", autoUpdate: false }),
});
