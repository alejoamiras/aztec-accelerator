import type { AcceleratorPhase } from "@alejoamiras/aztec-accelerator";
import type { UiMode } from "./aztec";

// ── Phase types ──

/** All phases the animation can display — SDK prover phases + app-level phases. */
export type AnimationPhase = AcceleratorPhase | "app:simulate" | "app:prove" | "app:confirm";

// ── Phase queue ──

const MIN_DISPLAY_MS = 1000;

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

// ── ASCII frames ──

const BRAILLE_SPINNER = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

function progressBar(tick: number, width: number): string {
  const filled = Math.min(width, Math.floor((tick % (width * 3)) / 3));
  return "█".repeat(filled) + "░".repeat(width - filled);
}

function spin(tick: number): string {
  return BRAILLE_SPINNER[tick % BRAILLE_SPINNER.length];
}

// ── Box helpers — auto-pad all rows to the widest line, impossible to misalign ──

type Border = "single" | "double" | "round";

const BORDER_CHARS: Record<
  Border,
  { tl: string; tr: string; bl: string; br: string; h: string; v: string }
> = {
  single: { tl: "┌", tr: "┐", bl: "└", br: "┘", h: "─", v: "│" },
  double: { tl: "╔", tr: "╗", bl: "╚", br: "╝", h: "═", v: "║" },
  round: { tl: "╭", tr: "╮", bl: "╰", br: "╯", h: "─", v: "│" },
};

/**
 * Build a perfectly aligned ASCII box. All rows are auto-padded to the widest
 * content line. Title is embedded in the top border if provided.
 */
function box(lines: string[], border: Border = "single", title?: string): string {
  const b = BORDER_CHARS[border];
  const w = Math.max(...lines.map((l) => l.length));
  const topFill = title
    ? `${b.h} ${title} ${b.h.repeat(Math.max(0, w - title.length - 1))}`
    : b.h.repeat(w + 2);
  return [
    `  ${b.tl}${topFill}${b.tr}`,
    ...lines.map((l) => `  ${b.v} ${l.padEnd(w)} ${b.v}`),
    `  ${b.bl}${b.h.repeat(w + 2)}${b.br}`,
  ].join("\n");
}

// ── Frame generators per (mode, phase) ──

type FrameFn = (tick: number) => string;

function simulateFrames(): FrameFn {
  return (tick) =>
    box(
      [`> simulating tx...    ${spin(tick)}`, `  ${progressBar(tick, 22)}`],
      "round",
      "witness generation",
    );
}

function serializeFrames(): FrameFn {
  const lines = [
    "fn: deploy_account",
    "args: 0x7a2f...3b1c",
    "witness: Map(42)",
    "bytecode: 1.2MB",
  ];
  return (tick) => {
    const shown = Math.min(lines.length, Math.floor(tick / 2) + 1);
    const rows = lines.map((l, i) => (i < shown ? l : ""));
    return box([...rows, "", `  ${progressBar(tick, 22)}`], "single", "SERIALIZING");
  };
}

function detectFrames(): FrameFn {
  return (tick) =>
    box([`> checking accelerator ${spin(tick)}`, "  localhost"], "round", "health check");
}

function fallbackFrames(): FrameFn {
  return (tick) =>
    box([`  accelerator offline  ${spin(tick)}`, "  falling back to wasm"], "round", "⚠ FALLBACK");
}

function transmitFrames(): FrameFn {
  const trackW = 24;
  const slots = trackW - 4;
  return (tick) => {
    const pos = tick % (slots + 1);
    const track = `${"░".repeat(pos)}>>>${"░".repeat(slots - pos)}▸`;
    return box([track, `payload                ${spin(tick)}`], "single", "SENDING to ACCELERATOR");
  };
}

const MODE_CONFIG: Record<UiMode, { border: Border; title: string; wrap?: Border }> = {
  local: { border: "single", title: "wasm prover", wrap: "round" },
  accelerated: { border: "single", title: "NATIVE ACCELERATOR", wrap: "round" },
};

function provingFrames(mode: UiMode): FrameFn {
  const W = 24;
  const { border, title, wrap } = MODE_CONFIG[mode];

  const inputPlain = [
    "fn: deploy_account",
    "args: 0x7a2f...3b1c",
    "witness: Map(42)",
    "bytecode: 1.2 MB",
  ].map((s) => s.padEnd(W));

  const inputCipher = [
    "a4F8#kL$mN2&pQ9*rT5^w%8!",
    "xC7@dG3%jH6(bE0)vI4+zRn#",
    "qW9$tY1&uO5*iP8^eA2!sD6@",
    "mK3#fJ7$gL4%hN0&bV9*cX2+",
  ];

  const proofPlain = ["proof: 0xab3f...c712", "public_inputs: [42,..]"].map((s) => s.padEnd(W));

  const proofCipher = ["rH5&nB8*vL2#tF6@wK9$jQ1%", "eM4^pC7!xA3+yD0(sG5)uI2&"];

  const blank = "".padEnd(W);

  return (tick) => {
    let inputRows: string[];
    let proofRows: string[];

    if (tick < 10) {
      // Stage 1: inputs appear line-by-line
      const shown = Math.min(inputPlain.length, Math.floor(tick / 2) + 1);
      inputRows = inputPlain.map((l, i) => (i < shown ? l : blank));
      proofRows = [blank, blank];
    } else if (tick < 30) {
      // Stage 2: inputs morph to cipher
      const progress = Math.min(1, (tick - 10) / 20);
      inputRows = inputPlain.map((p, i) => {
        const c = inputCipher[i];
        const cut = Math.floor(c.length * progress);
        return c.slice(0, cut) + p.slice(cut);
      });
      proofRows = [blank, blank];
    } else {
      // Stage 3: ciphered inputs scroll, proof crystallizes then shimmers
      inputRows = inputCipher.map((c, i) => {
        const shift = (tick + i * 7) % c.length;
        return c.slice(shift) + c.slice(0, shift);
      });
      const proofProgress = Math.min(1, (tick - 30) / 10);
      if (proofProgress < 1) {
        proofRows = proofCipher.map((c, i) => {
          const p = proofPlain[i];
          const cut = Math.floor(p.length * proofProgress);
          return p.slice(0, cut) + c.slice(cut);
        });
      } else {
        // Shimmer — a few glitch characters scan across the proof lines
        proofRows = proofPlain.map((p, i) => {
          const c = proofCipher[i];
          const chars = [...p];
          for (let j = 0; j < 3; j++) {
            const idx = (tick + i * 5 + j) % W;
            chars[idx] = c[idx];
          }
          return chars.join("");
        });
      }
    }

    const lines = [blank, ...inputRows, blank, ...proofRows, blank, progressBar(tick, W), blank];

    const inner = box(lines, border, title);

    if (!wrap) return inner;

    // Wrap in outer round box (local mode) — strip inner indent for symmetric padding
    const innerLines = inner.split("\n").map((l) => l.slice(2));
    const w = Math.max(...innerLines.map((l) => l.length)) + 2;
    return [
      `  ╭${"─".repeat(w)}╮`,
      ...innerLines.map((l) => `  │ ${l.padEnd(w - 1)}│`),
      `  ╰${"─".repeat(w)}╯`,
    ].join("\n");
  };
}

function receiveFrames(): FrameFn {
  const cipher = ["a4F8#kL$mN2&pQ9*rT5^w", "xC7@dG3%jH6(bE0)vI4+z"];
  const plain = ["proof: 0xab3f...c712  ", "publicInputs: [42,..]  "];
  return (tick) => {
    const progress = Math.min(1, tick / 10);
    const rows = cipher.map((c, i) => {
      const p = plain[i];
      const cut = Math.floor(p.length * progress);
      return p.slice(0, cut) + c.slice(cut);
    });
    const done = progress >= 1 ? "✓" : spin(tick);
    return box([...rows, "", `  decrypting result      ${done}`], "single", "RECEIVED");
  };
}

function confirmFrames(): FrameFn {
  return (tick) =>
    box([`> broadcasting tx...  ${spin(tick)}`, `  ${progressBar(tick, 22)}`], "round", "proof ✓");
}

function provedFrames(): FrameFn {
  return () => box(["  proof complete         ✓", "  waiting for response..."], "single", "PROVED");
}

function deniedFrames(): FrameFn {
  return () => box(["  site not authorized    ✗", "  falling back to wasm"], "round", "⚠ DENIED");
}

/** Return the frame generator for a given (mode, phase) combination. */
export function getFrameFn(mode: UiMode, phase: AnimationPhase): FrameFn {
  switch (phase) {
    case "detect":
      return detectFrames();
    case "fallback":
      return fallbackFrames();
    case "downloading":
      return detectFrames(); // reuse detect animation for download
    case "app:simulate":
      return simulateFrames();
    case "serialize":
      return serializeFrames();
    case "transmit":
      return transmitFrames();
    case "proving":
    case "app:prove":
      return provingFrames(mode);
    case "proved":
      return provedFrames();
    case "denied":
      return deniedFrames();
    case "receive":
      return receiveFrames();
    case "app:confirm":
      return confirmFrames();
  }
}

// ── Controller ──

const FRAME_INTERVAL_MS = 100;

export class AsciiController {
  #el: HTMLElement;
  #elapsedEl: HTMLElement | null;
  #mode: UiMode = "local";
  #queue: PhaseQueue;
  #frameFn: FrameFn | null = null;
  #tick = 0;
  #startTime = 0;
  #animTimer: ReturnType<typeof setInterval> | null = null;

  constructor(el: HTMLElement, elapsedEl?: HTMLElement | null) {
    this.#el = el;
    this.#elapsedEl = elapsedEl ?? null;
    this.#queue = new PhaseQueue((phase) => {
      this.#frameFn = getFrameFn(this.#mode, phase);
      this.#tick = 0;
    });
  }

  start(mode: UiMode): void {
    this.#mode = mode;
    this.#startTime = Date.now();
    this.#el.classList.remove("hidden");
    this.#tick = 0;
    this.#animTimer = setInterval(() => {
      if (this.#frameFn) {
        this.#el.textContent = this.#frameFn(this.#tick);
        if (this.#elapsedEl) {
          const elapsed = ((Date.now() - this.#startTime) / 1000).toFixed(1);
          this.#elapsedEl.textContent = `elapsed ${elapsed}s`;
        }
        this.#tick++;
      }
    }, FRAME_INTERVAL_MS);
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
    this.#frameFn = null;
    this.#el.classList.add("hidden");
    this.#el.textContent = "";
    if (this.#elapsedEl) this.#elapsedEl.textContent = "";
  }
}
