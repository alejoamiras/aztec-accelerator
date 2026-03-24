/**
 * Update all @aztec/* version references across the repo.
 *
 * Usage: bun scripts/update-aztec-version.ts <version>
 * Example: bun scripts/update-aztec-version.ts 5.0.0-nightly.20260220
 */

const VERSION_PATTERN = /^\d+\.\d+\.\d+(-(?:nightly\.\d{8}|rc\.\d+))?$/;
const AZTEC_VERSION_PATTERN = /^\d+\.\d+\.\d+(-(?:nightly|spartan|devnet|rc)[\w.-]*)?$/;

const PACKAGE_JSON_FILES = [
  "packages/sdk/package.json",
  "packages/playground/package.json",
];

export function validateVersion(version: string): boolean {
  return VERSION_PATTERN.test(version);
}

export function updatePackageJson(content: string, newVersion: string, skipPackages?: Set<string>): string {
  const pkg = JSON.parse(content);

  for (const section of ["dependencies", "devDependencies"] as const) {
    const deps = pkg[section];
    if (!deps) continue;
    for (const [key, value] of Object.entries(deps)) {
      if (key.startsWith("@aztec/") && typeof value === "string" && AZTEC_VERSION_PATTERN.test(value)) {
        if (skipPackages?.has(key)) continue;
        deps[key] = newVersion;
      }
    }
  }

  return `${JSON.stringify(pkg, null, 2)}\n`;
}

async function findMissingPackages(version: string, packageFiles: string[]): Promise<Set<string>> {
  const allAztecPackages = new Set<string>();
  for (const filePath of packageFiles) {
    const pkg = await Bun.file(filePath).json();
    for (const section of ["dependencies", "devDependencies"] as const) {
      const deps = pkg[section];
      if (!deps) continue;
      for (const [key, value] of Object.entries(deps)) {
        if (key.startsWith("@aztec/") && typeof value === "string" && AZTEC_VERSION_PATTERN.test(value)) {
          allAztecPackages.add(key);
        }
      }
    }
  }

  const missing = new Set<string>();
  await Promise.all(
    [...allAztecPackages].map(async (pkg) => {
      const proc = Bun.spawn(["npm", "view", `${pkg}@${version}`, "version", "--json"], {
        stdout: "pipe",
        stderr: "pipe",
      });
      const exitCode = await proc.exited;
      if (exitCode !== 0) missing.add(pkg);
    }),
  );

  return missing;
}

async function main() {
  const newVersion = process.argv[2];

  if (!newVersion) {
    console.error("Usage: bun scripts/update-aztec-version.ts <version>");
    console.error("Example: bun scripts/update-aztec-version.ts 5.0.0-nightly.20260220");
    process.exit(1);
  }

  if (!validateVersion(newVersion)) {
    console.error(
      `Invalid version format: "${newVersion}". Expected: X.Y.Z, X.Y.Z-nightly.YYYYMMDD, or X.Y.Z-rc.N`,
    );
    process.exit(1);
  }

  const skipPackages = await findMissingPackages(newVersion, PACKAGE_JSON_FILES);
  if (skipPackages.size > 0) {
    console.log(`Skipping unpublished packages: ${[...skipPackages].join(", ")}`);
  }

  let updatedFiles = 0;

  for (const filePath of PACKAGE_JSON_FILES) {
    const file = Bun.file(filePath);
    const original = await file.text();
    const updated = updatePackageJson(original, newVersion, skipPackages);
    if (updated !== original) {
      await Bun.write(filePath, updated);
      console.log(`Updated ${filePath}`);
      updatedFiles++;
    }
  }

  if (updatedFiles === 0) {
    console.log("All files already at target version. No changes needed.");
  } else {
    console.log(`\nDone. Updated ${updatedFiles} file(s) to ${newVersion}.`);
    console.log("Run 'bun install' to regenerate the lockfile.");
  }
}

if (import.meta.main) {
  main().catch((err) => {
    console.error(err.message);
    process.exit(1);
  });
}
