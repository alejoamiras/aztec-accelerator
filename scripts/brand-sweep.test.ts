/**
 * Rebrand sweep, two directions:
 *  1. Retired visual values (old palette hexes, old font families) must be GONE from every
 *     product surface — a token swap that misses a hardcoded literal fails here, not in review.
 *  2. Frozen operational identity must be byte-INTACT: the visual rebrand may never leak
 *     "Presto" into identity-bearing files, and every frozen literal must still exist at its site.
 * Patterns are word-boundary/quoted on purpose: bare `Inter` matches IntersectionObserver.
 */
import { describe, expect, test } from "bun:test";
import { readdirSync } from "node:fs";
import { join, resolve } from "node:path";

const ROOT = resolve(import.meta.dir, "..");

const PRODUCT_SURFACES = [
  "packages/accelerator/src-tauri/frontend/style.css",
  "packages/accelerator/src-tauri/frontend/onboarding.css",
  "packages/accelerator/src-tauri/frontend/renewal.css",
  "packages/accelerator/src-tauri/frontend/onboarding.html",
  "packages/accelerator/src-tauri/frontend/settings.html",
  "packages/accelerator/src-tauri/frontend/authorize.html",
  "packages/accelerator/src-tauri/frontend/update-prompt.html",
  "packages/accelerator/src-tauri/frontend/renewal.html",
  "packages/landing/index.html",
  "packages/landing/src/style.css",
  "packages/landing/src/main.ts",
  "packages/landing/src/race.ts",
  "packages/playground/index.html",
  "packages/playground/src/style.css",
  "packages/playground/src/main.ts",
  "packages/playground/src/ui.ts",
  "packages/playground/src/results.ts",
  "packages/playground/src/spark-orbit.ts",
];

const RETIRED = [
  "d4ff28",
  "b8dd1e",
  "0b0a06",
  "131008",
  "1a1610",
  "2a2518",
  "f0e8d0",
  "9a9080",
  "0e1400",
  "rgba(212, 255, 40",
  '"Space Grotesk"',
  "Space+Grotesk",
  '"Inter"',
  "family=Inter",
  '"JetBrains Mono"',
  "JetBrains+Mono",
];

/** file → literals that must remain byte-identical (the operational freeze). */
const FROZEN_PRESENT: Record<string, string[]> = {
  "packages/accelerator/src-tauri/tauri.conf.json": [
    '"productName": "Aztec Accelerator"',
    '"identifier": "dev.aztec.accelerator"',
    '"publisher": "Aztec Accelerator"',
    "https://aztec-accelerator.dev/releases/latest.json",
    '"copyright": "© 2026 Aztec Accelerator contributors"',
    '"homepage": "https://aztec-accelerator.dev"',
  ],
  "packages/accelerator/src-tauri/Cargo.toml": ['name = "aztec-accelerator"', "AztecAccelerator"],
  "packages/accelerator/src-tauri/src/certs.rs": ["Aztec Accelerator Local CA", ".aztec-accelerator"],
  "packages/accelerator/src-tauri/src/trust/linux.rs": ["aztec-accelerator-ca-"],
  "packages/accelerator/src-tauri/src/trust/windows.rs": ["Aztec Accelerator Local CA"],
  "packages/accelerator/src-tauri/src/autostart.rs": ['"Aztec Accelerator"'],
  "packages/accelerator/src-tauri/src/crash_recovery.rs": ["Aztec Accelerator Crash Recovery"],
  "packages/accelerator/src-tauri/src/update_marker.rs": ["AztecAccelerator", '"Aztec Accelerator"'],
  "packages/accelerator/src-tauri/nsis/hooks.nsi": ["Aztec Accelerator Local CA"],
  "packages/sdk/package.json": ["@alejoamiras/aztec-accelerator"],
  ".github/workflows/release-accelerator.yml": ["Aztec-Accelerator-"],
};

/** Identity-bearing files that must never mention the visual brand. */
const FROZEN_NO_PRESTO = [
  "packages/accelerator/src-tauri/tauri.conf.json",
  "packages/accelerator/src-tauri/Cargo.toml",
  "packages/accelerator/src-tauri/src/certs.rs",
  "packages/accelerator/src-tauri/src/autostart.rs",
  "packages/accelerator/src-tauri/src/crash_recovery.rs",
  "packages/accelerator/src-tauri/src/update_marker.rs",
  "packages/accelerator/src-tauri/src/updater.rs",
  "packages/accelerator/src-tauri/src/uninstall.rs",
  "packages/accelerator/src-tauri/nsis/hooks.nsi",
];

describe("brand sweep", () => {
  test("retired visual values are gone from every product surface", async () => {
    const hits: string[] = [];
    for (const rel of PRODUCT_SURFACES) {
      const text = await Bun.file(join(ROOT, rel)).text();
      for (const pattern of RETIRED) {
        if (text.toLowerCase().includes(pattern.toLowerCase())) hits.push(`${rel}: ${pattern}`);
      }
    }
    expect(hits).toEqual([]);
  });

  test("frozen identity literals are intact at their sites", async () => {
    const missing: string[] = [];
    for (const [rel, literals] of Object.entries(FROZEN_PRESENT)) {
      const text = await Bun.file(join(ROOT, rel)).text();
      for (const lit of literals) {
        if (!text.includes(lit)) missing.push(`${rel}: ${lit}`);
      }
    }
    expect(missing).toEqual([]);
  });

  test("identity-bearing files never mention Presto", async () => {
    const leaks: string[] = [];
    for (const rel of FROZEN_NO_PRESTO) {
      const text = await Bun.file(join(ROOT, rel)).text();
      if (/presto/i.test(text)) leaks.push(rel);
    }
    // Every trust backend too, not just the enumerated ones.
    const trustDir = join(ROOT, "packages/accelerator/src-tauri/src/trust");
    for (const f of readdirSync(trustDir).filter((f) => f.endsWith(".rs"))) {
      const text = await Bun.file(join(trustDir, f)).text();
      if (/presto/i.test(text)) leaks.push(`trust/${f}`);
    }
    expect(leaks).toEqual([]);
  });

  test("all CI workflows are untouched by the rebrand", async () => {
    const wfDir = join(ROOT, ".github", "workflows");
    const leaks: string[] = [];
    for (const f of readdirSync(wfDir).filter((f) => f.endsWith(".yml"))) {
      const text = await Bun.file(join(wfDir, f)).text();
      if (/presto/i.test(text)) leaks.push(f);
    }
    expect(leaks).toEqual([]);
  });
});
