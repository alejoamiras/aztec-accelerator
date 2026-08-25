import { describe, expect, test } from "bun:test";
import { majorOf, sameMajor } from "./version";

describe("majorOf", () => {
  test("plain, prefixed, range, and prerelease forms", () => {
    expect(majorOf("5.2.0")).toBe("5");
    expect(majorOf("v5.2.0")).toBe("5");
    expect(majorOf("^5.2.0")).toBe("5");
    expect(majorOf("5.2.0-rc.1")).toBe("5");
  });

  test("unparseable forms yield undefined", () => {
    expect(majorOf("unknown")).toBeUndefined();
    expect(majorOf("")).toBeUndefined();
    expect(majorOf("5")).toBeUndefined(); // bare major without a dot is not semver-ish
  });
});

describe("sameMajor", () => {
  test("minor/patch drift within a major is compatible", () => {
    expect(sameMajor("5.1.0", "5.2.0")).toBe(true);
    expect(sameMajor("5.2.0", "5.2.1")).toBe(true);
  });

  test("major drift is incompatible", () => {
    expect(sameMajor("5.2.0", "6.0.0")).toBe(false);
  });

  test("undecidable when either side is unparseable", () => {
    expect(sameMajor("unknown", "5.2.0")).toBeUndefined();
    expect(sameMajor("5.2.0", "unknown")).toBeUndefined();
  });
});
