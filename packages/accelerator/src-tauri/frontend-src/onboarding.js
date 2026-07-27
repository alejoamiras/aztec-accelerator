import { invoke, wireButton } from "./bridge.js";

// Per-OS copy for the HTTPS certificate step (the wizard's one consequential action).
const HTTPS_WARN = {
  macos: "⚠ Installs a local certificate — macOS will ask for your password once.",
  windows: "⚠ Installs a local certificate into your browsers when you click Start.",
  linux: "⚠ Installs a certificate into your browsers — no separate prompt will appear.",
};

async function load() {
  try {
    const state = await invoke("get_onboarding_state");
    // All three toggles default to the recommended YES (the HTML `checked` attributes) — HTTPS is
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
  // CSSOM property write (element.style) — NOT a CSP-governed inline style attribute.
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
  showResult(
    "autostart-result",
    asOk(res.autostart),
    asOk(res.autostart) ? "On" : `Failed: ${res.autostart.Err}`,
  );
  showResult(
    "auto-update-result",
    asOk(res.auto_update),
    asOk(res.auto_update) ? "On" : `Failed: ${res.auto_update.Err}`,
  );

  if (res.completed) {
    // F-012: Rust closes the wizard window on success (the page has no core:window grant) — leave the
    // buttons disabled and simply wait for the close.
    return;
  }
  // Partial failure (HTTPS is the only step that can fail here): let the user Retry or continue.
  // wireButton only re-enables buttons when onClick THROWS; complete_onboarding resolves normally with
  // {completed:false}, so re-enable Start + Skip explicitly (else the user is stuck).
  if (https && !asOk(res.https)) {
    document.getElementById("opt-https").checked = false;
    document.getElementById("https-retry").style.display = "inline-block";
    document.getElementById("skip").textContent = "Continue without HTTPS";
  }
  const start = document.getElementById("start");
  start.disabled = false;
  start.textContent = "Start";
  document.getElementById("skip").disabled = false;
}

wireButton("start", { disableAlso: "skip", loadingText: "Setting up…", onClick: runStart });
wireButton("skip", {
  onClick: async () => {
    // Rust sets the marker AND closes the window (F-012 — no core:window grant here).
    await invoke("dismiss_onboarding");
  },
});
document.getElementById("https-retry").addEventListener("click", () => {
  document.getElementById("opt-https").checked = true;
  document.getElementById("https-retry").style.display = "none";
  document.getElementById("start").disabled = false;
  document.getElementById("skip").disabled = false;
});

load();
