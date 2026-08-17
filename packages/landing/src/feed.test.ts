import { describe, expect, test } from "bun:test";
import { feedVersionToTag } from "./feed";

// B6: the landing download button derives the live version from the SIGNED update feed, whose payload is
// UNTRUSTED (the browser doesn't verify its signature). These pin the parse contract: only a strict stable
// SemVer becomes a canonical `accelerator-v<version>` tag; everything else → null (caller falls back to the
// GitHub releases page). [mut: relax STABLE_SEMVER in feed.ts → the prerelease/injection rows fail.]

describe("feedVersionToTag (untrusted feed → canonical tag)", () => {
  test("a stable X.Y.Z version maps to the canonical release tag", () => {
    expect(feedVersionToTag({ version: "2.0.0" })).toBe("accelerator-v2.0.0");
    expect(feedVersionToTag({ version: "10.20.30" })).toBe("accelerator-v10.20.30");
  });

  test("a prerelease version is REJECTED (the promoted feed never carries an RC — fixes RC-as-download)", () => {
    expect(feedVersionToTag({ version: "2.0.0-rc.1" })).toBeNull();
    expect(feedVersionToTag({ version: "2.0.0-nightly.20260101" })).toBeNull();
  });

  test("non-SemVer / injection-shaped versions are REJECTED (no host/path escape)", () => {
    // Anything that isn't exactly digits.digits.digits — so a feed field can never inject a URL host/path.
    expect(feedVersionToTag({ version: "2.0" })).toBeNull();
    expect(feedVersionToTag({ version: "2.0.0.0" })).toBeNull();
    expect(feedVersionToTag({ version: "../../evil" })).toBeNull();
    expect(feedVersionToTag({ version: "2.0.0/../../x" })).toBeNull();
    expect(feedVersionToTag({ version: "2.0.0 evil.com" })).toBeNull();
    expect(feedVersionToTag({ version: "evil.com/2.0.0" })).toBeNull();
    expect(feedVersionToTag({ version: "" })).toBeNull();
  });

  test("missing / wrong-typed / non-object bodies are REJECTED", () => {
    expect(feedVersionToTag({})).toBeNull();
    expect(feedVersionToTag({ version: 200 })).toBeNull();
    expect(feedVersionToTag({ version: null })).toBeNull();
    expect(feedVersionToTag(null)).toBeNull();
    expect(feedVersionToTag(undefined)).toBeNull();
    expect(feedVersionToTag("2.0.0")).toBeNull();
    expect(feedVersionToTag(["2.0.0"])).toBeNull();
  });
});
