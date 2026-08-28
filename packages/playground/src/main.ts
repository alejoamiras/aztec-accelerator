import "./style.css";
import {
  AcceleratorStatusController,
  acceleratorStatusView,
  watchLoopbackPermissionChanges,
} from "./accelerator-status";
import { AsciiController } from "./ascii-animation";
import {
  AZTEC_DISPLAY_URL,
  AZTEC_SDK_VERSION,
  checkAcceleratorStatus,
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
import { sameMajor } from "./version";

let deploying = false;

const acceleratorStatus = new AcceleratorStatusController({
  check: checkAcceleratorStatus,
  render: (status) => {
    const view = acceleratorStatusView(status);
    setStatus("accelerator-status", view.connected);
    $("accelerator-label").textContent = view.label;
    $("accelerator-cta").classList.toggle("hidden", !view.showInstall);
    $("accelerator-permission-help").classList.toggle("hidden", !view.showPermissionHelp);

    const showInstallBanner = view.showInstall && !localStorage.getItem("accel-banner-dismissed");
    $("accel-banner").classList.toggle("hidden", !showInstallBanner);
    appendLog(view.log, view.logLevel);
  },
  setPending: (pending) => {
    const retry = $btn("accelerator-retry");
    retry.disabled = pending;
    retry.textContent = pending ? "Checking…" : "Retry";
  },
});

// ── Clock ──
startClock();

// ── Service checks ──

async function checkServices(): Promise<void> {
  await acceleratorStatus.refresh();
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
    const active = key === mode;
    btn.className = active ? ACTIVE_BTN : INACTIVE_BTN;
    btn.dataset.active = String(active);
  }
}

$("mode-local").addEventListener("click", () => {
  if (deploying) return;
  setUiMode("local");
  updateModeUI("local");
  appendLog("Proving mode → WASM");
});

$("mode-accelerated").addEventListener("click", () => {
  if (deploying) return;
  setUiMode("accelerated");
  updateModeUI("accelerated");
  appendLog("Proving mode → Accelerated");
});

// ── Shared helpers ──

/** Handle a prover phase: feed the animation and react to fallback. */
function handleProverPhase(ascii: AsciiController, phase: string, _data?: unknown): void {
  ascii.pushPhase(phase as Parameters<typeof ascii.pushPhase>[0]);
  if (phase === "fallback") {
    appendLog("Native proving fell back to WASM (this will be slower)", "warn");
    // The proof path never awaits this. The controller starts a single forced refresh only if its
    // last rendered state was available, so WASM fallback remains immediate and failure-proof.
    if (state.uiMode === "accelerated") acceleratorStatus.refreshAfterFallback();
  }
}

function setActionButtonsDisabled(disabled: boolean): void {
  $btn("deploy-btn").disabled = disabled;
  // The token flow needs a session-deployed sender (see pickSessionSender) — an enabled
  // button must imply the action can succeed, so it stays disabled until one exists.
  $btn("token-flow-btn").disabled = disabled || state.sessionAddresses.length === 0;
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
    appendLog(`Deploy failed: ${err instanceof Error ? err.message : String(err)}`, "error");
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
    appendLog(`Token flow failed: ${err instanceof Error ? err.message : String(err)}`, "error");
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
      appendLog("Ready. Deploy a test account to get started", "success");
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

  // Install this before the first health request can open the LNA prompt. The health probe stays
  // bounded; a later Allow/Block decision owns a fresh, cache-bypassing status refresh instead.
  await watchLoopbackPermissionChanges(() => {
    void acceleratorStatus.refreshAfterPermissionChange().catch(() => {
      appendLog("Accelerator status refresh failed; WASM fallback remains available", "error");
    });
  });

  $("aztec-url").textContent = AZTEC_DISPLAY_URL;

  // Wire diagnostics export
  $("export-diagnostics-btn").addEventListener("click", downloadDiagnostics);

  // Wire accelerator banner dismiss
  $("accel-banner-dismiss").addEventListener("click", () => {
    $("accel-banner").classList.add("hidden");
    localStorage.setItem("accel-banner-dismissed", "1");
  });

  $btn("accelerator-retry").addEventListener("click", () => {
    void acceleratorStatus.refresh({ forceRefresh: true }).catch(() => {
      appendLog("Accelerator status refresh failed; WASM fallback remains available", "error");
    });
  });

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
      if (sameMajor(AZTEC_SDK_VERSION, nodeVersion) === false) {
        appendLog(`Version mismatch: SDK ${AZTEC_SDK_VERSION} ≠ node ${nodeVersion}`, "warn");
        sdkEl.classList.add("text-amber-500/80");
        nodeEl.classList.add("text-amber-500/80");
      } else if (sameMajor(AZTEC_SDK_VERSION, nodeVersion) && nodeVersion !== AZTEC_SDK_VERSION) {
        appendLog(`SDK ${AZTEC_SDK_VERSION} / node ${nodeVersion}: same major, compatible`);
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
