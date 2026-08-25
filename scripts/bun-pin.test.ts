import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

/**
 * Bun-version pin invariant: `.bun-version` is the single source of truth. Every `setup-bun`
 * call site must reference it via `bun-version-file` — an inline `bun-version:` reintroduces the
 * per-site drift this centralization removed (the publish pipeline once floated `latest` while
 * 22 sibling sites pinned an older version).
 */
const ROOT = join(import.meta.dir, "..");

function ymlFiles(dir: string): string[] {
  return readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((e) => e.isFile() && /\.ya?ml$/.test(e.name))
    .map((e) => join(e.parentPath, e.name));
}

describe("bun version pin", () => {
  const files = [...ymlFiles(join(ROOT, ".github/workflows")), ...ymlFiles(join(ROOT, ".github/actions"))];

  test(".bun-version is a single exact semver line", () => {
    const content = readFileSync(join(ROOT, ".bun-version"), "utf8");
    expect(content).toMatch(/^\d+\.\d+\.\d+\n?$/);
  });

  test("every setup-bun step carries bun-version-file in ITS with-block; no inline bun-version", () => {
    // Association, not co-occurrence: a new unpinned setup-bun step alongside 23 pinned ones must
    // fail, so each step is checked for a pin within its own `with:` block (bounded by the next
    // step's `- ` at equal-or-lower indent).
    let steps = 0;
    for (const file of files) {
      const lines = readFileSync(file, "utf8").split("\n");
      lines.forEach((line, i) => {
        if (/^\s*bun-version\s*:/.test(line)) {
          expect(true, `${file}:${i + 1} — inline bun-version reintroduced; use bun-version-file`).toBe(false);
        }
        const use = line.match(/^(\s*)-\s+uses:\s*oven-sh\/setup-bun@/);
        if (!use) return;
        steps++;
        const indent = use[1]?.length ?? 0;
        let pinned = false;
        for (let j = i + 1; j < lines.length; j++) {
          const l = lines[j] ?? "";
          if (new RegExp(`^\\s{0,${indent}}-\\s`).test(l)) break;
          if (/^\s*bun-version-file\s*:\s*\.bun-version\s*$/.test(l)) pinned = true;
        }
        expect(pinned, `${file}:${i + 1} — setup-bun step without bun-version-file in its with-block`).toBe(true);
      });
    }
    expect(steps, "sweep is vacuous — no setup-bun steps found").toBeGreaterThan(20);
  });
});
