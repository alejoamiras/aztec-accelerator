import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { type AnimationPhase, AsciiController, getFrameFn, PhaseQueue } from "./ascii-animation";
import type { UiMode } from "./aztec";

describe("PhaseQueue", () => {
  test("first push is displayed immediately", () => {
    const phases: AnimationPhase[] = [];
    const queue = new PhaseQueue((p) => phases.push(p));
    queue.push("serialize");
    expect(phases).toEqual(["serialize"]);
    expect(queue.current).toBe("serialize");
    queue.clear();
  });

  test("queued phases drain in order", async () => {
    const phases: AnimationPhase[] = [];
    const queue = new PhaseQueue((p) => phases.push(p));
    queue.push("serialize");
    queue.push("transmit");
    queue.push("proving");
    expect(phases).toEqual(["serialize"]);

    // Wait for two drain cycles (1000ms each + buffer)
    await new Promise((r) => setTimeout(r, 2200));
    expect(phases).toEqual(["serialize", "transmit", "proving"]);
    queue.clear();
  });

  test("clear resets state", () => {
    const phases: AnimationPhase[] = [];
    const queue = new PhaseQueue((p) => phases.push(p));
    queue.push("proving");
    queue.push("receive");
    queue.clear();
    expect(queue.current).toBeNull();
  });

  test("stays on current phase when queue is empty", async () => {
    const phases: AnimationPhase[] = [];
    const queue = new PhaseQueue((p) => phases.push(p));
    queue.push("proving");
    // Wait past the min display time — should not emit anything new
    await new Promise((r) => setTimeout(r, 1100));
    expect(phases).toEqual(["proving"]);
    expect(queue.current).toBe("proving");
    queue.clear();
  });
});

describe("getFrameFn", () => {
  const allModes: UiMode[] = ["local", "accelerated"];
  const allPhases: AnimationPhase[] = [
    "detect",
    "fallback",
    "downloading",
    "app:simulate",
    "serialize",
    "transmit",
    "proving",
    "receive",
    "app:prove",
    "app:confirm",
  ];

  for (const mode of allModes) {
    for (const phase of allPhases) {
      test(`(${mode}, ${phase}) returns non-empty string`, () => {
        const fn = getFrameFn(mode, phase);
        const frame = fn(0);
        expect(typeof frame).toBe("string");
        expect(frame.length).toBeGreaterThan(0);
      });
    }
  }

  test("box frames have consistent line widths", () => {
    const boxPhases: [UiMode, AnimationPhase][] = [
      ["accelerated", "app:simulate"],
      ["accelerated", "serialize"],
      ["accelerated", "proving"],
      ["accelerated", "receive"],
      ["local", "proving"],
    ];
    for (const [mode, phase] of boxPhases) {
      const fn = getFrameFn(mode, phase);
      const frame = fn(5);
      const lines = frame.split("\n").filter((l) => l.length > 0);
      const widths = lines.map((l) => l.length);
      const maxW = Math.max(...widths);
      for (let i = 0; i < lines.length; i++) {
        expect(widths[i]).toBe(maxW);
      }
    }
  });

  test("frames change across ticks (proving animation)", () => {
    const fn = getFrameFn("accelerated", "proving");
    const frame0 = fn(0);
    const frame5 = fn(5);
    expect(frame0).not.toBe(frame5);
  });

  test("proving stage 3 contains proof and public_inputs", () => {
    const fn = getFrameFn("accelerated", "proving");
    const frame = fn(38);
    expect(frame).toContain("proof:");
    expect(frame).toContain("public_inputs:");
  });

  test("proving alignment across all stages and modes", () => {
    const modes: UiMode[] = ["local", "accelerated"];
    const ticks = [0, 5, 15, 25, 35, 50];
    for (const mode of modes) {
      const fn = getFrameFn(mode, "proving");
      for (const tick of ticks) {
        const frame = fn(tick);
        const lines = frame.split("\n").filter((l) => l.length > 0);
        const widths = lines.map((l) => l.length);
        const maxW = Math.max(...widths);
        for (let i = 0; i < lines.length; i++) {
          if (widths[i] !== maxW) {
            throw new Error(
              `mode=${mode} tick=${tick} line=${i}: width ${widths[i]} !== ${maxW}\n"${lines[i]}"`,
            );
          }
        }
      }
    }
  });

  test("app:prove produces identical frames to proving", () => {
    const modes: UiMode[] = ["local", "accelerated"];
    for (const mode of modes) {
      const provingFn = getFrameFn(mode, "proving");
      const appProveFn = getFrameFn(mode, "app:prove");
      for (const tick of [0, 10, 30, 50]) {
        expect(provingFn(tick)).toBe(appProveFn(tick));
      }
    }
  });
});

describe("AsciiController", () => {
  let el: HTMLPreElement;

  beforeEach(() => {
    el = document.createElement("pre");
    el.id = "ascii-art";
    el.classList.add("hidden");
    document.body.appendChild(el);
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("start shows element and stop hides it", () => {
    const ctrl = new AsciiController(el);
    ctrl.start("local");
    expect(el.classList.contains("hidden")).toBe(false);
    ctrl.stop();
    expect(el.classList.contains("hidden")).toBe(true);
    expect(el.textContent).toBe("");
  });

  test("pushPhase renders frame content", async () => {
    const ctrl = new AsciiController(el);
    ctrl.start("accelerated");
    ctrl.pushPhase("proving");

    // Wait for one animation frame (100ms interval)
    await new Promise((r) => setTimeout(r, 150));
    expect(el.textContent!.length).toBeGreaterThan(0);
    expect(el.textContent).toContain("NATIVE ACCELERATOR");
    ctrl.stop();
  });

  test("stop clears content and timers", async () => {
    const ctrl = new AsciiController(el);
    ctrl.start("local");
    ctrl.pushPhase("proving");
    await new Promise((r) => setTimeout(r, 150));
    ctrl.stop();
    expect(el.textContent).toBe("");
    // Ensure no more updates after stop
    const snapshot = el.textContent;
    await new Promise((r) => setTimeout(r, 200));
    expect(el.textContent).toBe(snapshot);
  });
});
