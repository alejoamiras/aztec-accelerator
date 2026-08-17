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
    // B7: the typed error + the api-version constant are runtime values on the barrel.
    expect(typeof sdk.AcceleratorHttpError).toBe("function");
    expect(sdk.ACCELERATOR_API_VERSION).toBe(1);
    // Typed consts force the type-only barrel exports to resolve — dropping one from the barrel
    // (how AcceleratorProtocol went missing) becomes a `tsc --noEmit` compile error right here.
    const protocol: AcceleratorProtocol = "https";
    const phase: AcceleratorPhase = "proving";
    // B7: pins the new `version-mismatch` phase into the barrel's type surface.
    const versionPhase: AcceleratorPhase = "version-mismatch";
    expect(protocol).toBe("https");
    expect(phase).toBe("proving");
    expect(versionPhase).toBe("version-mismatch");
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

  test("MIGRATION references AcceleratorProtocol + the typed error, and SHIPS in the tarball (F15)", () => {
    const migration = read("../../MIGRATION.md");
    expect(migration).toContain("AcceleratorProtocol");
    expect(migration).toContain("AcceleratorHttpError");
    // F15: MIGRATION.md must be in `files` — otherwise npm never packs it and consumers never see it.
    const pkg = JSON.parse(read("../../package.json"));
    expect(pkg.files).toContain("MIGRATION.md");
  });
});
