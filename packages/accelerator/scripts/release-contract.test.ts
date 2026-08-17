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
  test("least privilege: `promote` is the ONLY leg with AWS creds; `release` (publish) has none", () => {
    // [mut: move the aws s3 cp back into `release`, or add id-token to release → fails]
    expect(WF).toContain("id-token: write    # OIDC → AWS feed-only role");
    expect(WF).toContain("aws s3 cp feed/latest.json"); // promote uploads the verified downloaded bytes
    expect(WF).not.toContain("aws s3 cp release-files/latest.json"); // the old in-release promote is gone
    expect(WF).toContain("contents: write     # gh release create (publish ONLY");
  });

  test("append-only: never delete a published release, never --clobber", () => {
    // [mut: re-add `gh release delete "$RELEASE_TAG" --yes` → fails]
    expect(WF).not.toContain("gh release delete");
    expect(WF).not.toContain("--clobber");
    expect(WF).toContain("append-only; bump the version");
  });

  test("the signed feed is the source of truth — no release is marked GitHub --latest", () => {
    // [mut: change a `gh release create` back to a bare `--latest \` → fails]
    expect(WF).not.toContain("--latest \\"); // bare `--latest` line-continued (the removed stable mark)
    expect(WF).toContain("--latest=false");
  });

  test("mode split: `promote` runs only under promote-only; `release` only under publish", () => {
    // [mut: drop the mode guard on `release` → its !cancelled() would run it under promote-only → fails]
    expect(WF).toContain("inputs.mode == 'promote-only'");
    // The release job's guard: publish-mode AND its original no-failed-needs condition (unique fragment).
    expect(WF).toContain("inputs.mode == 'publish' && !cancelled()");
  });

  test("promote pre-flight verifies a published, non-draft, non-prerelease stable with a signed feed", () => {
    // [mut: delete any of these pre-flight checks → a half-built / draft / wrong-version promote slips → fails]
    expect(WF).toContain(".isDraft == false");
    expect(WF).toContain(".isPrerelease == false");
    expect(WF).toContain("verify --feed feed/latest.json"); // production Ed25519 verifier over the feed
    expect(WF).toContain("!= dispatched"); // feed version == dispatched version guard
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
