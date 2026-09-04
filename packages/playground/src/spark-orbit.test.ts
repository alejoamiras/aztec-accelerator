import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import type { AnimationPhase } from "./phase-queue";
import { MIN_DISPLAY_MS, PhaseQueue } from "./phase-queue";
import { phaseToDial, QUADRANT_LABELS, SparkOrbitController } from "./spark-orbit";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe("PhaseQueue", () => {
  test("first phase displays immediately", () => {
    const seen: AnimationPhase[] = [];
    const q = new PhaseQueue((p) => seen.push(p));
    q.push("detect");
    expect(seen).toEqual(["detect"]);
    expect(q.current).toBe("detect");
    q.clear();
  });

  test("subsequent phases wait out MIN_DISPLAY_MS", async () => {
    const seen: AnimationPhase[] = [];
    const q = new PhaseQueue((p) => seen.push(p));
    q.push("detect");
    q.push("proving");
    expect(seen).toEqual(["detect"]);
    await sleep(MIN_DISPLAY_MS + 100);
    expect(seen).toEqual(["detect", "proving"]);
    q.clear();
  });

  test("stays on current phase when queue is empty", async () => {
    const seen: AnimationPhase[] = [];
    const q = new PhaseQueue((p) => seen.push(p));
    q.push("proving");
    await sleep(MIN_DISPLAY_MS + 100);
    expect(seen).toEqual(["proving"]);
    expect(q.current).toBe("proving");
    q.clear();
  });

  test("clear resets state", () => {
    const q = new PhaseQueue(() => {});
    q.push("detect");
    q.clear();
    expect(q.current).toBeNull();
  });
});

describe("phaseToDial", () => {
  const cases: Array<[AnimationPhase, number, boolean]> = [
    ["detect", 0, false],
    ["downloading", 0, false],
    ["serialize", 0, false],
    ["app:simulate", 0, false],
    ["transmit", 1, false],
    ["proving", 2, false],
    ["app:prove", 2, false],
    ["denied", 2, true],
    ["version-mismatch", 2, true],
    ["fallback", 2, true],
    ["secure-connection-unavailable", 2, true],
    ["proved", 3, false],
    ["receive", 3, false],
    ["app:confirm", 3, false],
  ];

  test("every phase maps to a quadrant; fallback family continues (never terminal)", () => {
    for (const [phase, quadrant, fallback] of cases) {
      const t = phaseToDial(phase);
      expect(`${phase}:${t.quadrant}:${Boolean(t.fallback)}`).toBe(
        `${phase}:${quadrant}:${fallback}`,
      );
    }
  });
});

describe("SparkOrbitController", () => {
  let host: HTMLElement;
  let elapsed: HTMLElement;

  beforeEach(() => {
    host = document.createElement("div");
    host.id = "ascii-art";
    host.className = "dial-host hidden";
    elapsed = document.createElement("span");
    elapsed.id = "ascii-elapsed";
    document.body.append(host, elapsed);
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("start builds the dial and unhides the host", () => {
    const c = new SparkOrbitController(host, elapsed, { reducedMotion: true });
    c.start("accelerated");
    expect(host.classList.contains("hidden")).toBe(false);
    expect(host.querySelector(".dial-svg")).not.toBeNull();
    expect(host.querySelectorAll(".dial-label").length).toBe(QUADRANT_LABELS.length);
    expect(host.querySelector(".dial-label-0")?.classList.contains("on")).toBe(true);
    c.stop();
  });

  test("phases advance the active label and the angle never rewinds", async () => {
    const c = new SparkOrbitController(host, elapsed, { reducedMotion: true });
    c.start("accelerated");
    const a0 = c.angle();
    c.pushPhase("detect");
    await sleep(MIN_DISPLAY_MS + 80);
    c.pushPhase("transmit");
    await sleep(MIN_DISPLAY_MS + 80);
    expect(host.querySelector(".dial-label-1")?.classList.contains("on")).toBe(true);
    const a1 = c.angle();
    expect(a1).toBeGreaterThan(a0);
    c.pushPhase("proving");
    await sleep(MIN_DISPLAY_MS + 80);
    c.pushPhase("proved");
    await sleep(MIN_DISPLAY_MS + 80);
    expect(host.querySelector(".dial-label-3")?.classList.contains("on")).toBe(true);
    expect(c.angle()).toBeGreaterThan(a1);
    c.stop();
  });

  test("fallback switches to in-browser mode and keeps progressing", async () => {
    const c = new SparkOrbitController(host, elapsed, { reducedMotion: true });
    c.start("accelerated");
    expect(host.classList.contains("dial-fallback")).toBe(false);
    c.pushPhase("denied");
    await sleep(50);
    expect(host.classList.contains("dial-fallback")).toBe(true);
    expect(host.querySelector(".dial-phase")?.textContent).toBe("proving in your browser");
    const before = c.angle();
    c.pushPhase("receive");
    await sleep(MIN_DISPLAY_MS + 80);
    expect(host.querySelector(".dial-label-3")?.classList.contains("on")).toBe(true);
    expect(c.angle()).toBeGreaterThanOrEqual(before);
    c.stop();
  });

  test("reduced motion renders statically (no interval) and stop clears everything", async () => {
    const c = new SparkOrbitController(host, elapsed, { reducedMotion: true });
    c.start("accelerated");
    c.pushPhase("proving");
    await sleep(30);
    const orb = host.querySelector(".dial-orb");
    const t1 = orb?.getAttribute("transform");
    await sleep(250);
    expect(orb?.getAttribute("transform")).toBe(t1 as string);
    c.stop();
    expect(host.classList.contains("hidden")).toBe(true);
    expect(host.children.length).toBe(0);
    expect(elapsed.textContent).toBe("");
  });

  test("a new run laps forward instead of rewinding", async () => {
    const c = new SparkOrbitController(host, elapsed, { reducedMotion: true });
    c.start("accelerated");
    c.pushPhase("proved");
    await sleep(30);
    const atTada = c.angle();
    c.pushPhase("app:simulate"); // next run starts at quadrant 0 — must lap, not rewind
    await sleep(MIN_DISPLAY_MS + 80);
    expect(c.angle()).toBeGreaterThan(atTada);
    c.stop();
  });

  test("a repeated same-quadrant phase after a lap never rewinds the angle", async () => {
    const c = new SparkOrbitController(host, elapsed, { reducedMotion: false });
    c.start("accelerated");
    c.pushPhase("proved"); // quadrant 3
    await sleep(MIN_DISPLAY_MS + 80);
    c.pushPhase("app:simulate"); // laps to quadrant 0
    // Let quadrant 0 ease MATERIALLY (p well past what a reset could re-reach quickly), so a
    // rewind bug produces a strictly smaller sample rather than a within-jitter equal one.
    await sleep(MIN_DISPLAY_MS + 1200);
    const afterLap = c.angle();
    c.pushPhase("detect"); // same quadrant 0 again — must not reset easing
    // Sample shortly after the queued phase is applied: a reset would sit far below afterLap.
    await sleep(MIN_DISPLAY_MS + 150);
    expect(c.angle()).toBeGreaterThanOrEqual(afterLap);
    c.stop();
  });
});
