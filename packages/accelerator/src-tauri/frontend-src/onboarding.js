import { invoke, wireButton } from "./bridge.js";

// Per-OS copy for the HTTPS certificate step (the wizard's one consequential action).
const HTTPS_WARN = {
  macos: "⚠ Installs a local certificate. macOS will ask for your password once.",
  windows: "⚠ Installs a local certificate into your browsers when you click Start.",
  linux: "⚠ Installs a certificate into your browsers. No separate prompt will appear.",
};

/// The three choice toggles, in render order.
const OPTION_IDS = ["opt-https", "opt-autostart", "opt-auto-update"];

/**
 * The primary button is the ONLY way out of the wizard (there is deliberately no Skip — every outcome
 * it offered is reachable by unchecking toggles, more deliberately, and a second route to "no" next to
 * the primary action invites reflexive dismissal). So its label has to follow the toggles: an
 * eager "Let's go" reads wrong when the user has turned everything off and just wants out.
 */
function syncPrimaryLabel() {
  const anyChecked = OPTION_IDS.some((id) => document.getElementById(id).checked);
  document.getElementById("start").textContent = anyChecked ? "Let's go" : "Continue";
}

async function load() {
  try {
    const state = await invoke("get_onboarding_state");
    // All three toggles default to the recommended YES (the HTML `checked` attributes): HTTPS is
    // pre-checked for everyone, and Start-on-Login + Auto-Update are the recommended defaults. We only
    // set the per-OS certificate copy here; the user can uncheck anything before Start.
    document.getElementById("https-warn").textContent =
      HTTPS_WARN[state.platform] || HTTPS_WARN.linux;
  } catch (err) {
    console.error("get_onboarding_state failed:", err);
  }
}

function showResult(id, ok, msg) {
  const el = document.getElementById(id);
  el.textContent = msg;
  el.className = `result ${ok ? "ok" : "err"}`;
  // CSSOM property write (element.style), NOT a CSP-governed inline style attribute.
  el.style.display = "block";
}

async function runStart() {
  const https = document.getElementById("opt-https").checked;
  const autostart = document.getElementById("opt-autostart").checked;
  const autoUpdate = document.getElementById("opt-auto-update").checked;

  const res = await invoke("complete_onboarding", { https, autostart, autoUpdate });

  // Render per-row outcomes. Result<(),String> serializes as {Ok:null} | {Err:"..."}.
  const asOk = (r) => r && "Ok" in r;
  if (https)
    showResult(
      "https-result",
      asOk(res.https),
      asOk(res.https) ? "Encrypted connection enabled" : `Failed: ${res.https.Err}`,
    );
  // Report the state the user CHOSE, not a fixed "On". `complete_onboarding` returns Ok for "set it
  // off" just as much as for "set it on", so declining Start-on-Login used to be confirmed back as
  // "On" — the wizard reporting the opposite of what it just did (post-impl codex Low).
  const applied = (chosen, r) => (asOk(r) ? (chosen ? "On" : "Off") : `Failed: ${r.Err}`);
  showResult("autostart-result", asOk(res.autostart), applied(autostart, res.autostart));
  showResult("auto-update-result", asOk(res.auto_update), applied(autoUpdate, res.auto_update));

  if (res.completed) {
    // F-012: Rust closes the wizard window on success (the page has no core:window grant), so leave
    // the button disabled and simply wait for the close.
    return;
  }
  // Partial failure (HTTPS is the only step that can fail here). HTTPS is unchecked for the user, so
  // pressing the button again persists "HTTPS off", sets the marker and closes — the same exit the old
  // "Continue without HTTPS" button provided, without a second button to reason about. Retry re-checks
  // it for another attempt. wireButton only re-enables when onClick THROWS, and complete_onboarding
  // resolves normally with {completed:false}, so re-enable explicitly (else the user is stuck).
  if (https && !asOk(res.https)) {
    document.getElementById("opt-https").checked = false;
    document.getElementById("https-retry").style.display = "inline-block";
  }
  const start = document.getElementById("start");
  start.disabled = false;
  syncPrimaryLabel();
}

wireButton("start", { loadingText: "Setting up…", onClick: runStart });

document.getElementById("https-retry").addEventListener("click", () => {
  document.getElementById("opt-https").checked = true;
  document.getElementById("https-retry").style.display = "none";
  document.getElementById("start").disabled = false;
  syncPrimaryLabel();
});

for (const id of OPTION_IDS) {
  document.getElementById(id).addEventListener("change", syncPrimaryLabel);
}

syncPrimaryLabel();
load();
