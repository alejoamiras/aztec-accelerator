import type { AnimationPhase } from "./ascii-animation";
import type { StepTiming } from "./aztec";
import { $, formatDuration } from "./ui";

export function formatMs(ms: number): string {
  return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${Math.round(ms)}ms`;
}

/** Shorten "ContractName:function_name" → "function_name" */
export function shortFnName(name: string): string {
  if (!name) return "unknown";
  const i = name.lastIndexOf(":");
  return i >= 0 && i < name.length - 1 ? name.slice(i + 1) : name;
}

/** Build a "label ··· value" row using safe DOM APIs (no innerHTML). */
export function buildDotRow(
  className: string,
  label: string,
  labelClass: string,
  value: string,
  valueClass: string,
): HTMLDivElement {
  const row = document.createElement("div");
  row.className = className;

  const labelSpan = document.createElement("span");
  labelSpan.className = labelClass;
  labelSpan.textContent = label;

  const dots = document.createElement("span");
  dots.className = "step-dots";

  const valueSpan = document.createElement("span");
  valueSpan.className = valueClass;
  valueSpan.textContent = value;

  row.append(labelSpan, dots, valueSpan);
  return row;
}

export function renderSteps(container: HTMLElement, steps: StepTiming[]): void {
  container.replaceChildren();
  const details = document.createElement("details");
  const summary = document.createElement("summary");
  summary.textContent = `${steps.length} steps`;
  details.appendChild(summary);

  const list = document.createElement("div");
  list.className = "mt-1.5 space-y-1.5";

  for (const step of steps) {
    const group = document.createElement("div");

    // Step header row
    group.appendChild(
      buildDotRow(
        "step-row",
        step.step,
        "text-gray-300",
        formatMs(step.durationMs),
        "text-emerald-500/80 tabular-nums",
      ),
    );

    // Sub-phase details (simulation + prove/send + confirm)
    if (step.simulation || step.proveSendMs != null) {
      const sub = document.createElement("div");
      sub.className = "step-sim";

      // Simulation sub-details
      if (step.simulation) {
        const sim = step.simulation;
        sub.appendChild(
          buildDotRow(
            "step-sim-row",
            "sim",
            "text-gray-600",
            formatMs(sim.totalMs),
            "tabular-nums",
          ),
        );
        sub.appendChild(
          buildDotRow(
            "step-sim-row",
            "sync",
            "text-gray-600",
            formatMs(sim.syncMs),
            "tabular-nums",
          ),
        );
        for (const fn of sim.perFunction) {
          sub.appendChild(
            buildDotRow(
              "step-sim-row",
              shortFnName(fn.name),
              "text-gray-600",
              formatMs(fn.ms),
              "tabular-nums",
            ),
          );
        }
      }

      // Pure proving time (subset of prove + send)
      if (step.proveMs != null) {
        sub.appendChild(
          buildDotRow(
            "step-sim-row",
            "prove",
            "text-gray-600",
            formatMs(step.proveMs),
            "tabular-nums",
          ),
        );
      }

      // Prove + send / confirm sub-rows
      if (step.proveSendMs != null) {
        sub.appendChild(
          buildDotRow(
            "step-sim-row",
            "prove + send",
            "text-gray-600",
            formatMs(step.proveSendMs),
            "tabular-nums",
          ),
        );
      }

      if (step.confirmMs != null) {
        sub.appendChild(
          buildDotRow(
            "step-sim-row",
            "confirm",
            "text-gray-600",
            formatMs(step.confirmMs),
            "tabular-nums",
          ),
        );
      }

      group.appendChild(sub);
    }

    list.appendChild(group);
  }

  details.appendChild(list);
  container.appendChild(details);
  container.classList.remove("hidden");
}

/**
 * Display a result in the appropriate column.
 * @param prefix - Element ID prefix: "" for embedded (uses `time-local` etc.), "ext-" for external (uses `ext-time-wallet` etc.)
 * @param mode - The mode suffix for element IDs (e.g., "local", "uee", "tee", "wallet")
 */
export function showResult(
  prefix: string,
  mode: string,
  durationMs: number,
  tag: string,
  steps?: StepTiming[],
): void {
  $(`${prefix}results`).classList.remove("hidden");

  const timeEl = $(`${prefix}time-${mode}`);
  timeEl.textContent = formatDuration(durationMs);
  timeEl.className = "text-3xl font-bold tabular-nums text-emerald-400";

  const tagEl = $(`${prefix}tag-${mode}`);
  tagEl.textContent = tag;
  tagEl.className = `mt-1.5 text-[10px] uppercase tracking-widest ${
    tag === "token flow"
      ? "text-cyan-500/70"
      : tag === "cold"
        ? "text-amber-500/70"
        : "text-cyan-500/70"
  }`;

  $(`${prefix}result-${mode}`).classList.add("result-filled");

  if (steps?.length) {
    renderSteps($(`${prefix}steps-${mode}`), steps);
  }
}

/** Map onStep step names to app-level animation phases. */
export function stepToPhase(stepName: string): AnimationPhase | null {
  if (stepName.includes("simulat")) return "app:simulate";
  if (stepName.includes("proving") || stepName.includes("deploying")) return "app:prove";
  if (stepName.includes("confirm")) return "app:confirm";
  return null;
}
