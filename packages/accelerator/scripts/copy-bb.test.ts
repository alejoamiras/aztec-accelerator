import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import {
  assertSha256,
  resolveAztecBb,
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

  test("the LIVE bb.js version has an accepted, well-formed manual-review pin", () => {
    // F-008: key on the version the gate actually resolves (resolveAztecBb), not a hard-coded literal.
    const { version } = resolveAztecBb();
    expect(resolveWindowsBbChecksum(version)).toMatch(/^[0-9a-f]{64}$/);
    expect(WINDOWS_BB_CHECKSUMS[version]?.provenance).toBe("manual-review");
  });

  test("an unknown version fails closed — forces a reviewed pin on every bb bump", () => {
    expect(() => resolveWindowsBbChecksum("9.9.9")).toThrow(/No pinned Windows bb\.exe SHA-256/);
  });

  test("a non-manual-review provenance fails closed (reserved attestation)", () => {
    WINDOWS_BB_CHECKSUMS["0.0.0-att"] = {
      sha256: "a".repeat(64),
      provenance: "attestation",
      note: "reserved",
    };
    try {
      expect(() => resolveWindowsBbChecksum("0.0.0-att")).toThrow(/not accepted yet/);
    } finally {
      delete WINDOWS_BB_CHECKSUMS["0.0.0-att"];
    }
  });

  test("a malformed sha256 fails closed", () => {
    WINDOWS_BB_CHECKSUMS["0.0.0-badsha"] = {
      sha256: "NOTHEX",
      provenance: "manual-review",
      note: "x",
    };
    try {
      expect(() => resolveWindowsBbChecksum("0.0.0-badsha")).toThrow(/malformed sha256/);
    } finally {
      delete WINDOWS_BB_CHECKSUMS["0.0.0-badsha"];
    }
  });

  test("an empty / whitespace review note fails closed", () => {
    WINDOWS_BB_CHECKSUMS["0.0.0-emptynote"] = {
      sha256: "a".repeat(64),
      provenance: "manual-review",
      note: "",
    };
    WINDOWS_BB_CHECKSUMS["0.0.0-wsnote"] = {
      sha256: "a".repeat(64),
      provenance: "manual-review",
      note: "   ",
    };
    try {
      expect(() => resolveWindowsBbChecksum("0.0.0-emptynote")).toThrow(/empty review note/);
      expect(() => resolveWindowsBbChecksum("0.0.0-wsnote")).toThrow(/empty review note/);
    } finally {
      delete WINDOWS_BB_CHECKSUMS["0.0.0-emptynote"];
      delete WINDOWS_BB_CHECKSUMS["0.0.0-wsnote"];
    }
  });

  test("every committed pin is manual-review with a well-formed sha + a real note", () => {
    for (const [version, pin] of Object.entries(WINDOWS_BB_CHECKSUMS)) {
      expect(pin.provenance, `${version} provenance`).toBe("manual-review");
      expect(pin.sha256, `${version} sha`).toMatch(/^[0-9a-f]{64}$/);
      expect(pin.note.trim().length, `${version} note`).toBeGreaterThan(0);
    }
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

describe("aztec bb version resolver (Phase 3b — the version-only CI path)", () => {
  test("resolves a live @aztec/bb.js version + package root from the dep tree", () => {
    const { version, bbJsRoot } = resolveAztecBb();
    // A real semver-ish version (e.g. 4.2.0 / 4.2.0-aztecnr-rc.2), never the "unknown" fallback.
    expect(version).toMatch(/^\d+\.\d+\.\d+/);
    expect(version).not.toBe("unknown");
    expect(existsSync(bbJsRoot)).toBe(true);
  });
});
