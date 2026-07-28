import { invoke, showErrorHint, wireToggle } from "./bridge.js";

// CPU count comes from the Rust backend (std::thread::available_parallelism),
// NOT navigator.hardwareConcurrency which WebKit caps for fingerprinting protection.
let CPUS = 1;

function threadsForLevel(index) {
  const fractions = [1 / 4, 3 / 8, 1 / 2, 3 / 4, 1];
  return Math.max(1, Math.floor(CPUS * fractions[index]));
}

const SPEED_LEVELS = [
  { value: "low", label: "Low" },
  { value: "light", label: "Light" },
  { value: "balanced", label: "Balanced" },
  { value: "high", label: "High" },
  { value: "full", label: "Full" },
];

function descForLevel(index) {
  const t = threadsForLevel(index);
  if (index === 4) return `All ${CPUS} cores. Fastest proving.`;
  return `${t} of ${CPUS} cores. ${
    index <= 1 ? "Keeps the system responsive." : "Good balance of speed and usability."
  }`;
}

function speedToIndex(speed) {
  const idx = SPEED_LEVELS.findIndex((l) => l.value === speed);
  return idx >= 0 ? idx : 4;
}

function updateSpeedUI(index) {
  const level = SPEED_LEVELS[index];
  document.getElementById("speed-label").textContent = level.label;
  document.getElementById("speed-desc").textContent = descForLevel(index);
  // F-012: the slider fill was `.style.setProperty("--fill", …)` (a CSSOM mutation — not
  // CSP-governed, but externalized for consistency). `[data-fill="N"]` maps to `--fill:N%`
  // in style.css (0-4 → 0/25/50/75/100%), so no inline style is written.
  document.getElementById("speed").dataset.fill = String(index);
}

async function loadSettings() {
  const [config, sysInfo] = await Promise.all([invoke("get_config"), invoke("get_system_info")]);

  CPUS = sysInfo.cpu_count;

  // codex r2 #6 / r3 #6: the autostart switch ships DISABLED (settings.html) and stays disabled until
  // its true state is CONFIRMED — so an unknown state is never presented as an actionable "off", and a
  // failure of ANY preceding request (which would throw before this block) still leaves it disabled
  // rather than at a false default. Fetched independently so a read error doesn't fail the whole panel.
  await loadAutostart();
  document.getElementById("auto-update").checked = config.auto_update === true;

  // The Encrypted Connection section (toggle + the disclosed certificate action) shows on all three
  // desktop OSes — each now has a trust backend (macOS Keychain / Linux user NSS / Windows
  // CurrentUser Root). F-012: hidden via the `hidden` attribute, never an inline style.
  if (["macos", "linux", "windows"].includes(sysInfo.platform)) {
    document.getElementById("https-section").hidden = false;
    document.getElementById("https").checked = config.https_enabled;
  }

  const idx = speedToIndex(config.speed || "full");
  document.getElementById("speed").value = idx;
  updateSpeedUI(idx);

  renderOrigins(config.approved_origins || []);
}

function renderOrigins(origins) {
  const list = document.getElementById("origins");
  const empty = document.getElementById("origins-empty");
  list.innerHTML = "";

  // F-012: was `.style.display`; `[hidden]` on `.empty-state` resolves via the UA stylesheet.
  empty.hidden = origins.length !== 0;
  if (origins.length === 0) return;

  for (const origin of origins) {
    const li = document.createElement("li");
    li.className = "origin-item";

    // Removing "allow once" made Allow permanent, which promotes THIS list to a load-bearing
    // security control: it is now the only way to end a grant. It therefore needs the same F-014
    // treatment the popup's origin line already had and this list never did — bidi isolation so
    // RTL/format characters cannot visually reorder a stored origin into a look-alike, `dir=ltr`,
    // and selectable text so the user can copy it out to inspect it. Two visually-confusable rows
    // here means removing the wrong one.
    const span = document.createElement("span");
    span.className = "origin-text";
    span.dir = "ltr";
    span.textContent = origin;

    const btn = document.createElement("button");
    btn.textContent = "Remove";
    // btn.onclick is a DOM property assignment from this (allowed, external) module —
    // NOT a CSP inline-string handler, so `script-src 'self'` permits it.
    btn.onclick = async () => {
      await invoke("remove_approved_origin", { origin });
      await loadSettings();
    };

    li.appendChild(span);
    li.appendChild(btn);
    list.appendChild(li);
  }
}

// ── Autostart: intent switch + health row (plan D3/D13/D17) ──
//
// get_autostart_enabled returns a STRUCTURED status, not a bool: the switch reflects the user's
// INTENT (entry present, not platform-disabled); the health row underneath says when the entry
// won't actually launch, and carries the action that fixes it. Turning it OFF always works — an
// earlier draft rendered a broken entry as an unchecked switch, which took away the user's only
// OFF control (plan D17).

const autostartEl = document.getElementById("autostart");

function renderAutostartStatus(st) {
  autostartEl.checked = st.intentEnabled;
  autostartEl.disabled = false; // known → actionable
  const row = document.getElementById("autostart-health");
  const text = document.getElementById("autostart-health-text");
  const fix = document.getElementById("autostart-fix");
  let message = "";
  let showFix = false;
  if (st.intentEnabled && !st.healthy) {
    if (st.canRepairNow) {
      message = "Start on Login points to a file that no longer exists.";
      showFix = true;
    } else {
      // D14: our own path doesn't resolve either (the app was moved while running) — a Fix here
      // would silently no-op, so say the one thing that will actually work instead.
      message = "Start on Login needs repair. Reopen Aztec Accelerator from its new location.";
    }
  } else if (st.intentEnabled && st.pointsElsewhere) {
    // Healthy entry for a DIFFERENT copy — never silently stolen (plan D1/D10); off→on recreates.
    message =
      "Start on Login points to another copy of Aztec Accelerator. Turn it off and on to use this one.";
  }
  // F-B2: textContent only — st fields are backend-controlled, but the rule is structural: nothing
  // reaching this row is ever interpreted as HTML. (storedPath is already redacted in Rust — D24 —
  // and deliberately not rendered at all.)
  text.textContent = message;
  row.hidden = message === "";
  fix.hidden = !showFix;
}

async function loadAutostart() {
  try {
    renderAutostartStatus(await invoke("get_autostart_enabled"));
  } catch (e) {
    // Unreadable artifact → the backend Errs (codex #7) → switch stays DISABLED (unknown state is
    // never an actionable "off") and the health row stays out of the way.
    console.error("Failed to read autostart state:", e);
    document.getElementById("autostart-health").hidden = true;
    showErrorHint(autostartEl, "Autostart state unavailable — reopen Settings to retry");
  }
}

// Not wireToggle: after a successful set_autostart the HEALTH may have changed too (off→on from
// points_elsewhere recreates the entry), so re-render from the authoritative status instead of
// trusting the optimistic flip. Failure semantics match wireToggle exactly (revert + hint).
autostartEl.addEventListener("change", async (e) => {
  const el = e.target;
  el.disabled = true;
  try {
    await invoke("set_autostart", { enabled: el.checked });
    await loadAutostart();
  } catch (err) {
    el.checked = !el.checked;
    console.error("Failed to invoke set_autostart:", err);
    showErrorHint(el, typeof err === "string" ? err : "Failed — try again");
    el.disabled = false;
  }
});

document.getElementById("autostart-fix").addEventListener("click", async (e) => {
  const btn = e.target;
  btn.disabled = true;
  try {
    // repair_autostart returns the fresh status — render from truth, not assumption.
    renderAutostartStatus(await invoke("repair_autostart"));
  } catch (err) {
    console.error("Autostart repair failed:", err);
    showErrorHint(btn, typeof err === "string" ? err : "Repair failed — try again");
  } finally {
    btn.disabled = false;
  }
});

wireToggle("auto-update", (checked) => ({ cmd: "set_auto_update", args: { enabled: checked } }));

// HTTPS does NOT use wireToggle's optimistic-flip-then-revert-on-error. That pattern assumes a
// command either fully applies or fully doesn't, which is true for autostart/auto-update and FALSE
// here: `enable_https` persists `https_enabled = true` and STILL returns Err when the running
// listener is serving a superseded certificate (the restart-required path). Reverting the checkbox
// there showed "off" while the config — and therefore the next launch — said on, and re-toggling
// just reproduced the same error with no account of the real state (post-impl codex Medium).
// Both HTTPS controls instead re-read the persisted config after every attempt, so the panel always
// shows what the backend actually stored, and they disable EACH OTHER while one is in flight: the
// Rust lifecycle mutex already serializes them correctly, but an un-disabled second control let the
// user queue an operation against state they could no longer see.
const httpsEls = () => [document.getElementById("https"), document.getElementById("remove-trust")];

async function runHttpsOp(anchor, op) {
  const controls = httpsEls();
  for (const el of controls) el.disabled = true;
  try {
    await op();
  } catch (err) {
    console.error("HTTPS operation failed:", err);
    // Surface the backend's own message: "restart to finish enabling" and "a proof is running" are
    // both actionable, and a generic "Failed — try again" would throw that away.
    showErrorHint(anchor, typeof err === "string" ? err : "Failed — try again");
  } finally {
    // Authoritative refresh — never infer the resulting state from whether the call threw.
    try {
      const config = await invoke("get_config");
      document.getElementById("https").checked = config.https_enabled === true;
    } catch (e) {
      console.error("Failed to re-read config after an HTTPS operation:", e);
    }
    for (const el of controls) el.disabled = false;
  }
}

document.getElementById("https").addEventListener("change", (e) => {
  const wanted = e.target.checked;
  runHttpsOp(e.target, () => invoke(wanted ? "enable_https" : "disable_https"));
});

// Removing the certificate (D5) — takes the local CA out of every browser store and turns HTTPS off.
// Lives behind the "Manage certificate" disclosure: it is the documented recovery path for a
// partially-trusted install, not an everyday control.
document.getElementById("remove-trust").addEventListener("click", (e) => {
  runHttpsOp(e.target, () => invoke("remove_https_trust"));
});

// Speed slider
const speedSlider = document.getElementById("speed");
speedSlider.addEventListener("input", (e) => {
  updateSpeedUI(Number(e.target.value));
});
speedSlider.addEventListener("change", (e) => {
  const level = SPEED_LEVELS[Number(e.target.value)];
  invoke("set_speed", { speed: level.value }).catch((err) => {
    console.error("Failed to set speed:", err);
    showErrorHint(speedSlider, "Failed to save");
    loadSettings();
  });
});

loadSettings().catch((err) => console.error("Failed to load settings:", err));
