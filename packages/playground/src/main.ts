import "./style.css";
import { AsciiController } from "./ascii-animation";
import {
  AZTEC_DISPLAY_URL,
  AZTEC_SDK_VERSION,
  checkAccelerator,
  checkAztecNode,
  deployTestAccount,
  initializeWallet,
  runTokenFlow,
  setUiMode,
  state,
  type UiMode,
} from "./aztec";
import {
  diagMemory,
  downloadDiagnostics,
  installErrorHandlers,
  installWasmDiagnostics,
  installWorkerDiagnostics,
} from "./diagnostics";
import { showResult, stepToPhase } from "./results";
import { $, $btn, appendLog, formatDuration, setStatus, startClock } from "./ui";

let deploying = false;

// ── Clock ──
startClock();

// ── Service checks ──

async function checkServices(): Promise<void> {
  const accel = await checkAccelerator();
  updateAcceleratorLabel(accel);
  if (accel) {
    appendLog("Native accelerator detected on localhost:59833", "success");
  } else {
    appendLog("Accelerator not detected, will fall back to WASM", "warn");
  }
}

// ── Mode toggle ──
const INACTIVE_BTN = "mode-btn";
const ACTIVE_BTN = "mode-btn mode-active";

function updateModeUI(mode: UiMode): void {
  const buttons: Record<UiMode, HTMLElement> = {
    local: $("mode-local"),
    accelerated: $("mode-accelerated"),
  };

  for (const [key, btn] of Object.entries(buttons)) {
    btn.className = key === mode ? ACTIVE_BTN : INACTIVE_BTN;
  }
}

$("mode-local").addEventListener("click", () => {
  if (deploying) return;
  setUiMode("local");
  updateModeUI("local");
  appendLog("Switched to local proving mode (WASM)");
});

$("mode-accelerated").addEventListener("click", () => {
  if (deploying) return;
  setUiMode("accelerated");
  updateModeUI("accelerated");
  appendLog("Switched to accelerated proving mode");
});

// ── Shared helpers ──

/** Update the accelerator service label and button state. */
function updateAcceleratorLabel(available: boolean): void {
  setStatus("accelerator-status", available);
  $("accelerator-label").textContent = available ? "available" : "not detected, fallback: wasm";
}

/** Handle a prover phase: feed the animation and react to fallback. */
function handleProverPhase(ascii: AsciiController, phase: string, _data?: unknown): void {
  ascii.pushPhase(phase as Parameters<typeof ascii.pushPhase>[0]);
  if (phase === "fallback") {
    updateAcceleratorLabel(false);
    appendLog("Accelerator offline, falling back to WASM (this will be slower)", "warn");
  }
}

function setActionButtonsDisabled(disabled: boolean): void {
  $btn("deploy-btn").disabled = disabled;
  $btn("token-flow-btn").disabled = disabled;
}

// ── Deploy ──
$("deploy-btn").addEventListener("click", async () => {
  if (deploying) return;
  deploying = true;
  setActionButtonsDisabled(true);

  const btn = $btn("deploy-btn");
  btn.textContent = "Proving...";

  $("progress").classList.remove("hidden");

  const ascii = new AsciiController($("ascii-art"), document.getElementById("ascii-elapsed"));
  ascii.start(state.uiMode);

  try {
    diagMemory("deploy-start");
    const result = await deployTestAccount(
      appendLog,
      () => {},
      (stepName) => {
        const phase = stepToPhase(stepName);
        if (phase) ascii.pushPhase(phase);
      },
      (phase, data) => handleProverPhase(ascii, phase, data),
    );
    diagMemory("deploy-end");

    for (const step of result.steps) {
      appendLog(`${step.step} ${formatDuration(step.durationMs)}`);
    }
    appendLog(`total: ${formatDuration(result.totalDurationMs)}`, "success");

    showResult("", result.mode, result.totalDurationMs, undefined, result.steps);
  } catch (err) {
    diagMemory("deploy-error");
    appendLog(`Deploy failed: ${err}`, "error");
  } finally {
    ascii.stop();
    deploying = false;
    setActionButtonsDisabled(false);
    btn.textContent = "Deploy Test Account";
    $("progress").classList.add("hidden");
  }
});

// ── Token Flow ──
$("token-flow-btn").addEventListener("click", async () => {
  if (deploying) return;
  deploying = true;
  setActionButtonsDisabled(true);

  const btn = $btn("token-flow-btn");
  btn.textContent = "Running...";

  $("progress").classList.remove("hidden");

  const ascii = new AsciiController($("ascii-art"), document.getElementById("ascii-elapsed"));
  ascii.start(state.uiMode);

  try {
    diagMemory("token-flow-start");
    const result = await runTokenFlow(
      appendLog,
      () => {},
      (stepName) => {
        const phase = stepToPhase(stepName);
        if (phase) ascii.pushPhase(phase);
      },
      (phase, data) => handleProverPhase(ascii, phase, data),
    );
    diagMemory("token-flow-end");

    for (const step of result.steps) {
      appendLog(`${step.step} ${formatDuration(step.durationMs)}`);
    }
    appendLog(`total: ${formatDuration(result.totalDurationMs)}`, "success");

    showResult("", result.mode, result.totalDurationMs, "token flow", result.steps);
  } catch (err) {
    diagMemory("token-flow-error");
    appendLog(`Token flow failed: ${err}`, "error");
  } finally {
    ascii.stop();
    deploying = false;
    setActionButtonsDisabled(false);
    btn.textContent = "Run Token Flow";
    $("progress").classList.add("hidden");
  }
});

// ── Init ──
async function initWallet(): Promise<void> {
  appendLog("Initializing wallet...");
  $("wallet-state").textContent = "initializing...";
  setStatus("wallet-dot", null);

  const ok = await initializeWallet(appendLog);
  if (ok) {
    $("wallet-state").textContent = "ready";
    $("wallet-state").className = "text-brand-accent/80 ml-auto text-[10px] font-mono font-light";
    setStatus("wallet-dot", true);
    setActionButtonsDisabled(false);

    const networkLabel = $("network-label");
    if (state.proofsRequired) {
      networkLabel.textContent = "proofs enabled";
      networkLabel.className = "text-amber-500/80 text-[10px] uppercase tracking-wider ml-auto";
      appendLog("Ready. Deploy a test account to get started (proofs enabled)", "success");
    } else {
      networkLabel.textContent = "proofs simulated";
      networkLabel.className =
        "text-brand-text-muted/50 text-[10px] uppercase tracking-wider ml-auto";
      appendLog("Ready. Deploy a test account or run the token flow", "success");
    }
  } else {
    $("wallet-state").textContent = "failed";
    $("wallet-state").className = "text-red-400/80 ml-auto text-[10px] font-mono font-light";
    setStatus("wallet-dot", false);
  }
}

async function init(): Promise<void> {
  // Install diagnostics BEFORE any Worker/WASM is created
  installWorkerDiagnostics();
  installWasmDiagnostics();
  installErrorHandlers();

  $("aztec-url").textContent = AZTEC_DISPLAY_URL;

  // Wire diagnostics export
  $("export-diagnostics-btn").addEventListener("click", downloadDiagnostics);

  // Default mode UI
  updateModeUI("accelerated");

  appendLog("Checking Aztec node...");
  const { reachable: aztec, nodeVersion } = await checkAztecNode();
  setStatus("aztec-status", aztec);

  // Show versions row once we have data
  if (AZTEC_SDK_VERSION !== "unknown" || nodeVersion) {
    $("versions-row").classList.remove("hidden");
    const sdkEl = $("version-sdk");
    const nodeEl = $("version-node");
    if (AZTEC_SDK_VERSION !== "unknown") sdkEl.textContent = AZTEC_SDK_VERSION;
    if (nodeVersion) {
      nodeEl.textContent = nodeVersion;
      appendLog(`Aztec node version: ${nodeVersion}`);
      if (nodeVersion !== AZTEC_SDK_VERSION) {
        appendLog(`Version mismatch: SDK ${AZTEC_SDK_VERSION} ≠ node ${nodeVersion}`, "warn");
        sdkEl.classList.add("text-amber-500/80");
        nodeEl.classList.add("text-amber-500/80");
      }
    }
  }

  // Check accelerator
  await checkServices();

  // Show embedded UI and hide fallback placeholder
  $("embedded-ui").classList.remove("hidden");
  document.querySelector(".embedded-ui-fallback")?.classList.add("hidden");

  if (aztec) {
    await initWallet();
  } else {
    appendLog(`Aztec node not reachable at ${AZTEC_DISPLAY_URL}`, "error");
    appendLog("Start the Aztec node before using the demo", "warn");
    $("wallet-state").textContent = "aztec unavailable";
    setStatus("wallet-dot", false);
  }
}

init();
