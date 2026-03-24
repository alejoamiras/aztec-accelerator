import { describe, expect, test } from "bun:test";
import { updatePackageJson, validateVersion } from "./update-aztec-version";

describe("validateVersion", () => {
  test("accepts nightly format", () => {
    expect(validateVersion("5.0.0-nightly.20260224")).toBe(true);
  });

  test("accepts rc format", () => {
    expect(validateVersion("4.1.0-rc.4")).toBe(true);
  });

  test("accepts stable semver", () => {
    expect(validateVersion("5.0.0")).toBe(true);
    expect(validateVersion("4.1.0")).toBe(true);
  });

  test("rejects invalid format", () => {
    expect(validateVersion("not-a-version")).toBe(false);
  });
});

describe("updatePackageJson", () => {
  const samplePkg = JSON.stringify(
    {
      name: "test",
      dependencies: {
        "@aztec/stdlib": "4.1.0-rc.4",
        "@aztec/bb-prover": "4.1.0-rc.4",
        "ky": "^1.14.3",
      },
      devDependencies: {
        "@aztec/simulator": "4.1.0-rc.4",
        "typescript": "^5.9.3",
      },
    },
    null,
    2,
  );

  test("updates all @aztec/* dependencies to new version", () => {
    const result = updatePackageJson(samplePkg, "5.0.0-nightly.20260224");
    const pkg = JSON.parse(result);
    expect(pkg.dependencies["@aztec/stdlib"]).toBe("5.0.0-nightly.20260224");
    expect(pkg.dependencies["@aztec/bb-prover"]).toBe("5.0.0-nightly.20260224");
    expect(pkg.devDependencies["@aztec/simulator"]).toBe("5.0.0-nightly.20260224");
  });

  test("does not touch non-@aztec dependencies", () => {
    const result = updatePackageJson(samplePkg, "5.0.0-nightly.20260224");
    const pkg = JSON.parse(result);
    expect(pkg.dependencies.ky).toBe("^1.14.3");
    expect(pkg.devDependencies.typescript).toBe("^5.9.3");
  });

  test("respects skipPackages set", () => {
    const skip = new Set(["@aztec/simulator"]);
    const result = updatePackageJson(samplePkg, "5.0.0-nightly.20260224", skip);
    const pkg = JSON.parse(result);
    expect(pkg.dependencies["@aztec/stdlib"]).toBe("5.0.0-nightly.20260224");
    expect(pkg.devDependencies["@aztec/simulator"]).toBe("4.1.0-rc.4");
  });

  test("updates to stable version", () => {
    const result = updatePackageJson(samplePkg, "4.1.0");
    const pkg = JSON.parse(result);
    expect(pkg.dependencies["@aztec/stdlib"]).toBe("4.1.0");
    expect(pkg.dependencies["@aztec/bb-prover"]).toBe("4.1.0");
    expect(pkg.devDependencies["@aztec/simulator"]).toBe("4.1.0");
  });

  test("updates from stable version to newer", () => {
    const stablePkg = JSON.stringify({
      name: "test",
      dependencies: { "@aztec/stdlib": "4.1.0" },
    }, null, 2);
    const result = updatePackageJson(stablePkg, "4.2.0-rc.1");
    const pkg = JSON.parse(result);
    expect(pkg.dependencies["@aztec/stdlib"]).toBe("4.2.0-rc.1");
  });
});
