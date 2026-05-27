import { describe, expect, test } from "bun:test";
import { resolvePublishVersion } from "./get-sdk-publish-version";

describe("resolvePublishVersion", () => {
  test("returns base version when not yet published", () => {
    expect(resolvePublishVersion("5.0.0-nightly.20260224", [])).toBe("5.0.0-nightly.20260224");
  });

  test("returns base version when only other versions are published", () => {
    expect(
      resolvePublishVersion("5.0.0-nightly.20260224", ["5.0.0-nightly.20260223"]),
    ).toBe("5.0.0-nightly.20260224");
  });

  test("appends .1 when base is already published", () => {
    expect(
      resolvePublishVersion("5.0.0-nightly.20260224", ["5.0.0-nightly.20260224"]),
    ).toBe("5.0.0-nightly.20260224.1");
  });

  test("appends .2 when .1 is already published", () => {
    expect(
      resolvePublishVersion("5.0.0-nightly.20260224", [
        "5.0.0-nightly.20260224",
        "5.0.0-nightly.20260224.1",
      ]),
    ).toBe("5.0.0-nightly.20260224.2");
  });

  test("skips gaps in revision numbers", () => {
    expect(
      resolvePublishVersion("5.0.0-nightly.20260224", [
        "5.0.0-nightly.20260224",
        "5.0.0-nightly.20260224.1",
        "5.0.0-nightly.20260224.3",
      ]),
    ).toBe("5.0.0-nightly.20260224.4");
  });

  test("works with rc versions", () => {
    expect(
      resolvePublishVersion("4.1.0-rc.4", ["4.1.0-rc.4"]),
    ).toBe("4.1.0-rc.4.1");
  });

  test("stable base returns -revision.1 when already published", () => {
    expect(resolvePublishVersion("4.2.0", ["4.2.0"])).toBe("4.2.0-revision.1");
  });

  test("stable base returns -revision.2 when -revision.1 already published", () => {
    expect(
      resolvePublishVersion("4.2.0", ["4.2.0", "4.2.0-revision.1"]),
    ).toBe("4.2.0-revision.2");
  });

  test("stable base unchanged when not yet published", () => {
    expect(resolvePublishVersion("4.3.0", [])).toBe("4.3.0");
  });

  test("stable base does not confuse prereleases with revisions", () => {
    expect(
      resolvePublishVersion("4.2.0", ["4.2.0", "4.2.0-rc.1", "4.2.0-rc.2"]),
    ).toBe("4.2.0-revision.1");
  });
});
