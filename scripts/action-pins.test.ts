import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

/**
 * Pin invariant for third-party GitHub Actions: every `uses:` of an external action is a full
 * 40-hex commit SHA with an exact `# vX.Y.Z` release label. SHAs are immutable where tags are
 * not (tag-hijack supply chain); the exact label keeps humans and dependabot honest — label
 * drift is real (a SHA sat labeled v6 while being the v7.0.1 release commit).
 *
 * Exceptions, each deliberate:
 * - local references (`./.github/...`) and reusable-workflow calls need no pin;
 * - dtolnay/rust-toolchain publishes no releases — its moving `v1` tag's commit is pinned and
 *   labeled `# v1`.
 */
const ROOT = join(import.meta.dir, "..");
const EXCEPTIONS = new Set(["dtolnay/rust-toolchain"]);
const PIN_RE = /^[a-f0-9]{40}$/;
const LABEL_RE = /^# v\d+\.\d+\.\d+$/;

function ymlFiles(dir: string): string[] {
  return readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((e) => e.isFile() && /\.ya?ml$/.test(e.name))
    .map((e) => join(e.parentPath, e.name));
}

function externalUses(): { file: string; line: number; action: string; ref: string; label: string }[] {
  const out: { file: string; line: number; action: string; ref: string; label: string }[] = [];
  for (const dir of [join(ROOT, ".github/workflows"), join(ROOT, ".github/actions")]) {
    for (const file of ymlFiles(dir)) {
      const lines = readFileSync(file, "utf8").split("\n");
      lines.forEach((raw, i) => {
        // Fail closed: any line that DECLARES uses: must parse — a trailing space or odd
        // shape must fail the sweep, not silently drop the reference from it.
        if (!/^\s*-?\s*uses\s*:/.test(raw)) return;
        const m = raw.match(/^\s*-?\s*uses\s*:\s*([^\s#]+)\s*(#[^\n]*?)?\s*$/);
        const target = m?.[1];
        expect(target, `${file}:${i + 1} — unparseable uses: declaration: ${JSON.stringify(raw)}`).toBeTruthy();
        if (!target) return;
        const comment = m?.[2] ?? "";
        if (target.startsWith("./")) return;
        const at = target.lastIndexOf("@");
        expect(at, `${file}:${i + 1} — unpinned action reference "${target}"`).toBeGreaterThan(0);
        out.push({
          file,
          line: i + 1,
          action: target.slice(0, at),
          ref: target.slice(at + 1),
          label: comment.trim(),
        });
      });
    }
  }
  return out;
}

describe("third-party action pins", () => {
  const uses = externalUses();

  test("references were found (sweep is not vacuous)", () => {
    expect(uses.length).toBeGreaterThan(50);
  });

  test("every external action is pinned to a full commit SHA", () => {
    for (const u of uses) {
      expect(PIN_RE.test(u.ref), `${u.file}:${u.line} — ${u.action}@${u.ref} is not a 40-hex SHA pin`).toBe(true);
    }
  });

  test("every pin carries an exact release label (or is a named exception)", () => {
    for (const u of uses) {
      if (EXCEPTIONS.has(u.action)) {
        expect(u.label, `${u.file}:${u.line} — exception ${u.action} must be labeled exactly "# v1"`).toBe("# v1");
        continue;
      }
      expect(LABEL_RE.test(u.label), `${u.file}:${u.line} — ${u.action} label "${u.label}" is not "# vX.Y.Z"`).toBe(
        true,
      );
    }
  });

  test("one SHA and one label per action (no split-brain pins or drifting labels)", () => {
    const shasByAction = new Map<string, Set<string>>();
    const labelsByAction = new Map<string, Set<string>>();
    for (const u of uses) {
      const key = u.action.split("/").slice(0, 2).join("/");
      if (!shasByAction.has(key)) shasByAction.set(key, new Set());
      if (!labelsByAction.has(key)) labelsByAction.set(key, new Set());
      shasByAction.get(key)?.add(u.ref);
      labelsByAction.get(key)?.add(u.label);
    }
    for (const [action, shas] of shasByAction) {
      expect(shas.size, `${action} is pinned to ${shas.size} different SHAs: ${[...shas].join(", ")}`).toBe(1);
    }
    for (const [action, labels] of labelsByAction) {
      expect(labels.size, `${action} carries ${labels.size} different labels: ${[...labels].join(", ")}`).toBe(1);
    }
  });
});
