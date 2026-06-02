import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import {
  assertSha256,
  resolveWindowsBbChecksum,
  WINDOWS_BB_ASSET,
  WINDOWS_BB_CHECKSUMS,
  windowsBbReleaseTag,
} from "./copy-bb.ts";

describe("windows bb.exe sidecar supply chain", () => {
  test("release tag derives from the live bb.js version", () => {
    expect(windowsBbReleaseTag("4.2.0")).toBe("v4.2.0");
    // A pre-release suffix is carried through verbatim (forces a matching pinned hash).
    expect(windowsBbReleaseTag("4.3.0-aztecnr-rc.1")).toBe("v4.3.0-aztecnr-rc.1");
  });

  test("the shipped version has a pinned, well-formed checksum", () => {
    expect(resolveWindowsBbChecksum("4.2.0")).toBe(WINDOWS_BB_CHECKSUMS["4.2.0"]);
    expect(resolveWindowsBbChecksum("4.2.0")).toMatch(/^[0-9a-f]{64}$/);
  });

  test("an unknown version fails closed — forces a hash review on every bb bump", () => {
    expect(() => resolveWindowsBbChecksum("9.9.9")).toThrow(/No pinned Windows bb\.exe SHA-256/);
  });

  test("a tampered tarball is rejected (SHA-256 mismatch)", () => {
    expect(() =>
      assertSha256(Buffer.from("malicious payload"), "0".repeat(64), WINDOWS_BB_ASSET),
    ).toThrow(/SHA-256 mismatch/);
  });

  test("a matching tarball passes verification", () => {
    const data = Buffer.from("bb.exe bytes");
    const sha = createHash("sha256").update(data).digest("hex");
    expect(() => assertSha256(data, sha, "test")).not.toThrow();
  });
});
