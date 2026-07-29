/**
 * Binary/bundle identity drift guards (`bun test scripts/`). The executable renamed
 * `aztec-accelerator` → `AztecAccelerator` at the CARGO layer ([[bin]] name + default-run) so that
 * plain `cargo build` (the webdriver E2E legs, F-012) and tauri-driven builds produce ONE name —
 * while productName, identifier, config dir, and artifact filenames stayed put. A half-rename
 * (someone touching one layer, one workflow, or re-adding the conf keys that would fork the name)
 * must fail CI in milliseconds on every platform, not on a Windows E2E leg an hour later.
 */
import { describe, expect, test } from "bun:test";
import fs from "node:fs";
import path from "node:path";

const PKG = path.resolve(import.meta.dir, "..");
const REPO = path.resolve(PKG, "..", "..");
const SRC_TAURI = path.join(PKG, "src-tauri");

const BIN = "AztecAccelerator";
const conf = JSON.parse(fs.readFileSync(path.join(SRC_TAURI, "tauri.conf.json"), "utf8"));
const cargo = fs.readFileSync(path.join(SRC_TAURI, "Cargo.toml"), "utf8");

describe("binary identity (cargo layer owns the name)", () => {
  test("Cargo [[bin]] + default-run are the renamed binary; crate name is NOT renamed", () => {
    expect(cargo).toContain(`name = "${BIN}"`);
    expect(cargo).toContain(`default-run = "${BIN}"`);
    // Crate name anchors Cargo.lock and the release version-sed — it must stay.
    expect(cargo).toContain('name = "aztec-accelerator"');
  });

  test("tauri.conf.json does NOT set mainBinaryName (it would re-fork plain-cargo vs tauri builds)", () => {
    expect(conf.mainBinaryName).toBeUndefined();
  });

  test("product identity unchanged (install dir, artifacts, config dir all hang off these)", () => {
    expect(conf.productName).toBe("Aztec Accelerator");
    expect(conf.identifier).toBe("dev.aztec.accelerator");
  });
});

describe("bundle metadata", () => {
  test("shipped metadata is pinned exactly", () => {
    expect(conf.bundle.category).toBe("DeveloperTool");
    expect(conf.bundle.copyright).toBe("© 2026 Aztec Accelerator contributors");
    expect(conf.bundle.homepage).toBe("https://aztec-accelerator.dev");
    expect(conf.bundle.license).toBe("AGPL-3.0-only");
    expect(conf.bundle.licenseFile).toBe("../../../LICENSE");
    expect(fs.existsSync(path.join(SRC_TAURI, conf.bundle.licenseFile))).toBe(true);
  });

  test("publisher stays ABSENT until it ships with a registry-migration story", () => {
    // publisher feeds NSIS ${MANUFACTURER}, whose registry namespace anchors custom-$INSTDIR
    // restore — changing it silently strands custom-directory installs (binary-rename plan,
    // audit fold #1). Deleting this assertion requires that migration, not just the config key.
    expect(conf.bundle.publisher).toBeUndefined();
  });
});

describe("CI/scripts reference the renamed binary (lockstep sites)", () => {
  const expectContains = (rel: string, needle: string) => {
    const body = fs.readFileSync(path.join(REPO, rel), "utf8");
    expect(body).toContain(needle);
  };

  test("webdriver APP_CMD paths", () => {
    expectContains(".github/workflows/_e2e-webdriver.yml", `target/debug/${BIN}$EXE`);
    expectContains(".github/workflows/_e2e-webdriver.yml", `target/release/${BIN}$EXE`);
    const wd = fs.readFileSync(path.join(REPO, ".github/workflows/_e2e-webdriver.yml"), "utf8");
    expect(wd).not.toContain("target/debug/aztec-accelerator");
    expect(wd).not.toContain("target/release/aztec-accelerator");
  });

  test("windows heal-scenario lookup, sentinel, bundle-shape invariant, smoke constant", () => {
    expectContains(".github/workflows/accelerator.yml", `"${BIN}.exe"`);
    expectContains(".github/workflows/smoke-updater-windows.yml", `$INSTDIR\\${BIN}.exe`);
    expectContains(".github/workflows/release-accelerator.yml", `${BIN}\\nbb`);
    expectContains(
      "packages/accelerator/scripts/updater-smoke-windows.ps1",
      `$NBinaryName = "${BIN}.exe"`,
    );
  });

  test("the ONLY deliberate old-name literal in workflows is the v1.0.7 fixture argument", () => {
    // The call-path N-1 is the pre-rename v1.0.7 until the fixture moves; _e2e-updater-windows.yml
    // passes -N1BinaryName "aztec-accelerator.exe" for it. Nothing else may reference the old exe.
    const body = fs.readFileSync(
      path.join(REPO, ".github/workflows/_e2e-updater-windows.yml"),
      "utf8",
    );
    expect(body).toContain('-N1BinaryName "aztec-accelerator.exe"');
  });
});
