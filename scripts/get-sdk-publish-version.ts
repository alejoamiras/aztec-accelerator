/**
 * Determine the SDK publish version, appending a revision suffix if the
 * base Aztec version has already been published to npm.
 *
 * Usage: bun scripts/get-sdk-publish-version.ts <base-version>
 *
 * Prerelease bases (contain "-") use dot-appended revisions, which extend
 * the existing prerelease identifier and remain valid semver:
 *   5.0.0-nightly.20260224      (if not yet published)
 *   5.0.0-nightly.20260224.1    (if base already exists)
 *   5.0.0-nightly.20260224.2    (if .1 already exists)
 *
 * Stable bases use a "-revision.N" suffix to produce valid semver
 * (a stable + "." + number is NOT valid semver, so we can't reuse the
 * dot-append trick):
 *   4.2.0                       (if not yet published)
 *   4.2.0-revision.1            (if 4.2.0 already exists)
 *   4.2.0-revision.2            (if -revision.1 already exists)
 */

const PACKAGE_NAME = "@alejoamiras/aztec-accelerator";

/**
 * Pure function: given a base version and the list of already-published
 * versions, return the version string to publish.
 */
export function resolvePublishVersion(
	baseVersion: string,
	publishedVersions: string[],
): string {
	if (!publishedVersions.includes(baseVersion)) {
		return baseVersion;
	}

	const isPrerelease = baseVersion.includes("-");
	const prefix = isPrerelease
		? `${baseVersion}.`
		: `${baseVersion}-revision.`;
	const revisions = publishedVersions
		.filter((v) => v.startsWith(prefix))
		.map((v) => Number(v.slice(prefix.length)))
		.filter((n) => Number.isInteger(n) && n > 0);

	const maxRevision = revisions.length > 0 ? Math.max(...revisions) : 0;
	const nextRevision = maxRevision + 1;
	return isPrerelease
		? `${baseVersion}.${nextRevision}`
		: `${baseVersion}-revision.${nextRevision}`;
}

async function getPublishedVersions(): Promise<string[]> {
	const proc = Bun.spawn(
		["npm", "view", PACKAGE_NAME, "versions", "--json"],
		{ stdout: "pipe", stderr: "pipe" },
	);
	const exitCode = await proc.exited;

	if (exitCode !== 0) {
		const stderr = await new Response(proc.stderr).text();
		if (stderr.includes("E404")) {
			return [];
		}
		throw new Error(`npm view failed (exit ${exitCode}): ${stderr}`);
	}

	const stdout = await new Response(proc.stdout).text();
	const parsed = JSON.parse(stdout);
	return Array.isArray(parsed) ? parsed : [parsed];
}

async function main() {
	const baseVersion = process.argv[2];
	if (!baseVersion) {
		console.error(
			"Usage: bun scripts/get-sdk-publish-version.ts <base-version>",
		);
		process.exit(1);
	}

	const versions = await getPublishedVersions();
	const publishVersion = resolvePublishVersion(baseVersion, versions);
	console.log(publishVersion);
}

if (import.meta.main) {
	main().catch((err) => {
		console.error(err.message);
		process.exit(1);
	});
}
