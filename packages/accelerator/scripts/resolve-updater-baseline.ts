export interface UpdaterReleaseCandidate {
  tagName: string;
  draft: boolean;
  assetNames: string[];
  pubkey: string;
}

export interface UpdaterBaseline {
  tag: string;
  version: string;
  rotation: boolean;
}

interface SelectUpdaterBaselineOptions {
  version: string;
  currentPubkey: string;
  allowKeyRotationBootstrap: boolean;
  releases: UpdaterReleaseCandidate[];
}

interface GitHubRelease {
  tag_name?: unknown;
  draft?: unknown;
  assets?: Array<{ name?: unknown }>;
}

interface GitHubContent {
  content?: unknown;
  encoding?: unknown;
}

type FetchLike = (input: string | URL | Request, init?: RequestInit) => Promise<Response>;

const TAG_PREFIX = "accelerator-v";

function versionFromTag(tagName: string): string | undefined {
  if (!tagName.startsWith(TAG_PREFIX)) return undefined;
  const version = tagName.slice(TAG_PREFIX.length);
  try {
    Bun.semver.order(version, version);
    return version;
  } catch {
    return undefined;
  }
}

function hasCompleteInstallerSet(candidate: UpdaterReleaseCandidate, version: string): boolean {
  const required = [
    `Aztec-Accelerator-${version}-macOS-Apple-Silicon.dmg`,
    `Aztec-Accelerator-${version}-macOS-Intel.dmg`,
    `Aztec-Accelerator-${version}-Linux-x86_64.AppImage`,
    `Aztec-Accelerator-${version}-Windows-x86_64-setup.exe`,
  ];
  const names = new Set(candidate.assetNames);
  return required.every((name) => names.has(name));
}

export function selectUpdaterBaseline(options: SelectUpdaterBaselineOptions): UpdaterBaseline {
  const eligible = options.releases
    .flatMap((release) => {
      const version = versionFromTag(release.tagName);
      if (
        !version ||
        release.draft ||
        Bun.semver.order(version, options.version) !== -1 ||
        !hasCompleteInstallerSet(release, version)
      ) {
        return [];
      }
      return [{ release, version }];
    })
    .sort((a, b) => Bun.semver.order(b.version, a.version));

  const sameKey = eligible.find(({ release }) => release.pubkey === options.currentPubkey);
  if (sameKey) {
    if (options.allowKeyRotationBootstrap) {
      throw new Error(
        `same-key updater baseline already exists at ${sameKey.release.tagName}; refuse key-rotation bootstrap`,
      );
    }
    return {
      tag: sameKey.release.tagName,
      version: sameKey.version,
      rotation: false,
    };
  }

  if (!options.allowKeyRotationBootstrap) {
    throw new Error(
      "no lower release uses the current updater key; the first release after an intentional rotation requires --allow-key-rotation-bootstrap",
    );
  }

  const priorKey = eligible[0];
  if (!priorKey)
    throw new Error("no complete lower accelerator release is available as a rotation witness");

  const firstMajorRc = /^(\d+)\.0\.0-rc\.1$/.exec(options.version);
  const priorMajor = /^(\d+)\./.exec(priorKey.version);
  if (!firstMajorRc || !priorMajor || Number(firstMajorRc[1]) !== Number(priorMajor[1]) + 1) {
    throw new Error(
      `key-rotation bootstrap is restricted to rc.1 of the next major after ${priorKey.version}`,
    );
  }

  return {
    tag: priorKey.release.tagName,
    version: priorKey.version,
    rotation: true,
  };
}

function githubHeaders(token: string): HeadersInit {
  return {
    Accept: "application/vnd.github+json",
    Authorization: `Bearer ${token}`,
    "X-GitHub-Api-Version": "2022-11-28",
  };
}

export async function loadUpdaterReleaseCandidates(
  repository: string,
  token: string,
  fetchImpl: FetchLike = fetch,
): Promise<UpdaterReleaseCandidate[]> {
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) {
    throw new Error(`invalid GitHub repository: ${repository}`);
  }

  const releasesResponse = await fetchImpl(
    `https://api.github.com/repos/${repository}/releases?per_page=100`,
    { headers: githubHeaders(token) },
  );
  if (!releasesResponse.ok) {
    throw new Error(`GitHub releases query failed with HTTP ${releasesResponse.status}`);
  }
  const releases = (await releasesResponse.json()) as GitHubRelease[];
  if (!Array.isArray(releases)) throw new Error("GitHub releases response was not an array");

  const candidates: UpdaterReleaseCandidate[] = [];
  for (const release of releases) {
    if (typeof release.tag_name !== "string" || typeof release.draft !== "boolean") continue;
    const version = versionFromTag(release.tag_name);
    if (!version) continue;

    const assetNames = (release.assets ?? []).flatMap((asset) =>
      typeof asset.name === "string" ? [asset.name] : [],
    );
    const shapeOnly: UpdaterReleaseCandidate = {
      tagName: release.tag_name,
      draft: release.draft,
      assetNames,
      pubkey: "",
    };
    if (release.draft || !hasCompleteInstallerSet(shapeOnly, version)) continue;

    const configResponse = await fetchImpl(
      `https://api.github.com/repos/${repository}/contents/packages/accelerator/src-tauri/tauri.conf.json?ref=${encodeURIComponent(release.tag_name)}`,
      { headers: githubHeaders(token) },
    );
    if (!configResponse.ok) {
      throw new Error(
        `failed to read updater configuration at ${release.tag_name}: HTTP ${configResponse.status}`,
      );
    }
    const content = (await configResponse.json()) as GitHubContent;
    if (content.encoding !== "base64" || typeof content.content !== "string") {
      throw new Error(`invalid updater configuration response at ${release.tag_name}`);
    }
    const config = JSON.parse(Buffer.from(content.content.replace(/\s/g, ""), "base64").toString());
    const pubkey = config?.plugins?.updater?.pubkey;
    if (typeof pubkey !== "string" || pubkey.length === 0) {
      throw new Error(`missing updater public key at ${release.tag_name}`);
    }
    candidates.push({ ...shapeOnly, pubkey });
  }
  return candidates;
}

function readArg(name: string): string {
  const index = Bun.argv.indexOf(name);
  const value = index >= 0 ? Bun.argv[index + 1] : undefined;
  if (!value) throw new Error(`missing ${name}`);
  return value;
}

if (import.meta.main) {
  try {
    const repository = readArg("--repository");
    const version = readArg("--version");
    const currentPubkey = readArg("--pubkey");
    const token = process.env.GITHUB_TOKEN;
    if (!token) throw new Error("GITHUB_TOKEN is required");
    const releases = await loadUpdaterReleaseCandidates(repository, token);
    const result = selectUpdaterBaseline({
      version,
      currentPubkey,
      allowKeyRotationBootstrap: Bun.argv.includes("--allow-key-rotation-bootstrap"),
      releases,
    });
    console.log(JSON.stringify(result));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  }
}
