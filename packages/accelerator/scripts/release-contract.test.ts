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
const UPDATER = fs.readFileSync(path.join(REPO, ".github/workflows/_e2e-updater.yml"), "utf8");
const UPDATER_LINUX = fs.readFileSync(
  path.join(REPO, ".github/workflows/_e2e-updater-linux.yml"),
  "utf8",
);

describe("release-accelerator.yml — B6 publish/promote contract", () => {
  test("least privilege: `promote` is the only leg that WRITES the feed; `release` (publish) has no AWS", () => {
    // [mut: move the aws s3 cp back into `release`, or add id-token to release → fails]
    expect(WF).toContain("aws s3 cp feed/latest.json"); // promote uploads the verified downloaded bytes
    expect(WF).not.toContain("aws s3 cp release-files/latest.json"); // the old in-release promote is gone
    expect(WF).toContain("contents: write     # gh release create --draft");
    // release-auth-preflight ALSO holds id-token but only PROBES — it must be gated off standalone
    // promote-only so it can't fail RED after the flip already mutated S3.
    expect(WF).toContain("inputs.auth_probe || inputs.mode == 'publish'");
  });

  test("append-only: only an isDraft-guarded draft delete, never a published release, never --clobber", () => {
    // B4 draft-gate: `release` MAY delete a stale UNPUBLISHED draft (fix-forward re-run), but never a
    // published release. The one `gh release delete` must be isDraft-guarded (a no-tag check + an immediate
    // isDraft re-check right before the delete), and the PUBLISHED path must still error, not delete.
    // [mut: remove the immediate `is no longer a draft` re-check before the delete → this assert fails]
    expect(WF).toContain("gh release delete"); // the B4 draft cleanup exists now
    expect(WF).toContain("is no longer a draft (published between view and delete)"); // TOCTOU re-check guard
    expect(WF).toContain("refusing to delete"); // the no-pushed-tag guard
    expect(WF).toContain("already PUBLISHED — append-only; bump the version"); // published ⇒ error, never delete
    expect(WF).not.toContain("--clobber");
  });

  test("the signed feed is the source of truth — no release is marked GitHub --latest", () => {
    // [mut: change a `gh release create` back to `--latest` / `--latest=true` → fails]
    // Any bare `--latest`, `--latest=true`, or line-continued `--latest` — but NOT `--latest=false`.
    expect(WF).not.toMatch(/--latest(\s|=true|$)/m);
    expect(WF).toContain("--latest=false");
  });

  test("mode split: `promote` runs only under promote-only; publish gated across release/tag/finalize", () => {
    // [mut: drop the mode guard on `release` → its always()/!cancelled() would run it under promote-only → fails]
    expect(WF).toContain("inputs.mode == 'promote-only'");
    expect(WF).toContain("inputs.mode == 'publish'"); // release/tag/finalize are all publish-only
    // B4 draft-gate: the publish DECISION moved to `finalize`, which requires the gate + tag to have
    // SUCCEEDED — a skipped/failed packaged gate or tag can NEVER publish. (Was one `release` fragment pre-B4.)
    expect(WF).toContain("needs.tag.result == 'success'");
    expect(WF).toContain("needs.packaged-e2e-on-draft.result == 'success'");
  });

  test("B4 draft-gate: draft(--target sha) → packaged-e2e → finalize with byte-provable publish", () => {
    // Recipe F: `release` creates a DRAFT pinned to the reviewed SHA + an immutable asset manifest; the
    // packaged gate runs the legs against the draft's OWN assets; `finalize` publishes only after the tag +
    // per-asset digests re-verify. So a failed gate never burns the version tag, and tested==published bytes.
    // [mut: drop `--target "$GITHUB_SHA"` from the draft create → this pin assert fails]
    expect(WF).toContain("--draft"); // draft, not a direct publish
    expect(WF).toContain('--target "$GITHUB_SHA"'); // pinned to the dispatched commit, not HEAD-at-publish
    expect(WF).toContain("packaged-e2e-on-draft"); // the gate job
    expect(WF).toContain("uses: ./.github/workflows/_e2e-packaged.yml"); // runs the packaged legs
    expect(WF).toContain("release-asset-manifest"); // the immutable SHA-256 asset manifest
    expect(WF).toContain("--draft=false"); // finalize flips draft → published
    expect(WF).toContain("digests/names differ from the gated manifest"); // finalize's byte re-verify
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

describe("release-machinery hardening (2026-08-17 GitHub asset-CDN incident)", () => {
  test("packaged-E2E-on-draft pins its harness checkout to the dispatched SHA, not moving main", () => {
    // _e2e-packaged.yml checks out `inputs.ref || github.ref`; the caller must pass github.sha so a
    // concurrent push to main mid-run can't test the draft's pinned installers against a newer harness.
    // [mut: drop `ref: ${{ github.sha }}` from the _e2e-packaged.yml caller → this assert fails]
    expect(WF).toMatch(
      /uses: \.\/\.github\/workflows\/_e2e-packaged\.yml[\s\S]{0,500}?ref: \$\{\{ github\.sha \}\}/,
    );
  });

  test("N-1 release-asset download retries + integrity-checks (linux + darwin) — the 3-strike flake fix", () => {
    // The unretried `gh release download` failed 3 straight RC dispatches during a GitHub asset-CDN incident.
    // Both legs must retry 5×, clear partials between tries, verify the asset's sha256 digest, and fail closed
    // after exhaustion. [mut: drop the retry loop / digest check → a single transient CDN error (or a
    // truncated exit-0 download) fails/poisons the gate again]
    for (const [wf, name] of [
      [UPDATER, "_e2e-updater.yml (darwin DMG)"],
      [UPDATER_LINUX, "_e2e-updater-linux.yml (linux AppImage)"],
    ] as const) {
      expect(wf, `${name}: retries the download`).toContain("for attempt in 1 2 3 4 5; do");
      expect(wf, `${name}: clears partials between tries`).toContain("rm -rf n1; mkdir -p n1");
      expect(wf, `${name}: verifies the asset sha256 digest`).toContain("shasum -a 256");
      expect(wf, `${name}: reads the API digest`).toContain(".digest // empty");
      expect(wf, `${name}: fails closed after exhaustion`).toContain(
        "failed or failed integrity after 5 attempts",
      );
    }
  });
});
