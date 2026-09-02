import { describe, expect, test } from "bun:test";
import { evaluateFindings, parseAllowlist, parseBunAudit, parseCargoAudit } from "./dependency-audit.ts";

describe("dependency audit gate", () => {
  test("parses npm advisory ids from their canonical URL", () => {
    expect(
      parseBunAudit({
        undici: [
          {
            severity: "high",
            title: "WebSocket denial of service",
            url: "https://github.com/advisories/GHSA-example-1234-5678",
          },
        ],
      }),
    ).toEqual([
      {
        ecosystem: "npm",
        id: "GHSA-example-1234-5678",
        package: "undici",
        severity: "high",
        title: "WebSocket denial of service",
      },
    ]);
  });

  test("treats cargo-audit vulnerabilities as blocking even without CVSS", () => {
    expect(
      parseCargoAudit(
        {
          vulnerabilities: {
            list: [
              {
                advisory: { id: "RUSTSEC-2026-0001", title: "Memory safety issue" },
                package: { name: "example" },
              },
            ],
          },
        },
        "Cargo.lock",
      ),
    ).toEqual([
      {
        ecosystem: "rust",
        id: "RUSTSEC-2026-0001",
        package: "example",
        severity: "unknown",
        title: "Memory safety issue",
        source: "Cargo.lock",
      },
    ]);
  });

  test("accepts reviewed findings, reports lower severities, and blocks new or expired findings", () => {
    const findings = [
      {
        ecosystem: "npm" as const,
        id: "GHSA-accepted",
        package: "accepted",
        severity: "high" as const,
        title: "accepted",
      },
      {
        ecosystem: "npm" as const,
        id: "GHSA-expired",
        package: "expired",
        severity: "critical" as const,
        title: "expired",
      },
      {
        ecosystem: "npm" as const,
        id: "GHSA-low",
        package: "low",
        severity: "low" as const,
        title: "reported",
      },
    ];
    const base = {
      ecosystem: "npm" as const,
      reason: "reviewed",
      upgrade: "upgrade upstream",
    };
    const result = evaluateFindings(
      findings,
      [
        { ...base, id: "GHSA-accepted", package: "accepted", expires: "2026-12-01" },
        { ...base, id: "GHSA-expired", package: "expired", expires: "2026-08-01" },
      ],
      "2026-08-31",
    );

    expect(result.accepted).toHaveLength(1);
    expect(result.reported).toHaveLength(1);
    expect(result.blocked.map((finding) => finding.id)).toEqual(["GHSA-expired"]);
  });

  test("validates the allowlist schema and real calendar dates", () => {
    expect(() =>
      parseAllowlist({
        reviewed: "2026-08-31",
        exceptions: [
          {
            ecosystem: "python",
            id: "CVE-example",
            package: "example",
            expires: "2026-02-29",
            reason: "reviewed",
            upgrade: "upgrade",
          },
        ],
      }),
    ).toThrow("invalid ecosystem");
    expect(() => parseAllowlist({ reviewed: "2026-02-29", exceptions: [] })).toThrow(
      "real YYYY-MM-DD date",
    );
  });
});
