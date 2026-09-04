import {
  fetchAndVerifySdkProvenance,
  LEGACY_SDK_RELEASE_WORKFLOW,
  SDK_PACKAGE,
  SDK_RELEASE_WORKFLOW,
  SDK_VERSION_PATTERN,
} from "./sdk-release-verification.ts";
import { verifySdkPackageSignatures } from "./verify-sdk-package-signatures.ts";

function run(command: string[], options: { inherit?: boolean } = {}): string {
  const result = Bun.spawnSync(command, {
    stdin: options.inherit ? "inherit" : undefined,
    stdout: options.inherit ? "inherit" : "pipe",
    stderr: options.inherit ? "inherit" : "pipe",
  });
  if (result.exitCode !== 0) {
    const detail = options.inherit ? "" : `: ${result.stderr?.toString().trim() ?? ""}`;
    throw new Error(`${command.join(" ")} failed (${result.exitCode})${detail}`);
  }
  return options.inherit ? "" : (result.stdout?.toString().trim() ?? "");
}

function npmJson(field: string): unknown {
  return JSON.parse(run(["npm", "view", SDK_PACKAGE, field, "--json"]));
}

export function isActiveWorkflowStatus(status: string): boolean {
  return ["queued", "in_progress", "pending", "requested", "waiting"].includes(status);
}

export function assertFreshPromotionState(
  initial: Record<string, string>,
  current: Record<string, string>,
  version: string,
  rollback: boolean,
): void {
  if (current.latest !== initial.latest) {
    throw new Error(
      `npm latest changed during preflight (${initial.latest ?? "unset"} -> ${current.latest ?? "unset"})`,
    );
  }
  if (!rollback && current.testnet !== version) {
    throw new Error(`npm testnet changed during preflight and now points to ${current.testnet ?? "nothing"}`);
  }
}

function activeReleaseRuns(): Array<{ status: string; url: string }> {
  const runs = JSON.parse(
    run([
      "gh",
      "run",
      "list",
      "--workflow",
      "release-sdk.yml",
      "--limit",
      "100",
      "--json",
      "status,url",
    ]),
  ) as Array<{ status: string; url: string }>;
  return runs.filter((item) => isActiveWorkflowStatus(item.status));
}

function assertNoActiveReleaseRuns(): void {
  const activeRuns = activeReleaseRuns();
  if (activeRuns.length) {
    throw new Error(`SDK release workflow is active: ${activeRuns.map((item) => item.url).join(", ")}`);
  }
}

async function fetchUncachedDistTags(): Promise<Record<string, string>> {
  const response = await fetch(
    `https://registry.npmjs.org/@alejoamiras%2faztec-accelerator?cache_bust=${Date.now()}`,
    {
      headers: {
        accept: "application/json",
        "cache-control": "no-cache",
      },
    },
  );
  if (!response.ok) throw new Error(`registry read-back failed: HTTP ${response.status}`);
  const packument = (await response.json()) as { "dist-tags"?: Record<string, string> };
  if (!packument["dist-tags"]) throw new Error("registry read-back has no dist-tags");
  return packument["dist-tags"];
}

export function resolveRemoteTagCommit(output: string): string | undefined {
  const lines = output
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((line) => line.split(/\s+/, 2) as [string, string]);
  return lines.find(([, ref]) => ref.endsWith("^{}"))?.[0] ?? lines[0]?.[0];
}

async function main() {
  const args = process.argv.slice(2);
  const dryRun = args.includes("--dry-run");
  const rollback = args.includes("--rollback");
  const positional = args.filter((arg) => arg !== "--dry-run" && arg !== "--rollback");
  const version = positional[0];
  if (!version || positional.length !== 1 || !SDK_VERSION_PATTERN.test(version)) {
    throw new Error(
      "usage: bun run sdk:promote -- <X.Y.Z|X.Y.Z-revision.N> [--dry-run] [--rollback]",
    );
  }

  assertNoActiveReleaseRuns();

  const published = run(["npm", "view", `${SDK_PACKAGE}@${version}`, "version"]);
  if (published !== version) throw new Error(`npm returned unexpected version ${published}`);

  const tags = npmJson("dist-tags") as Record<string, string>;
  if (!rollback && tags.testnet !== version) {
    throw new Error(`npm testnet points to ${tags.testnet ?? "nothing"}, not ${version}`);
  }
  if (rollback) {
    if (!tags.latest) throw new Error("npm latest is unset; there is nothing to roll back");
    if (Bun.semver.order(version, tags.latest) !== -1) {
      throw new Error(`rollback target ${version} must be lower than current latest ${tags.latest}`);
    }
  }

  const provenance = await fetchAndVerifySdkProvenance(
    version,
    undefined,
    rollback ? [SDK_RELEASE_WORKFLOW, LEGACY_SDK_RELEASE_WORKFLOW] : undefined,
  );
  await verifySdkPackageSignatures(version);
  const gitTag = `${SDK_PACKAGE}@${version}`;
  const remoteRefs = run([
    "git",
    "ls-remote",
    "--tags",
    "origin",
    `refs/tags/${gitTag}`,
    `refs/tags/${gitTag}^{}`,
  ]);
  const tagCommit = resolveRemoteTagCommit(remoteRefs);
  if (!tagCommit) throw new Error(`remote git tag ${gitTag} does not exist`);
  if (tagCommit !== provenance.commit) {
    throw new Error(`remote tag commit ${tagCommit} does not match provenance ${provenance.commit}`);
  }

  const release = JSON.parse(
    run(["gh", "release", "view", gitTag, "--json", "tagName,url"]),
  ) as { tagName: string; url: string };
  if (release.tagName !== gitTag) throw new Error(`GitHub release has unexpected tag ${release.tagName}`);

  const previousLatest = tags.latest;
  console.log(`Candidate: ${SDK_PACKAGE}@${version}`);
  console.log(`Provenance commit: ${provenance.commit}`);
  console.log(`GitHub release: ${release.url}`);
  console.log(`Current latest: ${previousLatest ?? "unset"}`);
  console.log(`Operation: ${rollback ? "rollback" : "candidate promotion"}`);
  if (dryRun) {
    console.log("Dry run complete; npm latest was not changed.");
    return;
  }

  const verb = rollback ? "Roll back" : "Move";
  const answer = prompt(`${verb} npm latest from ${previousLatest ?? "unset"} to ${version}? [y/N]`);
  if (answer?.trim().toLowerCase() !== "y") {
    console.log("Promotion cancelled.");
    return;
  }

  // npm dist-tags have no compare-and-swap API. Narrow the solo-maintainer race window by
  // repeating both external-state checks immediately before the only mutation.
  assertNoActiveReleaseRuns();
  assertFreshPromotionState(tags, await fetchUncachedDistTags(), version, rollback);

  run(["npm", "dist-tag", "add", `${SDK_PACKAGE}@${version}`, "latest"], { inherit: true });
  let verifiedLatest: string | undefined;
  let verificationError: string | undefined;
  for (let attempt = 1; attempt <= 10; attempt++) {
    try {
      verifiedLatest = (await fetchUncachedDistTags()).latest;
      if (verifiedLatest === version) break;
      verificationError = `registry still reports ${verifiedLatest ?? "unset"}`;
    } catch (error) {
      verificationError = error instanceof Error ? error.message : String(error);
    }
    if (attempt < 10) {
      console.log(`Waiting for uncached npm read-back (${attempt}/10): ${verificationError}`);
      await Bun.sleep(3000);
    }
  }
  if (verifiedLatest !== version) {
    if (previousLatest) {
      console.error(
        `Verification failed. Review npm state, then roll back with:\n  npm dist-tag add ${SDK_PACKAGE}@${previousLatest} latest`,
      );
    }
    throw new Error(
      `npm latest is ${verifiedLatest ?? "unset"}, expected ${version} (${verificationError ?? "read-back failed"})`,
    );
  }
  console.log(`Verified npm latest -> ${version}`);
}

if (import.meta.main) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  });
}
