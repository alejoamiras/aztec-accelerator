import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, expect, test } from "bun:test";

/**
 * Tripwire for the JEST_WORKER_ID contract the test preloads rely on: @aztec/foundation's logger
 * must keep its Jest branch — a truthy JEST_WORKER_ID takes a synchronous fd destination and
 * NEVER constructs the worker-thread transport (which crashes under bun 1.4.0 after happy-dom
 * replaces the global MessagePort — oven-sh/bun#40268). This is unversioned upstream behavior:
 * if a future @aztec bump renames or removes the branch, this test fails the bump PR instead of
 * the playground suite crashing at import time.
 *
 * Structural verification of the actual branch (not a bare variable-name grep): the
 * JEST_WORKER_ID conditional's consequent must reach pino.destination, and the transport
 * construction must live outside that consequent. Resolved from a declaring workspace via
 * Bun.resolveSync so the check survives the isolated linker's node_modules layout.
 */
describe("@aztec/foundation logger JEST_WORKER_ID contract", () => {
  const entry = Bun.resolveSync("@aztec/foundation/log", join(import.meta.dir, "../packages/sdk"));
  const loggerPath = join(dirname(entry), "pino-logger.js");
  const src = readFileSync(loggerPath, "utf8");

  test("the Jest branch exists, is positively gated, and takes the sync non-worker destination", () => {
    // The condition must be the bare truthy read — a negated (`!JEST_WORKER_ID`) or
    // equality-restricted (`=== "x"`) rewrite would invert/narrow the bypass while still
    // matching a loose contains-check.
    const m = src.match(/if\s*\(\s*(!?)\s*process\.env\.JEST_WORKER_ID\s*([^)]*)\)\s*(\{[\s\S]*?\n\s*\})/);
    expect(m, `no JEST_WORKER_ID conditional found in ${loggerPath}`).toBeTruthy();
    expect(m?.[1], "JEST_WORKER_ID condition is NEGATED — the bypass polarity flipped").toBe("");
    expect((m?.[2] ?? "").trim(), "JEST_WORKER_ID condition is no longer a bare truthy check").toBe("");
    const consequent = m?.[3] ?? "";
    expect(consequent, "Jest branch no longer uses pino.destination — the sync no-worker contract broke").toContain(
      "pino.destination",
    );
    expect(consequent, "Jest branch unexpectedly builds a transport (worker) — the bypass is gone").not.toContain(
      "pino.transport",
    );
  });

  test("the worker transport is still what the non-Jest path builds (bypass is meaningful)", () => {
    expect(src).toContain("pino.transport");
  });
});
