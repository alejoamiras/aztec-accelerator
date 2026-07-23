import { invoke, showErrorHint, wireButton } from "./bridge.js";

const params = new URLSearchParams(window.location.search);
// SEC-06: the opaque request id the server issued for this popup — the ONLY value we trust from the URL.
// C9 (D8): the ORIGIN is NOT taken from the query param; it is fetched from the server (get_pending_auth)
// so the popup displays exactly what respond_auth will grant. The query `origin` is display-only legacy.
const requestId = params.get("requestId") || "";

const originEl = document.getElementById("origin");
const allowBtn = document.getElementById("allow");
const denyBtn = document.getElementById("deny");

let serverOrigin = null; // authoritative origin, once get_pending_auth answers
let badgeShownFor = null; // origin we've already rendered the verified badge for
let decided = false; // set once the user acts — stops the poll fighting the button state
let poll = null;

// Never render the query-param origin as authoritative: show a placeholder + disabled buttons until the
// server answers.
originEl.textContent = "…";
setButtonsEnabled(false);

function setButtonsEnabled(on) {
  if (decided) return;
  allowBtn.disabled = !on;
  denyBtn.disabled = !on;
}

function renderVerifiedBadge(origin) {
  if (badgeShownFor === origin) return;
  badgeShownFor = origin;
  // C9 (D8): the verified badge is keyed on the SERVER origin, not the query param.
  invoke("get_verified_info", { origin })
    .then((info) => {
      if (!info) return;
      const recognized = document.getElementById("recognized");
      recognized.querySelector(".recognized-name").textContent = info.display_name;
      recognized.hidden = false;
    })
    .catch(() => {});
}

async function refreshPending() {
  if (decided) return;
  let info;
  try {
    info = await invoke("get_pending_auth", { requestId });
  } catch {
    // A2: transient IPC error — keep Allow/Deny disabled and let the user retry/close.
    setButtonsEnabled(false);
    showErrorHint(allowBtn, "Couldn't reach the accelerator — retrying…");
    return;
  }
  if (!info) {
    // A2: None ⇒ the request is already resolved/expired (this popup is stale) ⇒ close it.
    stopPolling();
    window.close();
    return;
  }
  serverOrigin = info.origin;
  originEl.textContent = info.origin;
  renderVerifiedBadge(info.origin);
  // Only the ACTIVE popup is actionable (the server enforces this too via resolve_active; this merely
  // reflects it so a queued popup's buttons are visibly disabled until it is promoted).
  setButtonsEnabled(info.active);
}

function stopPolling() {
  if (poll !== null) {
    clearInterval(poll);
    poll = null;
  }
}

refreshPending();
// Re-poll so a QUEUED popup enables its buttons when promoted to active (and closes if the request went
// away). Cheap; stops as soon as the user decides or the popup closes.
poll = setInterval(refreshPending, 1000);
window.addEventListener("beforeunload", stopPolling);

function respond(allowed) {
  decided = true;
  stopPolling();
  const remember = document.getElementById("remember").checked;
  // Send the server-authoritative origin (respond_auth treats it as diagnostics-only, but keep it honest).
  return invoke("respond_auth", { requestId, origin: serverOrigin ?? "", allowed, remember });
}

wireButton("allow", { disableAlso: "deny", guard: true, onClick: () => respond(true) });
wireButton("deny", { disableAlso: "allow", guard: true, onClick: () => respond(false) });
