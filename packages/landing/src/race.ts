/**
 * The hero race: one token flow timed in-browser (38.4s) vs with Presto (4.1s), replayed on a
 * loop. Numbers come from the playground's measured token-flow benchmark; update both together.
 * Honors prefers-reduced-motion by rendering the final state once, statically.
 */
const WASM_S = 38.4;
const PRESTO_S = 4.1;
const WASM_ANIM_MS = 7000;
const PRESTO_ANIM_MS = 750;
const LOOP_MS = 9000;

interface RaceEls {
  wBar: HTMLElement;
  pBar: HTMLElement;
  wNum: HTMLElement;
  pNum: HTMLElement;
  note: HTMLElement;
}

function els(): RaceEls | null {
  const get = (id: string) => document.getElementById(id);
  const wBar = get("race-w-bar");
  const pBar = get("race-p-bar");
  const wNum = get("race-w-num");
  const pNum = get("race-p-num");
  const note = get("race-note");
  return wBar && pBar && wNum && pNum && note ? { wBar, pBar, wNum, pNum, note } : null;
}

function renderFinal(r: RaceEls): void {
  r.wBar.style.width = "100%";
  r.pBar.style.width = "100%";
  r.wNum.textContent = `${WASM_S.toFixed(1)}s`;
  r.pNum.textContent = `${PRESTO_S.toFixed(1)}s`;
  r.note.style.visibility = "visible";
}

function runOnce(r: RaceEls): void {
  r.wBar.style.width = "0%";
  r.pBar.style.width = "0%";
  r.wNum.textContent = "0.0s";
  r.pNum.textContent = "0.0s";
  r.note.style.visibility = "hidden";
  const t0 = performance.now();
  const step = (t: number) => {
    const el = t - t0;
    const wp = Math.min(1, el / WASM_ANIM_MS);
    const pp = Math.min(1, el / PRESTO_ANIM_MS);
    r.wBar.style.width = `${wp * 100}%`;
    r.pBar.style.width = `${pp * 100}%`;
    r.wNum.textContent = `${(wp * WASM_S).toFixed(1)}s`;
    r.pNum.textContent = `${(pp * PRESTO_S).toFixed(1)}s`;
    if (pp >= 1) r.note.style.visibility = "visible";
    if (wp < 1) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

export function initRace(): void {
  const r = els();
  if (!r) return;
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
    renderFinal(r);
    return;
  }
  // Start when the card scrolls into view, then loop.
  let started = false;
  const start = () => {
    if (started) return;
    started = true;
    runOnce(r);
    setInterval(() => runOnce(r), LOOP_MS);
  };
  const card = document.getElementById("race");
  if (!card) {
    start();
    return;
  }
  const obs = new IntersectionObserver(
    (entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        start();
        obs.disconnect();
      }
    },
    { threshold: 0.3 },
  );
  obs.observe(card);
}
