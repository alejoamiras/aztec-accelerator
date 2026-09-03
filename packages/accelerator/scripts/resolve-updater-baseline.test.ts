import { describe, expect, test } from "bun:test";
import { selectUpdaterBaseline, type UpdaterReleaseCandidate } from "./resolve-updater-baseline";

const OLD_KEY = "old-updater-key";
const NEW_KEY = "new-updater-key";

function candidate(
  version: string,
  pubkey: string,
  overrides: Partial<UpdaterReleaseCandidate> = {},
): UpdaterReleaseCandidate {
  return {
    tagName: `accelerator-v${version}`,
    draft: false,
    pubkey,
    assetNames: [
      `Aztec-Accelerator-${version}-macOS-Apple-Silicon.dmg`,
      `Aztec-Accelerator-${version}-macOS-Intel.dmg`,
      `Aztec-Accelerator-${version}-Linux-x86_64.AppImage`,
      `Aztec-Accelerator-${version}-Windows-x86_64-setup.exe`,
    ],
    ...overrides,
  };
}

describe("updater smoke baseline selection", () => {
  test("prefers the greatest lower same-key release, including a prerelease", () => {
    const result = selectUpdaterBaseline({
      version: "3.0.0",
      currentPubkey: NEW_KEY,
      releases: [
        candidate("3.0.0-rc.1", NEW_KEY),
        candidate("2.0.1-rc.1", OLD_KEY),
        candidate("2.0.0", OLD_KEY),
      ],
    });

    expect(result).toEqual({
      tag: "accelerator-v3.0.0-rc.1",
      version: "3.0.0-rc.1",
    });
  });

  test("fails closed when no complete lower release uses the current key", () => {
    const releases = [candidate("2.0.1-rc.1", OLD_KEY), candidate("2.0.0", OLD_KEY)];

    expect(() =>
      selectUpdaterBaseline({
        version: "3.0.0-rc.1",
        currentPubkey: NEW_KEY,
        releases,
      }),
    ).toThrow("no complete published lower accelerator release uses the current updater key");
  });

  test("ignores drafts, incomplete releases, and versions not below N", () => {
    const incomplete = candidate("3.0.0-rc.2", NEW_KEY);
    incomplete.assetNames.pop();

    const result = selectUpdaterBaseline({
      version: "3.0.0-rc.3",
      currentPubkey: NEW_KEY,
      releases: [
        candidate("3.0.0", NEW_KEY),
        incomplete,
        candidate("3.0.0-rc.1", NEW_KEY),
        candidate("2.0.1-rc.1", NEW_KEY, { draft: true }),
      ],
    });

    expect(result.version).toBe("3.0.0-rc.1");
  });
});
