import { invoke, rearmClickGuard, showErrorHint, wireButton } from "./bridge.js";

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
let decided = false; // latched ONLY after respond_auth SUCCEEDS — then the window is closing
let responding = false; // a respond_auth call is in flight — don't let the poll fight the button state
let poll = null; // active setTimeout id (self-scheduling; never overlaps)
let wasActive = false; // B2 (F7): prior `info.active`, so we re-arm the guard exactly on the false→true edge

// Never render the query-param origin as authoritative: show a placeholder + disabled controls until the
// server answers.
originEl.textContent = "…";
setControlsEnabled(false);

function setControlsEnabled(on) {
  // Don't fight the button state while a decision is in flight or already made.
  if (decided || responding) return;
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
  if (decided || responding) return;
  let info;
  try {
    info = await invoke("get_pending_auth", { requestId });
  } catch {
    // A2: transient IPC error — keep controls disabled and let the user retry/close.
    setControlsEnabled(false);
    showErrorHint(allowBtn, "Couldn't reach the accelerator — retrying…");
    return;
  }
  if (decided || responding) return; // state changed while awaiting — drop this (possibly stale) result
  if (!info) {
    // A2: None ⇒ the request is already resolved/expired (this popup is stale) ⇒ close it.
    stopPolling();
    window.close();
    return;
  }
  serverOrigin = info.origin;
  originEl.textContent = info.origin;
  renderVerifiedBadge(info.origin);
  // B2 (F7): re-arm the click-steal guard on the queued→active EDGE — the instant this popup's controls
  // become actionable — not merely on a native focus event (which may have fired a poll-interval ago, or
  // not at all when set_focus() doesn't dispatch a JS focus event on this platform). Without this, a queued
  // popup promoted under the cursor (or under a click already travelling toward where Allow lights up) could
  // be actioned with no guard window. Arm BEFORE enabling the controls so the window is open the moment they
  // are clickable. The latch makes this the false→true transition only, so a steady `active:true` across
  // subsequent 1 s polls does not perpetually push the arm-time forward and defeat the guard.
  if (info.active && !wasActive) {
    rearmClickGuard();
  }
  wasActive = info.active;
  // Only the ACTIVE popup is actionable (the server enforces this too via resolve_active; this merely
  // reflects it so a queued popup's controls are visibly disabled until it is promoted).
  setControlsEnabled(info.active);
}

function stopPolling() {
  if (poll !== null) {
    clearTimeout(poll);
    poll = null;
  }
}

// C9 (audit fix): self-scheduling poll (setTimeout, not setInterval) so calls NEVER overlap — a delayed
// older `active:false` can no longer overwrite a newer `active:true`. Re-polls so a QUEUED popup enables
// when promoted (and closes if the request went away). Stops on decision/close.
async function pollLoop() {
  await refreshPending();
  if (decided || poll === null) return;
  poll = setTimeout(pollLoop, 1000);
}

poll = setTimeout(pollLoop, 0);
window.addEventListener("beforeunload", stopPolling);

async function respond(allowed) {
  // C9 (audit fix): do NOT latch `decided`/stop polling until respond_auth SUCCEEDS. On failure the poll
  // must keep running so a resolved/expired request still closes and a promoted one stays truthful, rather
  // than the popup being left falsely actionable with a stale "try again".
  responding = true;
  try {
    // Send the server-authoritative origin (respond_auth treats it as diagnostics-only, but keep it honest).
    // No `remember`: Allow is unconditionally persistent now, so there is no per-decision flag for the
    // renderer to set. The popup discloses that permanence instead (see #permanence in authorize.html).
    await invoke("respond_auth", { requestId, origin: serverOrigin ?? "", allowed });
    decided = true; // success — the Rust side is closing this window
    stopPolling();
  } finally {
    responding = false;
  }
}

wireButton("allow", { disableAlso: "deny", guard: true, onClick: () => respond(true) });
wireButton("deny", { disableAlso: "allow", guard: true, onClick: () => respond(false) });
