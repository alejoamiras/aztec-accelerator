import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const repository = resolve(import.meta.dir, "..");
const release = readFileSync(resolve(repository, ".github/workflows/release-sdk.yml"), "utf8");
const publish = readFileSync(resolve(repository, ".github/workflows/_publish-sdk.yml"), "utf8");

describe("SDK release workflow contract", () => {
  test("OIDC is the only publish credential and binds the top-level environment", () => {
    expect(release).toContain("id-token: write");
    expect(publish).toContain("id-token: write");
    expect(publish).toContain("environment: npm-publish");
    expect(`${release}\n${publish}`).not.toMatch(/NPM_TOKEN|NODE_AUTH_TOKEN/);
    expect(publish).toContain("npm publish \"$TARBALL\" --provenance --access public --tag testnet");
  });

  test("dependency audit and exact cryptographic verification gate publication records", () => {
    expect(release).toContain("uses: ./.github/workflows/dependency-audit.yml");
    expect(release).toContain("needs: [assert-main, e2e, dependency-audit]");
    const verification = publish.indexOf("bun scripts/verify-sdk-package-signatures.ts");
    const records = publish.indexOf("Create git tag and GitHub release");
    expect(verification).toBeGreaterThan(0);
    expect(records).toBeGreaterThan(verification);
  });
});
