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

  test("every setup-bun site uses bun-version-file, never an inline bun-version", () => {
    let sites = 0;
    for (const file of files) {
      const lines = readFileSync(file, "utf8").split("\n");
      lines.forEach((line, i) => {
        if (/^\s*bun-version\s*:/.test(line)) {
          expect(true, `${file}:${i + 1} — inline bun-version reintroduced; use bun-version-file`).toBe(false);
        }
        if (/^\s*bun-version-file\s*:\s*\.bun-version\s*$/.test(line)) sites++;
      });
    }
    expect(sites, "sweep is vacuous — no setup-bun sites found").toBeGreaterThan(20);
  });
});
