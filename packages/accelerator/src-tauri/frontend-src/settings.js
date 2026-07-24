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
  const autostartEl = document.getElementById("autostart");
  try {
    autostartEl.checked = await invoke("get_autostart_enabled");
    autostartEl.disabled = false; // known → actionable
  } catch (e) {
    console.error("Failed to read autostart state:", e);
    showErrorHint(autostartEl, "Autostart state unavailable — reopen Settings to retry");
  }
  document.getElementById("auto-update").checked = config.auto_update === true;

  if (sysInfo.platform === "macos") {
    // F-012: was `.style.display = ""`; the row is `hidden` in markup + `.row[hidden]` in CSS.
    document.getElementById("safari-row").hidden = false;
    document.getElementById("safari").checked = config.safari_support;
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

    const span = document.createElement("span");
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

// Toggles — all use wireToggle from the shared bridge.
wireToggle("autostart", (checked) => ({ cmd: "set_autostart", args: { enabled: checked } }));
wireToggle("auto-update", (checked) => ({ cmd: "set_auto_update", args: { enabled: checked } }));
wireToggle("safari", (checked) => ({
  cmd: checked ? "enable_safari_support" : "disable_safari_support",
}));

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
