import { describe, expect, test } from "bun:test";
import fs from "node:fs";
import path from "node:path";

const REPO = path.resolve(import.meta.dir, "..");
const packageFiles = [
  "packages/accelerator/package.json",
  "packages/playground/package.json",
] as const;

describe("Playwright version contract", () => {
  test("every Playwright consumer uses one exact version", () => {
    const versions = packageFiles.map((file) => {
      const manifest = JSON.parse(fs.readFileSync(path.join(REPO, file), "utf8")) as {
        devDependencies?: Record<string, string>;
      };
      return manifest.devDependencies?.["@playwright/test"];
    });

    expect(versions.every(Boolean)).toBe(true);
    expect(new Set(versions).size).toBe(1);
    expect(versions[0]).toMatch(/^\d+\.\d+\.\d+$/);
  });

  test("the direct macOS browser install resolves from the playground workspace", () => {
    const workflow = fs.readFileSync(
      path.join(REPO, ".github/workflows/_e2e-packaged.yml"),
      "utf8",
    );
    expect(workflow).toMatch(
      /- name: Install Playwright browser\n\s+working-directory: packages\/playground\n\s+run: bunx playwright install chromium/,
    );
  });
});
