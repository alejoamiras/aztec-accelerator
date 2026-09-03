import { describe, expect, test } from "bun:test";
import fs from "node:fs";
import path from "node:path";

const REPO = path.resolve(import.meta.dir, "..", "..", "..");
const FILTER_PATH = path.join(REPO, ".github/filters/accelerator.yml");
const WORKFLOW_PATH = path.join(REPO, ".github/workflows/accelerator.yml");
const filters = Bun.YAML.parse(fs.readFileSync(FILTER_PATH, "utf8")) as Record<string, string[]>;
const workflow = fs.readFileSync(WORKFLOW_PATH, "utf8");

const groupsFor = (...files: string[]) =>
  Object.entries(filters)
    .filter(([, patterns]) =>
      files.some((file) => patterns.some((pattern) => new Bun.Glob(pattern).match(file))),
    )
    .map(([group]) => group)
    .sort();

describe("accelerator CI path routing", () => {
  const cases: Array<[string, string[], string[]]> = [
    ["release runbook", ["docs/RELEASE_RUNBOOK.md"], ["release_tooling"]],
    [
      "release workflow",
      [".github/workflows/release-accelerator.yml"],
      ["release_tooling", "updater_feed"],
    ],
    [
      "updater resolver",
      ["packages/accelerator/scripts/resolve-updater-baseline.ts"],
      ["release_tooling", "updater_feed"],
    ],
    [
      "post-rotation cleanup change set",
      [
        ".github/workflows/release-accelerator.yml",
        ".github/workflows/_e2e-updater.yml",
        ".github/workflows/_e2e-updater-linux.yml",
        ".github/workflows/_e2e-updater-windows.yml",
        "CLAUDE.md",
        "docs/RELEASE_RUNBOOK.md",
        "packages/accelerator/README.md",
        "packages/accelerator/scripts/updater-smoke.sh",
      ],
      ["release_tooling", "updater_feed"],
    ],
    [
      "desktop Rust runtime",
      ["packages/accelerator/src-tauri/src/main.rs"],
      ["desktop_runtime", "release_tooling", "rust_platform", "windows_packaging"],
    ],
    [
      "desktop WebDriver spec",
      ["packages/accelerator/e2e-webdriver/smoke.spec.ts"],
      ["desktop_runtime", "release_tooling"],
    ],
    [
      "headless server",
      ["packages/accelerator/server/src/main.rs"],
      ["headless_server", "release_tooling", "sdk_integration"],
    ],
    ["SDK source", ["packages/sdk/src/index.ts"], ["sdk_integration"]],
    [
      "Windows bb downloader",
      ["scripts/download-bb.ts"],
      ["desktop_runtime", "release_tooling", "sdk_integration", "windows_bb", "windows_packaging"],
    ],
    ["unrelated documentation", ["docs/PLATFORM_SUPPORT.md"], []],
    ["unrelated workflow", [".github/workflows/deploy-landing.yml"], []],
    [
      "routing contract",
      [".github/filters/accelerator.yml"],
      [
        "desktop_runtime",
        "headless_server",
        "release_tooling",
        "rust_platform",
        "sdk_integration",
        "updater_feed",
        "windows_bb",
        "windows_packaging",
      ],
    ],
  ];

  for (const [name, files, expected] of cases) {
    test(name, () => expect(groupsFor(...files)).toEqual(expected.sort()));
  }

  test("the workflow consumes the central filter and routes every job group", () => {
    expect(workflow).toContain("filters: .github/filters/accelerator.yml");
    for (const group of Object.keys(filters)) {
      expect(workflow).toContain(`needs.changes.outputs.${group} == 'true'`);
    }
  });

  test("PR Rust caches restore but only refs/heads/main may save", () => {
    const setup = fs.readFileSync(
      path.join(REPO, ".github/actions/setup-accelerator/action.yml"),
      "utf8",
    );
    const sdkE2e = fs.readFileSync(path.join(REPO, ".github/workflows/_e2e.yml"), "utf8");
    const saveOnMain = ["save-if: $", "{{ github.ref == 'refs/heads/main' }}"].join("");
    for (const source of [setup, sdkE2e]) {
      expect(source).toContain(saveOnMain);
    }
  });

  test("App LNA routing excludes arbitrary docs and workflows", () => {
    const app = fs.readFileSync(path.join(REPO, ".github/workflows/app.yml"), "utf8");
    const lna = app.split("            lna_relevant:")[1]?.split("      - name:")[0] ?? "";
    expect(lna).toContain("'packages/sdk/**'");
    expect(lna).toContain("'packages/playground/**'");
    expect(lna).toContain("'packages/landing/**'");
    expect(lna).toContain("'.github/actions/playwright-cache/**'");
    expect(lna).toContain("'.github/workflows/app.yml'");
    expect(lna).not.toContain("'docs/");
    expect(lna).not.toContain("'.github/workflows/**'");
  });

  test("dependency audit filters only PRs and keeps non-PR entry points", () => {
    const audit = fs.readFileSync(
      path.join(REPO, ".github/workflows/dependency-audit.yml"),
      "utf8",
    );
    expect(audit).toContain("  workflow_call:\n  workflow_dispatch:\n  pull_request:");
    expect(audit).toContain('    - cron: "23 9 * * 1"');
    for (const dependencyPath of [
      "'**/package.json'",
      "'bun.lock'",
      "'**/Cargo.toml'",
      "'**/Cargo.lock'",
      "'scripts/dependency-audit.ts'",
      "'scripts/dependency-audit.test.ts'",
      "'scripts/dependency-audit-allowlist.json'",
      "'.github/workflows/dependency-audit.yml'",
    ]) {
      expect(audit).toContain(dependencyPath);
    }
  });
});
