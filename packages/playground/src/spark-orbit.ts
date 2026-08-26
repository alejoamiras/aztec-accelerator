import type { UiMode } from "./aztec";
import { type AnimationPhase, PhaseQueue } from "./phase-queue";

/**
 * The spark-orbit proving theater: a dashed dial whose spark position IS the run's progress.
 * Each dial quadrant owns a phase bucket (witness → sync → prove → ta-da); within a bucket the
 * spark eases toward the quadrant's end but never crosses it, so a silent stretch (steps that
 * emit no phase) reads as a purposeful hold, not fake progress. The angle only ever grows —
 * a new run adds a lap instead of rewinding. Matches the tray icon's orbit: one proving gesture
 * across the product.
 */

export type DialQuadrant = 0 | 1 | 2 | 3;

export interface DialTarget {
  quadrant: DialQuadrant;
  /** denied / version-mismatch / fallback: the run continues in-browser — a mode, not a failure. */
  fallback?: boolean;
}

export const QUADRANT_LABELS = ["witness", "sync", "prove", "ta-da"] as const;

export function phaseToDial(phase: AnimationPhase): DialTarget {
  switch (phase) {
    case "detect":
    case "downloading":
    case "serialize":
    case "app:simulate":
      return { quadrant: 0 };
    case "transmit":
      return { quadrant: 1 };
    case "proving":
    case "app:prove":
      return { quadrant: 2 };
    case "denied":
    case "version-mismatch":
    case "fallback":
      // Verified SDK flow: these are followed by the local prover + "receive" — keep moving.
      return { quadrant: 2, fallback: true };
    case "proved":
    case "receive":
    case "app:confirm":
      return { quadrant: 3 };
  }
}

const TICK_MS = 100;
/** Within-quadrant easing: p = 1 − e^(−t/τ), capped so the boundary is only crossed by a phase. */
const QUADRANT_TAU_MS = 1400;
const QUADRANT_CAP = 0.96;

const SVG_NS = "http://www.w3.org/2000/svg";

function svgEl(name: string, attrs: Record<string, string>): SVGElement {
  const el = document.createElementNS(SVG_NS, name);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
  return el;
}

export class SparkOrbitController {
  #host: HTMLElement;
  #elapsedEl: HTMLElement | null;
  #queue: PhaseQueue;
  #orb: SVGElement | null = null;
  #bolt: SVGElement | null = null;
  #labels: HTMLElement[] = [];
  #phaseLine: HTMLElement | null = null;
  #reduced: boolean;
  #animTimer: ReturnType<typeof setInterval> | null = null;
  #startTime = 0;
  #lap = 0;
  #quadrant: DialQuadrant = 0;
  #quadrantSince = 0;
  #fallback = false;
  #running = false;

  constructor(
    host: HTMLElement,
    elapsedEl?: HTMLElement | null,
    opts?: { reducedMotion?: boolean },
  ) {
    this.#host = host;
    this.#elapsedEl = elapsedEl ?? null;
    this.#reduced =
      opts?.reducedMotion ??
      (typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches);
    this.#queue = new PhaseQueue((phase) => this.#enter(phaseToDial(phase)));
  }

  start(mode: UiMode): void {
    this.#running = true;
    this.#startTime = Date.now();
    this.#lap = 0;
    this.#quadrant = 0;
    this.#quadrantSince = Date.now();
    this.#fallback = mode === "local";
    this.#build();
    this.#host.classList.remove("hidden");
    this.#applyMode();
    this.#setLabel(0);
    if (!this.#reduced) {
      this.#animTimer = setInterval(() => this.#tick(), TICK_MS);
    }
  }

  pushPhase(phase: AnimationPhase): void {
    this.#queue.push(phase);
  }

  stop(): void {
    if (this.#animTimer) {
      clearInterval(this.#animTimer);
      this.#animTimer = null;
    }
    this.#queue.clear();
    this.#running = false;
    this.#host.classList.add("hidden");
    this.#host.replaceChildren();
    this.#orb = null;
    this.#bolt = null;
    this.#labels = [];
    this.#phaseLine = null;
    if (this.#elapsedEl) this.#elapsedEl.textContent = "";
  }

  /** Current spark angle in degrees — monotonically non-decreasing across a run. */
  angle(): number {
    const held = Date.now() - this.#quadrantSince;
    // Quadrants 0-2 never cross their boundary without a phase (cap); ta-da may complete the
    // lap so the spark lands exactly on top for the pop.
    const cap = this.#quadrant === 3 ? 1 : QUADRANT_CAP;
    const p = this.#reduced ? cap : Math.min(cap, 1 - Math.exp(-held / QUADRANT_TAU_MS));
    return this.#lap * 360 + (this.#quadrant + p) * 90;
  }

  #enter(target: DialTarget): void {
    if (!this.#running) return;
    if (target.fallback && !this.#fallback) {
      this.#fallback = true;
      this.#applyMode();
    }
    if (target.quadrant < this.#quadrant) {
      this.#lap += 1; // a new run — keep the angle growing, never rewind
    }
    // Reset easing only on a quadrant CHANGE (a lap always is one); a repeated same-quadrant
    // phase must not restart p, or the angle would dip backwards.
    if (target.quadrant !== this.#quadrant) {
      this.#quadrantSince = Date.now();
    }
    this.#quadrant = target.quadrant;
    this.#setLabel(target.quadrant);
    if (this.#bolt) {
      this.#bolt.classList.toggle("dial-bolt-pop", target.quadrant === 3);
    }
    if (this.#reduced) this.#render();
  }

  #tick(): void {
    this.#render();
    if (this.#elapsedEl) {
      const elapsed = ((Date.now() - this.#startTime) / 1000).toFixed(1);
      this.#elapsedEl.textContent = `elapsed ${elapsed}s`;
    }
  }

  #render(): void {
    this.#orb?.setAttribute("transform", `rotate(${this.angle().toFixed(2)} 84 84)`);
  }

  #applyMode(): void {
    this.#host.classList.toggle("dial-fallback", this.#fallback);
    if (this.#phaseLine) {
      this.#phaseLine.textContent = this.#fallback
        ? "proving in your browser"
        : "proving with Presto";
    }
  }

  #setLabel(active: DialQuadrant): void {
    this.#labels.forEach((el, i) => {
      el.classList.toggle("on", i === active);
    });
  }

  #build(): void {
    this.#host.replaceChildren();
    const wrap = document.createElement("div");
    wrap.className = "dial-wrap";

    const svg = svgEl("svg", { viewBox: "0 0 168 168", class: "dial-svg", "aria-hidden": "true" });
    svg.append(
      svgEl("circle", {
        cx: "84",
        cy: "84",
        r: "58",
        class: "dial-track",
        "stroke-dasharray": "3 7",
        fill: "none",
      }),
    );
    this.#orb = svgEl("g", { class: "dial-orb" });
    this.#orb.append(
      svgEl("path", {
        d: "M84 18 l2.7 5.6 5.6 2.7 -5.6 2.7 -2.7 5.6 -2.7 -5.6 -5.6 -2.7 5.6 -2.7 Z",
        class: "dial-spark",
      }),
    );
    this.#bolt = svgEl("path", {
      d: "M90 60 L72 88 H84 L81 110 L103 79 H89 Z",
      class: "dial-bolt",
      "stroke-linejoin": "round",
    });
    svg.append(this.#orb, this.#bolt);
    wrap.append(svg);

    this.#labels = QUADRANT_LABELS.map((text, i) => {
      const span = document.createElement("span");
      span.className = `dial-label dial-label-${i}`;
      span.textContent = text;
      wrap.append(span);
      return span;
    });

    this.#phaseLine = document.createElement("div");
    this.#phaseLine.className = "dial-phase";
    this.#host.append(wrap, this.#phaseLine);
    this.#render();
  }
}
