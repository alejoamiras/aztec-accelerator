import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import type { AcceleratorPhase, AcceleratorProtocol } from "../index.js";
import * as sdk from "../index.js";

// F-05 — doc-sync guard. Pins the published contract so source ↔ barrel ↔ docs can't silently drift
// again: the README had documented the obsolete *flat* `AcceleratorStatus`, `AcceleratorProtocol` was
// missing from the barrel (so a documented import failed), and `setForceLocal` + the `denied` phase
// were undocumented.

const read = (rel: string) => readFileSync(fileURLToPath(new URL(rel, import.meta.url)), "utf8");

describe("public contract (F-05 doc-sync guard)", () => {
  test("barrel exports the runtime + type surface", () => {
    expect(typeof sdk.AcceleratorProver).toBe("function");
    // Typed consts force the type-only barrel exports to resolve — dropping one from the barrel
    // (how AcceleratorProtocol went missing) becomes a `tsc --noEmit` compile error right here.
    const protocol: AcceleratorProtocol = "https";
    const phase: AcceleratorPhase = "proving";
    expect(protocol).toBe("https");
    expect(phase).toBe("proving");
  });

  test("README documents the discriminated union, not the obsolete flat interface", () => {
    const readme = read("../../README.md");
    expect(readme).not.toContain("interface AcceleratorStatus {");
    expect(readme).toContain('reason: "offline"');
    expect(readme).toContain("setForceLocal");
  });

  test("README + SKILL phase tables both document the `denied` phase", () => {
    expect(read("../../README.md")).toContain("`denied`");
    expect(read("../../.claude/skills/aztec-accelerator/SKILL.md")).toContain("`denied`");
  });

  test("MIGRATION references AcceleratorProtocol (now delivered by the barrel)", () => {
    expect(read("../../MIGRATION.md")).toContain("AcceleratorProtocol");
  });
});
