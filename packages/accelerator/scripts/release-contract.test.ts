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
const PACKAGED = fs.readFileSync(path.join(REPO, ".github/workflows/_e2e-packaged.yml"), "utf8");
const UNINSTALL_WIN = fs.readFileSync(
  path.join(REPO, ".github/scripts/packaged-e2e-uninstall-windows.ps1"),
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
    // finalize fetches the DRAFT's live digests by numeric release id — a draft 404s on
    // GET /releases/tags/{tag}, so a by-tag fetch yields a 404 body that never matches the manifest and the
    // release never publishes. [mut: revert the fetch to `releases/tags/$TAG` → 404 on the draft → this fails]
    expect(WF).toMatch(/RID=\$\(gh release view "\$TAG" --json databaseId/);
    expect(WF).toContain('gh api "repos/$GH_REPO/releases/$RID"');
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

  test("blocker-1 failure-atomicity: authoritative PUT (fatal) split from best-effort invalidation (non-fatal)", () => {
    // The S3 PUT commits the promote; the CloudFront invalidation only accelerates propagation. They MUST be
    // separate steps AND the invalidation must be non-fatal: if a transient invalidation error after the PUT
    // failed the job, `promote` would go RED (reading as "feed unflipped" when it IS flipped) and the
    // downstream verify-live-feed gate — implicit success() on `promote` — would be SKIPPED, leaving the flip
    // unconfirmed. So promote-success must equal PUT-success. Both steps retry; the PUT is fatal on
    // exhaustion (safe — the feed is unchanged, nothing downstream ran), the invalidation warns + continues.
    expect(WF).toContain("Flip the S3 feed to this version (authoritative PUT)");
    expect(WF).toContain("Invalidate CloudFront (best-effort — never fails a committed promote)");
    // Scope each assert to its own step body so PUT-fatal and invalidation-non-fatal are provable in
    // isolation (and can't false-catch verify-live-feed's own `exit 1`).
    const putStep =
      WF.split("Flip the S3 feed to this version (authoritative PUT)")[1]?.split(
        "Invalidate CloudFront (best-effort",
      )[0] ?? "";
    const invStep =
      WF.split("Invalidate CloudFront (best-effort")[1]?.split(
        "# Enforce the live-feed check",
      )[0] ?? "";
    // PUT: retried, and FATAL on exhaustion. [mut: drop the `exit 1` / retry → the flip stops being atomic]
    expect(putStep, "PUT step body found").toContain("aws s3 cp feed/latest.json");
    expect(putStep, "PUT retries").toContain("for attempt in 1 2 3 4 5; do");
    expect(putStep, "PUT is fatal on exhaustion").toContain("exit 1");
    expect(putStep).toContain("feed unchanged, promote aborted");
    // Invalidation: retried, then WARN + continue — never fatal. [mut: add `exit 1` to the invalidation
    // failure branch, or re-merge it into the PUT step → a post-PUT invalidation hiccup skips verify-live-feed]
    expect(invStep, "invalidation step body found").toContain("create-invalidation");
    expect(invStep, "invalidation retries").toContain("for attempt in 1 2 3; do");
    expect(
      invStep,
      "a failed invalidation must NOT be fatal — no `exit 1` in the step body",
    ).not.toContain("exit 1");
    expect(invStep, "invalidation exhaustion warns (non-fatal)").toContain(
      "::warning::CloudFront invalidation failed after 3 attempts",
    );
    // codex High: verify-live-feed must run after ANY attempted promote via its OWN status function, so a
    // PUT whose CLI errored AFTER S3 committed (feed live but `promote` RED) can't skip live verification —
    // the live feed is the source of truth. [mut: revert to the implicit success() on `promote` → fails]
    expect(WF, "verify-live-feed runs on its own always() status function").toMatch(
      /Verify live updater feed[\s\S]{0,800}?if: \$\{\{ always\(\) && !cancelled\(\) && needs\.validate\.result == 'success' && inputs\.mode == 'promote-only'/,
    );
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

  test("prerelease publish DAG is scheduled (own status fn) yet still can't publish an ungated draft", () => {
    // The RC's skipped `sign-update-feed` poisoned the IMPLICIT success() of the packaged gate / tag /
    // finalize (transitive via `release`'s always()), so the first RC to reach them drafted but never
    // published. Each MUST carry its own status function (always()+!cancelled()) so GitHub schedules it,
    // while the explicit `.result == 'success'` chain stays the authorization policy.
    // [mut: drop `always()` from any of the three → the RC silently skips publish again; drop a
    //  `.result == 'success'` guard → an ungated/failed gate could publish]
    for (const [job, needResult] of [
      ["Packaged E2E on draft", "needs.release.result == 'success'"],
      ["Create Git Tag", "needs.packaged-e2e-on-draft.result == 'success'"],
      ["Publish release (finalize)", "needs.tag.result == 'success'"],
    ] as const) {
      const esc = job.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      const escNeed = needResult.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      expect(WF, `${job}: carries its own always()+!cancelled() status function`).toMatch(
        new RegExp(`${esc}[\\s\\S]{0,700}?always\\(\\) && !cancelled\\(\\)`),
      );
      expect(WF, `${job}: keeps its explicit .result authorization gate`).toMatch(
        new RegExp(`${esc}[\\s\\S]{0,900}?${escNeed}`),
      );
    }
  });

  test("draft-asset write is isolated to the no-code staging job; E2E jobs stay read-only", () => {
    // codex privilege isolation: a DRAFT release is only visible to a write token, but the E2E jobs execute the
    // installed app + a browser + the packed SDK. Only `stage-installers` (no checkout, no app code) holds
    // contents:write to fetch+verify+re-upload the draft's installers; the code-executing E2E jobs consume
    // the staged artifact at contents:read. [mut: elevate an E2E job to contents:write → the write count
    //  exceeds 1 (and reads drop below the job count) → this fails]
    expect(PACKAGED).toContain("stage-installers");
    // Count the actual job PERMISSION lines (6-space indent), not comment mentions of the words.
    const writes = (PACKAGED.match(/^ {6}contents: write\b/gm) || []).length;
    const reads = (PACKAGED.match(/^ {6}contents: read\b/gm) || []).length;
    expect(writes, "exactly one contents:write perm — the staging job only").toBe(1);
    // linux composed / macos composed / linux upgrade-migration / linux uninstall / windows uninstall.
    // Five, not six: there is deliberately NO windows composed-proof leg. The app only serves HTTPS when its
    // own trust predicate passes, and that store is populated only by the interactive root-CA consent dialog
    // — five headless seeding mechanisms were measured dead on hosted runners. That proof is a documented
    // manual pre-GA check instead (packages/accelerator/README.md, "Windows composed proof").
    expect(reads, "every code-executing E2E job stays read-only").toBe(5);
  });

  test("windows uninstall leg drives the REAL uninstaller and cannot pass vacuously", () => {
    // The first end-to-end run of the NSIS PREUNINSTALL/POSTUNINSTALL pair against a real install (today's
    // Windows CI drives those hooks only against a stub exe, or installs without ever uninstalling). Its
    // whole value rests on two properties, so both are pinned:
    //   1. it runs the REAL silent uninstaller, not `--prepare-uninstall` alone;
    //   2. every artifact it asserts GONE is asserted PRESENT first — otherwise the leg would pass on a
    //      runner where the app was never installed, which is exactly the failure mode that makes a
    //      cleanup test worthless (codex: "a post-uninstall absent-assertion without a positive
    //      precondition is worthless").
    // [mut: drop the precondition block, or point the leg at --prepare-uninstall instead of uninstall.exe
    //  → these fail]
    expect(PACKAGED).toContain("Full uninstall (windows)");
    // The leg delegates to a script so the SAME code is runnable outside the release pipeline (which refuses
    // to run from a non-main ref, and is the only caller of this workflow) — otherwise this logic could
    // first be exercised only AFTER it was merged and already release-blocking.
    expect(PACKAGED).toContain("packaged-e2e-uninstall-windows.ps1");
    expect(UNINSTALL_WIN).toMatch(/\$uninstaller\s*=\s*Join-Path \$installDir "uninstall\.exe"/);
    expect(UNINSTALL_WIN).toMatch(/Start-Process -FilePath \$uninstaller -ArgumentList "\/S"/);
    // Positive preconditions: the product must have armed the Run value and the crash-recovery task.
    expect(UNINSTALL_WIN).toContain(
      "precondition: the product did not heal the Run value to its own quoted path",
    );
    expect(UNINSTALL_WIN).toContain(
      "precondition: the product did not arm the crash-recovery task",
    );
    // Retention is asserted alongside removal, BY BYTES — "still exists" would also pass if the uninstall
    // truncated or rewrote the user's config (codex r2).
    expect(UNINSTALL_WIN).toContain("config.json was deleted");
    expect(UNINSTALL_WIN).toContain("config.json was MODIFIED by the uninstall");
    // Absence oracles must be FAIL-CLOSED. `schtasks /Query` exit 1 is "does not exist"; any other non-zero
    // is an error (access denied, scheduler fault) and must NOT be read as "gone". Likewise the Run value is
    // checked by enumerating value names, not via -EA SilentlyContinue (which maps a read failure to $null).
    // [mut: accept any non-zero as absent, or go back to the SilentlyContinue read → these fail]
    expect(UNINSTALL_WIN).toContain("cannot conclude the task is gone");
    expect(UNINSTALL_WIN).toMatch(/GetValueNames\(\) -contains \$runValueName/);
    // "Uninstalled while running" is the leg's whole point, so the app must be provably alive at that moment
    // and that exact process must be gone after.
    // By NAME, not by pid: deleting a scheduled task does not terminate a process it already started, so a
    // task-spawned instance could outlive both the task and our original pid (codex r3). [mut: narrow either
    // side back to $proc.HasExited → this fails]
    expect(UNINSTALL_WIN).toContain(
      "precondition: no AztecAccelerator process is running before the uninstall",
    );
    expect(UNINSTALL_WIN).toContain("process(es) survived the uninstall");
    // The task precondition parses the XML and compares Exec.Command, rather than substring-matching the
    // whole document (where the path could sit in an argument or description).
    // Array-wrapped and required to be exactly ONE action: `.Command` is a collection when the task has
    // multiple <Exec> nodes, and `-ne` against a collection filters instead of returning a boolean, so a
    // bare comparison passes silently (codex r4). [mut: drop the Count check → false-green returns]
    expect(UNINSTALL_WIN).toMatch(/\$taskCommands = @\(\$taskDoc\.Task\.Actions\.Exec\.Command\)/);
    expect(UNINSTALL_WIN).toMatch(/\$taskCommands\.Count -ne 1/);
  });
});
