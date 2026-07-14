import { invoke, wireButton } from "./bridge.js";

const params = new URLSearchParams(window.location.search);
const origin = params.get("origin") || "unknown";
// SEC-06: the opaque request id the server issued for this popup. Echoed back to respond_auth so
// the decision resolves by id (not origin) — a stale/forged popup can't resolve another request.
const requestId = params.get("requestId") || "";
document.getElementById("origin").textContent = origin;

// Fetch recognition info; render badge if the origin is on the curated list.
// Safe on error: an IPC failure falls through to the unrecognized rendering.
invoke("get_verified_info", { origin })
  .then((info) => {
    if (!info) return;
    const recognized = document.getElementById("recognized");
    recognized.querySelector(".recognized-name").textContent = info.display_name;
    recognized.hidden = false;
  })
  .catch(() => {});

function respond(allowed) {
  const remember = document.getElementById("remember").checked;
  return invoke("respond_auth", { requestId, origin, allowed, remember });
}

wireButton("allow", {
  disableAlso: "deny",
  onClick: () => respond(true),
});

wireButton("deny", {
  disableAlso: "allow",
  onClick: () => respond(false),
});
