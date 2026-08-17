/**
 * B6 release-pipeline contract guards (`bun test scripts/`). release-accelerator.yml SPLITS publish from
 * promote: a `publish` dispatch builds+gates+publishes the GitHub release but NEVER flips the auto-updater's
 * S3 `latest.json` feed; a separate `promote-only` dispatch flips the feed (and is the rollback lever).
 * Each row below pins one invariant of that split and is mutation-provable by a single YAML edit — a
 * regression (re-coupling promote into publish, deleting a published release, marking GitHub --latest,
 * dropping a pre-flight check) flips CI in milliseconds instead of surfacing as a bad/oversold release.
 */
import { describe, expect, test } from "bun:test";
import fs from "node:fs";
import path from "node:path";

const REPO = path.resolve(import.meta.dir, "..", "..", "..");
const WF = fs.readFileSync(path.join(REPO, ".github/workflows/release-accelerator.yml"), "utf8");

describe("release-accelerator.yml — B6 publish/promote contract", () => {
  test("least privilege: `promote` is the only leg that WRITES the feed; `release` (publish) has no AWS", () => {
    // [mut: move the aws s3 cp back into `release`, or add id-token to release → fails]
    expect(WF).toContain("aws s3 cp feed/latest.json"); // promote uploads the verified downloaded bytes
    expect(WF).not.toContain("aws s3 cp release-files/latest.json"); // the old in-release promote is gone
    expect(WF).toContain("contents: write     # gh release create (publish ONLY");
    // release-auth-preflight ALSO holds id-token but only PROBES — it must be gated off standalone
    // promote-only so it can't fail RED after the flip already mutated S3.
    expect(WF).toContain("inputs.auth_probe || inputs.mode == 'publish'");
  });

  test("append-only: never delete a published release, never --clobber", () => {
    // [mut: re-add `gh release delete "$RELEASE_TAG" --yes` → fails]
    // B6 has NO release delete at all. When B4 adds the stable DRAFT-gate it will introduce an
    // isDraft-GUARDED draft cleanup — at that point tighten this to reject only an UNguarded/published
    // delete, not the blanket string. (codex flagged the blanket check would reject B4's legit cleanup.)
    expect(WF).not.toContain("gh release delete");
    expect(WF).not.toContain("--clobber");
    expect(WF).toContain("append-only; bump the version");
  });

  test("the signed feed is the source of truth — no release is marked GitHub --latest", () => {
    // [mut: change a `gh release create` back to `--latest` / `--latest=true` → fails]
    // Any bare `--latest`, `--latest=true`, or line-continued `--latest` — but NOT `--latest=false`.
    expect(WF).not.toMatch(/--latest(\s|=true|$)/m);
    expect(WF).toContain("--latest=false");
  });

  test("mode split: `promote` runs only under promote-only; `release` only under publish", () => {
    // [mut: drop the mode guard on `release` → its !cancelled() would run it under promote-only → fails]
    expect(WF).toContain("inputs.mode == 'promote-only'");
    // The release job's guard: publish-mode AND tag SUCCESS (not merely not-failed) so a skipped tag can't
    // publish onto an orphan tag via --verify-tag. This exact fragment is unique to the release job.
    expect(WF).toContain("inputs.mode == 'publish' && needs.tag.result == 'success'");
  });

  test("promote pre-flight verifies a published, non-draft, non-prerelease stable with a signed feed", () => {
    // [mut: delete any pre-flight check → a half-built / draft / wrong-version / wrong-URL / wrong-platform
    //  feed could be promoted → fails]
    expect(WF).toContain(".isDraft == false");
    expect(WF).toContain(".isPrerelease == false");
    expect(WF).toContain("verify --feed feed/latest.json"); // production Ed25519 verifier over the feed
    expect(WF).toContain("!= dispatched"); // feed version == dispatched version guard
    // EXACT 17-name asset set (not count+category — which padding could game).
    expect(WF).toContain("asset set != the expected 17");
    // `VER` is the literal bash `${VERSION}` placeholder the workflow uses; template-interpolating it below
    // reproduces the exact asset names without a plain-string `${...}` (which biome's noTemplateCurlyInString
    // would flag). The single suppressed line is the only place the literal placeholder appears.
    // biome-ignore lint/suspicious/noTemplateCurlyInString: intentional literal bash ${VERSION} placeholder
    const VER = "${VERSION}";
    // Drift guard: ALL 16 build-asset names must appear in BOTH the flatten (publish) list AND the promote
    // list — so the two authoritative lists can't silently diverge (codex r3: sample all 16, not 2).
    for (const name of [
      `Aztec-Accelerator-${VER}-macOS-Apple-Silicon.dmg`,
      `Aztec-Accelerator-${VER}-macOS-Intel.dmg`,
      `Aztec-Accelerator-${VER}-macOS-Apple-Silicon.app.tar.gz`,
      `Aztec-Accelerator-${VER}-macOS-Intel.app.tar.gz`,
      `Aztec-Accelerator-${VER}-Linux-x86_64.deb`,
      `Aztec-Accelerator-${VER}-Linux-x86_64.AppImage`,
      `Aztec-Accelerator-${VER}-Windows-x86_64-setup.exe`,
      `Aztec-Accelerator-${VER}-Windows-x86_64-setup.nsis.zip`,
      `accelerator-server-${VER}-macos-arm64.tar.gz`,
      `accelerator-server-${VER}-macos-arm64.tar.gz.sha256`,
      `accelerator-server-${VER}-macos-x86_64.tar.gz`,
      `accelerator-server-${VER}-macos-x86_64.tar.gz.sha256`,
      `accelerator-server-${VER}-linux-x86_64.tar.gz`,
      `accelerator-server-${VER}-linux-x86_64.tar.gz.sha256`,
      `accelerator-server-${VER}-linux-arm64.tar.gz`,
      `accelerator-server-${VER}-linux-arm64.tar.gz.sha256`,
    ]) {
      expect(
        WF.split(name).length - 1,
        `${name} must be in both the flatten + promote asset lists`,
      ).toBeGreaterThanOrEqual(2);
    }
    // The verifier accepts any non-empty signed map, so bind the feed to THIS release: exact 4 platform keys
    // AND each platform's EXACT artifact URL (not just a canonical prefix — codex: darwin-aarch64 could
    // otherwise point at the Intel tarball). Assert ALL FOUR key→filename mappings (whitespace-flexible).
    expect(WF).toContain('["darwin-aarch64","darwin-x86_64","linux-x86_64","windows-x86_64"]');
    for (const [key, file] of [
      ["darwin-aarch64", `Aztec-Accelerator-${VER}-macOS-Apple-Silicon.app.tar.gz`],
      ["darwin-x86_64", `Aztec-Accelerator-${VER}-macOS-Intel.app.tar.gz`],
      ["linux-x86_64", `Aztec-Accelerator-${VER}-Linux-x86_64.AppImage`],
      ["windows-x86_64", `Aztec-Accelerator-${VER}-Windows-x86_64-setup.nsis.zip`],
    ]) {
      const esc = file.replace(/[.$^{}()|[\]\\]/g, "\\$&");
      expect(WF, `${key} must be bound to ${file}`).toMatch(
        new RegExp(`assert_url ${key}\\s+"${esc}"`),
      );
    }
  });

  test("dry_run flips nothing — the S3 write is gated on !dry_run", () => {
    // [mut: remove the `if: !inputs.dry_run` on the flip step → a dry run would mutate prod → fails]
    expect(WF).toMatch(/Flip the S3 feed[\s\S]{0,120}?if: \$\{\{ !inputs\.dry_run \}\}/);
  });

  test("downstream wiring: verify-live-feed needs promote; bump-source only on organic-GA promote", () => {
    // [mut: point verify-live-feed back at `release`, or drop the bump_source guard → fails]
    expect(WF).toContain("needs: [validate, promote]");
    expect(WF).toContain("inputs.bump_source && !inputs.dry_run");
  });
});
