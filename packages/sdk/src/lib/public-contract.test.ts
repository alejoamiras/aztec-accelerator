import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import type {
  AcceleratorPhase,
  AcceleratorProtocol,
  AcceleratorStatusCheckOptions,
  SecureConnectionDiagnosis,
} from "../index.js";
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
    const securePhase: AcceleratorPhase = "secure-connection-unavailable";
    const diagnosis: SecureConnectionDiagnosis = "tls-or-trust-failure";
    const statusOptions: AcceleratorStatusCheckOptions = { forceRefresh: true };
    expect(protocol).toBe("https");
    expect(phase).toBe("proving");
    expect(versionPhase).toBe("version-mismatch");
    expect(securePhase).toBe("secure-connection-unavailable");
    expect(diagnosis).toBe("tls-or-trust-failure");
    expect(statusOptions.forceRefresh).toBe(true);
  });

  test("README documents the discriminated union, not the obsolete flat interface", () => {
    const readme = read("../../README.md");
    expect(readme).not.toContain("interface AcceleratorStatus {");
    expect(readme).toContain('reason: "offline"');
    expect(readme).toContain('reason: "permission-blocked"');
    expect(readme).toContain("forceRefresh: true");
    expect(readme).toContain("setForceLocal");
  });

  test("README + SKILL phase tables both document the `denied` phase", () => {
    expect(read("../../README.md")).toContain("`denied`");
    expect(read("../../.claude/skills/aztec-accelerator/SKILL.md")).toContain("`denied`");
  });

  test("README + SKILL document the B7 surface (typed error, version-mismatch) and NOT peer-deps", () => {
    const readme = read("../../README.md");
    const skill = read("../../.claude/skills/aztec-accelerator/SKILL.md");
    // The typed error + the new phase must be documented in BOTH (F14 doc-sync).
    for (const doc of [readme, skill]) {
      expect(doc).toContain("AcceleratorHttpError");
      expect(doc).toContain("version-mismatch");
    }
    // F13 verdict is KEEP DEPS — the docs must NOT claim peer-dependency semantics the manifest doesn't
    // have (they used to say "Peer dependency: @aztec/...").
    expect(readme).not.toContain("Peer dependency");
    expect(skill).not.toContain("Peer dependency");
  });

  test("README + MIGRATION + packaged SKILL document permission-blocked and forced Retry", () => {
    for (const doc of [
      read("../../README.md"),
      read("../../MIGRATION.md"),
      read("../../.claude/skills/aztec-accelerator/SKILL.md"),
    ]) {
      expect(doc).toContain("permission-blocked");
      expect(doc).toContain("forceRefresh: true");
    }
  });

  test("README + MIGRATION + packaged SKILL document secure recovery and session-only HTTP consent", () => {
    for (const doc of [
      read("../../README.md"),
      read("../../MIGRATION.md"),
      read("../../.claude/skills/aztec-accelerator/SKILL.md"),
    ]) {
      expect(doc).toContain("secure-connection-unavailable");
      expect(doc).toContain("tls-or-trust-failure");
      expect(doc).toContain("httpsOnly: false");
      expect(doc).toContain("allowInsecureDowngrade: true");
      expect(doc).toContain("forceRefresh: true");
    }
  });

  test("MIGRATION references AcceleratorProtocol + the typed error, and SHIPS in the tarball (F15)", () => {
    const migration = read("../../MIGRATION.md");
    expect(migration).toContain("AcceleratorProtocol");
    expect(migration).toContain("AcceleratorHttpError");
    expect(migration).toContain("source-breaking for exhaustive TypeScript switches");
    expect(migration).toContain('reason: "permission-blocked"');
    // F15: MIGRATION.md must be in `files` — otherwise npm never packs it and consumers never see it.
    const pkg = JSON.parse(read("../../package.json"));
    expect(pkg.files).toContain("MIGRATION.md");
  });
});
