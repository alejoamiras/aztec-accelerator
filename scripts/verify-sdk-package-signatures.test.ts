import { describe, expect, test } from "bun:test";
import { hasVerifiedSdkProvenance } from "./verify-sdk-package-signatures.ts";

describe("SDK signature verification", () => {
  test("requires verified SLSA provenance for the exact package and version", () => {
    const report = {
      verified: [
        {
          name: "@alejoamiras/aztec-accelerator",
          version: "5.2.0",
          attestations: { provenance: { predicateType: "https://slsa.dev/provenance/v1" } },
        },
      ],
    };
    expect(hasVerifiedSdkProvenance(report, "5.2.0")).toBe(true);
    expect(hasVerifiedSdkProvenance(report, "5.2.1")).toBe(false);
  });
});
