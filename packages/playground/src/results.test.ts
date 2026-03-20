import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import {
  buildDotRow,
  formatMs,
  renderSteps,
  shortFnName,
  showResult,
  stepToPhase,
} from "./results";
import { setupDOM } from "./test-helpers";

describe("formatMs", () => {
  test("formats sub-second as ms", () => {
    expect(formatMs(42)).toBe("42ms");
    expect(formatMs(999)).toBe("999ms");
  });

  test("formats 1000ms+ as seconds", () => {
    expect(formatMs(1000)).toBe("1.0s");
    expect(formatMs(1500)).toBe("1.5s");
    expect(formatMs(12345)).toBe("12.3s");
  });

  test("formats 0ms", () => {
    expect(formatMs(0)).toBe("0ms");
  });
});

describe("shortFnName", () => {
  test("extracts function name after colon", () => {
    expect(shortFnName("TokenContract:mint_to_private")).toBe("mint_to_private");
  });

  test("returns full name when no colon", () => {
    expect(shortFnName("standalone_fn")).toBe("standalone_fn");
  });

  test("returns 'unknown' for empty string", () => {
    expect(shortFnName("")).toBe("unknown");
  });

  test("handles trailing colon gracefully", () => {
    expect(shortFnName("Contract:")).toBe("Contract:");
  });
});

describe("buildDotRow", () => {
  test("creates a div with label, dots, and value spans", () => {
    const row = buildDotRow("test-class", "label", "label-class", "value", "value-class");
    expect(row.className).toBe("test-class");
    expect(row.children.length).toBe(3);
    expect(row.children[0].textContent).toBe("label");
    expect(row.children[0].className).toBe("label-class");
    expect(row.children[1].className).toBe("step-dots");
    expect(row.children[2].textContent).toBe("value");
    expect(row.children[2].className).toBe("value-class");
  });
});

describe("stepToPhase", () => {
  test("maps simulation steps", () => {
    expect(stepToPhase("simulating deploy")).toBe("app:simulate");
  });

  test("maps proving/deploying steps", () => {
    expect(stepToPhase("proving + sending")).toBe("app:prove");
    expect(stepToPhase("deploying token")).toBe("app:prove");
  });

  test("maps confirming steps", () => {
    expect(stepToPhase("confirming token deploy")).toBe("app:confirm");
  });

  test("returns null for unknown steps", () => {
    expect(stepToPhase("checking balances")).toBeNull();
    expect(stepToPhase("creating account")).toBeNull();
  });
});

describe("renderSteps", () => {
  beforeEach(() => setupDOM());
  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("renders steps into a details element", () => {
    const container = document.createElement("div");
    container.classList.add("hidden");
    document.body.appendChild(container);

    renderSteps(container, [
      { step: "deploy token", durationMs: 1500 },
      { step: "check balances", durationMs: 200 },
    ]);

    expect(container.classList.contains("hidden")).toBe(false);
    const details = container.querySelector("details");
    expect(details).toBeTruthy();
    expect(details!.querySelector("summary")!.textContent).toBe("2 steps");
  });
});

describe("showResult", () => {
  beforeEach(() => setupDOM());
  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("shows embedded result with empty prefix", () => {
    showResult("", "local", 5000, "cold");

    expect(document.getElementById("results")!.classList.contains("hidden")).toBe(false);
    expect(document.getElementById("time-local")!.textContent).toBe("5.0s");
    expect(document.getElementById("tag-local")!.textContent).toBe("cold");
    expect(document.getElementById("result-local")!.classList.contains("result-filled")).toBe(true);
  });

  test("shows accelerated result", () => {
    showResult("", "accelerated", 3000, "token flow");

    expect(document.getElementById("results")!.classList.contains("hidden")).toBe(false);
    expect(document.getElementById("time-accelerated")!.textContent).toBe("3.0s");
    expect(document.getElementById("tag-accelerated")!.textContent).toBe("token flow");
    expect(document.getElementById("result-accelerated")!.classList.contains("result-filled")).toBe(
      true,
    );
  });
});
