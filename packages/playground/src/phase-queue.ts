import type { AcceleratorPhase } from "@alejoamiras/aztec-accelerator";

/** All phases the proving theater can display — SDK prover phases + app-level phases. */
export type AnimationPhase = AcceleratorPhase | "app:simulate" | "app:prove" | "app:confirm";

export const MIN_DISPLAY_MS = 1000;

/**
 * Buffers fast phases with a minimum display time so users can see each one.
 * When the queue empties on a long-running phase, it stays displayed until the next push.
 */
export class PhaseQueue {
  #queue: AnimationPhase[] = [];
  #current: AnimationPhase | null = null;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #onChange: (phase: AnimationPhase) => void;

  constructor(onChange: (phase: AnimationPhase) => void) {
    this.#onChange = onChange;
  }

  get current(): AnimationPhase | null {
    return this.#current;
  }

  push(phase: AnimationPhase): void {
    if (this.#current === null) {
      // First phase — display immediately
      this.#current = phase;
      this.#onChange(phase);
      this.#scheduleNext();
    } else {
      this.#queue.push(phase);
      this.#scheduleNext(); // Ensure drain timer is running
    }
  }

  #scheduleNext(): void {
    if (this.#timer) return;
    this.#timer = setTimeout(() => {
      this.#timer = null;
      const next = this.#queue.shift();
      if (next) {
        this.#current = next;
        this.#onChange(next);
        this.#scheduleNext();
      }
      // If queue is empty, stay on current phase
    }, MIN_DISPLAY_MS);
  }

  clear(): void {
    if (this.#timer) {
      clearTimeout(this.#timer);
      this.#timer = null;
    }
    this.#queue = [];
    this.#current = null;
  }
}
