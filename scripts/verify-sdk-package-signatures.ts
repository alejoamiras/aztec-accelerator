import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { SDK_PACKAGE, SDK_VERSION_PATTERN } from "./sdk-release-verification.ts";

interface SignatureAudit {
  verified?: Array<{
    name?: string;
    version?: string;
    attestations?: { provenance?: { predicateType?: string } };
  }>;
}

function run(command: string[], cwd: string): string {
  const result = Bun.spawnSync(command, { cwd, stdout: "pipe", stderr: "pipe" });
  if (result.exitCode !== 0) {
    throw new Error(
      `${command.join(" ")} failed (${result.exitCode}): ${result.stderr.toString().trim()}`,
    );
  }
  return result.stdout.toString();
}

export function hasVerifiedSdkProvenance(
  report: SignatureAudit,
  version: string,
): boolean {
  return Boolean(
    report.verified?.some(
      (item) =>
        item.name === SDK_PACKAGE &&
        item.version === version &&
        item.attestations?.provenance?.predicateType === "https://slsa.dev/provenance/v1",
    ),
  );
}

export async function verifySdkPackageSignatures(version: string): Promise<void> {
  if (!SDK_VERSION_PATTERN.test(version)) throw new Error(`invalid SDK version ${version}`);
  const directory = await mkdtemp(join(tmpdir(), "aztec-sdk-signature-audit-"));
  try {
    await Bun.write(
      join(directory, "package.json"),
      `${JSON.stringify({ private: true, dependencies: { [SDK_PACKAGE]: version } }, null, 2)}\n`,
    );
    run(
      ["npm", "install", "--ignore-scripts", "--no-audit", "--no-fund", "--loglevel=error"],
      directory,
    );
    const installed = run(
      [
        "node",
        "-p",
        "require('./node_modules/@alejoamiras/aztec-accelerator/package.json').version",
      ],
      directory,
    ).trim();
    if (installed !== version) throw new Error(`installed ${installed}, expected ${version}`);
    const report = JSON.parse(
      run(["npm", "audit", "signatures", "--json", "--include-attestations"], directory),
    ) as SignatureAudit;
    if (!hasVerifiedSdkProvenance(report, version)) {
      throw new Error(`npm did not cryptographically verify provenance for ${SDK_PACKAGE}@${version}`);
    }
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

if (import.meta.main) {
  const version = process.argv[2];
  if (!version) {
    console.error("usage: bun scripts/verify-sdk-package-signatures.ts <version>");
    process.exit(1);
  }
  await verifySdkPackageSignatures(version);
  console.log(`verified registry signatures and SLSA provenance for ${SDK_PACKAGE}@${version}`);
}
