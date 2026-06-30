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

const CRS_FILE = "packages/playground/src/aztec.ts";
const COPY_BB_FILE = "packages/accelerator/scripts/copy-bb.ts";

/** Bump CRS_CACHE_VERSION so returning playground visitors re-download the CRS if bb.js changed its format. */
async function updateCrsCacheVersion(version: string): Promise<boolean> {
  const original = await Bun.file(CRS_FILE).text();
  const updated = original.replace(/(const CRS_CACHE_VERSION\s*=\s*")[^"]*(")/, `$1${version}$2`);
  if (updated === original) return false;
  await Bun.write(CRS_FILE, updated);
  return true;
}

/** Fetch + pin the Windows bb.exe SHA-256 (the Windows Prebuild/Build Smoke gates verify it). Best-effort. */
async function pinWindowsBbChecksum(version: string): Promise<string> {
  const original = await Bun.file(COPY_BB_FILE).text();
  if (original.includes(`"${version}":`)) return `Windows bb checksum: already pinned for ${version}.`;
  const url = `https://github.com/AztecProtocol/aztec-packages/releases/download/v${version}/barretenberg-amd64-windows.tar.gz`;
  try {
    const res = await fetch(url);
    if (!res.ok) return `Windows bb checksum: fetch failed (HTTP ${res.status}) — pin "${version}" in ${COPY_BB_FILE} manually.`;
    const digest = await crypto.subtle.digest("SHA-256", await res.arrayBuffer());
    const sha = [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
    const marker = "};\n\nexport function resolveWindowsBbChecksum";
    const idx = original.indexOf(marker);
    const entry = `  // @aztec/bb.js ${version} — sha256 of barretenberg-amd64-windows.tar.gz from the v${version} release (auto-pinned).\n  "${version}": "${sha}",\n`;
    if (idx === -1) return `Windows bb checksum for ${version} = ${sha} — couldn't auto-insert; add it to ${COPY_BB_FILE} manually.`;
    await Bun.write(COPY_BB_FILE, original.slice(0, idx) + entry + original.slice(idx));
    return `Windows bb checksum: pinned ${version} = ${sha.slice(0, 12)}… in ${COPY_BB_FILE}.`;
  } catch (err) {
    return `Windows bb checksum: ${(err as Error).message} — pin "${version}" manually.`;
  }
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

  // Companion bumps an @aztec version change also requires (lessons from the 5.0.0-rc.2 bump):
  const crsBumped = await updateCrsCacheVersion(newVersion);
  if (crsBumped) console.log(`Bumped CRS_CACHE_VERSION → ${newVersion} in ${CRS_FILE}.`);
  console.log(await pinWindowsBbChecksum(newVersion));

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
