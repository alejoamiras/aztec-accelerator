/**
 * Update all @aztec/* version references across the repo.
 *
 * Usage: bun scripts/update-aztec-version.ts <version>
 * Example: bun scripts/update-aztec-version.ts 5.0.0-nightly.20260220
 */

const VERSION_PATTERN = /^\d+\.\d+\.\d+(-(?:nightly\.\d{8}|rc\.\d+|aztecnr-rc\.\d+))?$/;
const AZTEC_VERSION_PATTERN = /^\d+\.\d+\.\d+(-(?:nightly|spartan|devnet|aztecnr-rc|rc)[\w.-]*)?$/;

const PACKAGE_JSON_FILES = [
  "packages/sdk/package.json",
  "packages/playground/package.json",
];

/**
 * Companion packages that must stay in version-lockstep with @aztec/*: their generated
 * code carries undeclared runtime imports of @aztec/aztec.js resolved against OUR pins,
 * so version skew breaks at runtime, silently. Explicit allowlist — NOT a scope prefix —
 * so unrelated @aztec-foundation packages never get swept up.
 */
const LOCKSTEP_PACKAGES = new Set(["@aztec-foundation/aztec-standards"]);

export function isAztecManagedDep(key: string): boolean {
  return key.startsWith("@aztec/") || LOCKSTEP_PACKAGES.has(key);
}

export function validateVersion(version: string): boolean {
  return VERSION_PATTERN.test(version);
}

export function updatePackageJson(content: string, newVersion: string, skipPackages?: Set<string>): string {
  const pkg = JSON.parse(content);

  for (const section of ["dependencies", "devDependencies"] as const) {
    const deps = pkg[section];
    if (!deps) continue;
    for (const [key, value] of Object.entries(deps)) {
      if (isAztecManagedDep(key) && typeof value === "string" && AZTEC_VERSION_PATTERN.test(value)) {
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
        if (isAztecManagedDep(key) && typeof value === "string" && AZTEC_VERSION_PATTERN.test(value)) {
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

const CRS_FILE = "packages/playground/src/aztec.ts";

/** Bump CRS_CACHE_VERSION so returning playground visitors re-download the CRS if bb.js changed its format. */
async function updateCrsCacheVersion(version: string): Promise<boolean> {
  const original = await Bun.file(CRS_FILE).text();
  const updated = original.replace(/(const CRS_CACHE_VERSION\s*=\s*")[^"]*(")/, `$1${version}$2`);
  if (updated === original) return false;
  await Bun.write(CRS_FILE, updated);
  return true;
}

// F-008: the Windows bb.exe pin is NEVER auto-generated here. Auto-downloading the asset and writing its
// own hash is circular (a twice-downloaded asset is not independent evidence). A human adds a reviewed
// `manual-review` entry to WINDOWS_BB_CHECKSUMS (copy-bb.ts); the post-install `check-windows-bb-pin.ts`
// step reports whether the live bb.js version has a pin, and the Windows CI gate fails closed without one.

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
    // LOCKSTEP packages track @aztec/* releases from a DIFFERENT publisher — a skip here means
    // the app would run mixed versions (undeclared runtime imports of @aztec/aztec.js make that
    // lockstep a hard requirement). Loud, not fatal: nightlies stay unblocked, and the CI token
    // spec is the behavioral gate that catches a truly broken mix.
    const lockstepSkipped = [...skipPackages].filter((p) => LOCKSTEP_PACKAGES.has(p));
    if (lockstepSkipped.length > 0) {
      console.warn(
        `⚠️  LOCKSTEP PACKAGE(S) NOT PUBLISHED AT ${newVersion}: ${lockstepSkipped.join(", ")} — ` +
          `left at their previous version; the app will mix versions until they publish. ` +
          `Verify the CI token spec passes before trusting this bump.`,
      );
    }
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

  // Companion bumps an @aztec version change also requires (lessons from the 5.0.0-rc.2 bump):
  const crsBumped = await updateCrsCacheVersion(newVersion);
  if (crsBumped) console.log(`Bumped CRS_CACHE_VERSION → ${newVersion} in ${CRS_FILE}.`);
  // The Windows bb.exe pin is intentionally NOT touched here (F-008) — see check-windows-bb-pin.ts.

  if (updatedFiles === 0 && !crsBumped) {
    console.log("\nAll files already at target version. No changes needed.");
  } else {
    console.log(`\nDone. Updated ${updatedFiles} package.json file(s) to ${newVersion}.`);
  }

  console.log("\nNext steps:");
  console.log(
    "  1. bun install   (add --minimum-release-age=0 ONLY if the version is <7 days old — local lockfile regen, never CI)",
  );
  console.log(
    "  2. bun run --cwd packages/playground typecheck:scripts   (catch @aztec API breaks in the deploy/fund scripts)",
  );
  console.log(
    "  3. ⚠️  Aztec artifacts may have recompiled → the salt=0 SponsoredFPC address can MOVE. Derive + redeploy on testnet if so:",
  );
  console.log(
    "       bun run packages/playground/scripts/deploy-sponsored-fpc.ts --salt 0x0   (--salt 0x0 mandatory; the script defaults to random)",
  );
}

if (import.meta.main) {
  main().catch((err) => {
    console.error(err.message);
    process.exit(1);
  });
}
