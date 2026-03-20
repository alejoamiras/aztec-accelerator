import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { setupDOM } from "./test-helpers";
import { $, appendLog, formatDuration, resetLogCount, setStatus } from "./ui";

describe("formatDuration", () => {
  test("formats 0ms", () => {
    expect(formatDuration(0)).toBe("0.0s");
  });

  test("formats 500ms", () => {
    expect(formatDuration(500)).toBe("0.5s");
  });

  test("formats 1000ms", () => {
    expect(formatDuration(1000)).toBe("1.0s");
  });

  test("formats 1500ms", () => {
    expect(formatDuration(1500)).toBe("1.5s");
  });

  test("formats 60000ms", () => {
    expect(formatDuration(60000)).toBe("60.0s");
  });

  test("formats 123456ms", () => {
    expect(formatDuration(123456)).toBe("123.5s");
  });
});

describe("$", () => {
  beforeEach(() => setupDOM());
  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("returns element when it exists", () => {
    const el = $("clock");
    expect(el).toBeInstanceOf(HTMLElement);
    expect(el.id).toBe("clock");
  });

  test("throws when element does not exist", () => {
    expect(() => $("nonexistent")).toThrow("Element #nonexistent not found");
  });
});

describe("setStatus", () => {
  beforeEach(() => setupDOM());
  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("sets status-online when connected=true", () => {
    setStatus("aztec-status", true);
    const el = $("aztec-status");
    expect(el.className).toContain("status-online");
    expect(el.className).not.toContain("status-offline");
    expect(el.className).not.toContain("status-unknown");
  });

  test("sets status-offline when connected=false", () => {
    setStatus("aztec-status", false);
    const el = $("aztec-status");
    expect(el.className).toContain("status-offline");
    expect(el.className).not.toContain("status-online");
  });

  test("sets status-unknown when connected=null", () => {
    setStatus("aztec-status", null);
    const el = $("aztec-status");
    expect(el.className).toContain("status-unknown");
    expect(el.className).not.toContain("status-online");
  });
});

describe("appendLog", () => {
  beforeEach(() => {
    setupDOM();
    resetLogCount();
  });
  afterEach(() => {
    document.body.innerHTML = "";
  });

  test("appends a child div to the log element", () => {
    appendLog("test message");
    expect($("log").children.length).toBe(1);
  });

  test("sets log-info class for info level", () => {
    appendLog("msg", "info");
    expect($("log").children[0].className).toBe("log-info");
  });

  test("sets log-error class for error level", () => {
    appendLog("msg", "error");
    expect($("log").children[0].className).toBe("log-error");
  });

  test("sets log-warn class for warn level", () => {
    appendLog("msg", "warn");
    expect($("log").children[0].className).toBe("log-warn");
  });

  test("sets log-success class for success level", () => {
    appendLog("msg", "success");
    expect($("log").children[0].className).toBe("log-success");
  });

  test("defaults to info level", () => {
    appendLog("msg");
    expect($("log").children[0].className).toBe("log-info");
  });

  test("updates log-count text", () => {
    appendLog("a");
    expect($("log-count").textContent).toBe("1 entry");
    appendLog("b");
    expect($("log-count").textContent).toBe("2 entries");
  });
});
