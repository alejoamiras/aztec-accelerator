import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

// bunfig.toml's minimumReleaseAgeExcludes must track bun.lock's resolved @aztec graph EXACTLY
// (exact names only — Bun 1.3.14 silently ignores globs, and the age filter applies to
// transitive resolution too). The list is hand-maintained on every @aztec bump; this test is
// the enforcement the list's own maintenance comment prescribes, in both directions:
//  - a lock name missing from the excludes would fail the NEXT <7-day-old bump's install;
//  - a stale exclude (name no longer in the lock) silently keeps a permanent age-gate
//    exemption for a package that could re-enter the tree unreviewed.
// Runs under `bun run test:scripts`, which every @aztec bump PR triggers via the SDK pipeline.

const ROOT = join(import.meta.dir, "..");

function bunfigExcludes(): string[] {
  const toml = readFileSync(join(ROOT, "bunfig.toml"), "utf8");
  const block = toml.match(/minimumReleaseAgeExcludes\s*=\s*\[([\s\S]*?)\]/);
  if (!block?.[1]) throw new Error("minimumReleaseAgeExcludes not found in bunfig.toml");
  return [...block[1].matchAll(/"([^"]+)"/g)].map((m) => m[1] as string);
}

function lockAztecNames(): string[] {
  const lock = readFileSync(join(ROOT, "bun.lock"), "utf8");
  // Resolved entries look like "@aztec/<name>@<version>" (in keys and value tuples alike).
  // [^"@]+ stops the name before the version's @; captures ending in "/" would be nested-path
  // keys, not package names — filter them defensively.
  const names = [...lock.matchAll(/"(@aztec\/[^"@]+)@/g)]
    .map((m) => m[1] as string)
    .filter((name) => !name.endsWith("/"));
  return [...new Set(names)].sort();
}

describe("bunfig minimumReleaseAgeExcludes", () => {
  const excludes = bunfigExcludes();
  const lockNames = lockAztecNames();

  test("lock parsing found the @aztec graph (pattern-rot guard)", () => {
    expect(lockNames.length).toBeGreaterThan(0);
  });

  test("only @aztec/-scoped names are exempted from the age gate", () => {
    const offScope = excludes.filter((name) => !name.startsWith("@aztec/"));
    expect(offScope).toEqual([]);
  });

  test("every resolved @aztec package is excluded (else a <7-day-old bump fails install)", () => {
    const missing = lockNames.filter((name) => !excludes.includes(name));
    expect(missing).toEqual([]);
  });

  test("no stale excludes: every entry still resolves in bun.lock", () => {
    const stale = excludes.filter((name) => !lockNames.includes(name));
    expect(stale).toEqual([]);
  });
});
