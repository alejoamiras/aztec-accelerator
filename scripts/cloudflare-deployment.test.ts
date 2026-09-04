import { describe, expect, test } from "bun:test";
import fs from "node:fs";
import path from "node:path";

const ROOT = path.resolve(import.meta.dir, "..");
const read = (relative: string) => fs.readFileSync(path.join(ROOT, relative), "utf8");

describe("Cloudflare deployment contract", () => {
  test("site deploys use Workers Static Assets and keep build-time configuration in Actions", () => {
    const landing = read(".github/workflows/deploy-landing.yml");
    const sdk = read(".github/workflows/release-sdk.yml");
    expect(landing).toContain("wrangler deploy --config packages/landing/wrangler.jsonc");
    expect(sdk).toContain("AZTEC_NODE_URL: ${{ secrets.TESTNET_AZTEC_NODE_URL }}");
    expect(sdk).toContain("wrangler deploy --config packages/playground/wrangler.jsonc");
    expect(`${landing}\n${sdk}`).not.toMatch(/aws-actions|aws s3|cloudfront/i);

    for (const file of ["packages/landing/wrangler.jsonc", "packages/playground/wrangler.jsonc"]) {
      const config = read(file);
      expect(config).toContain('"directory": "./dist"');
      expect(config).toContain('"not_found_handling": "single-page-application"');
      expect(config).toContain('"preview_urls": true');
    }
  });

  test("only the promote workflow writes the exact verified feed bytes to production KV", () => {
    const release = read(".github/workflows/release-accelerator.yml");
    expect(release).toContain(
      "wrangler kv key put latest.json --path feed/latest.json --remote",
    );
    expect(release.match(/wrangler kv key put/g)).toHaveLength(1);
    expect(release).toContain("CLOUDFLARE_RELEASE_FEED_API_TOKEN");
    expect(release).not.toMatch(/aws-actions|aws s3|cloudfront/i);
  });

  test("the release-feed Worker is deployed independently from feed promotion", () => {
    const deploy = read(".github/workflows/deploy-release-feed.yml");
    expect(deploy).toContain("wrangler deploy --config packages/release-feed/wrangler.jsonc");
    expect(deploy).toContain("environment: release-feed");
    expect(deploy).toContain("CLOUDFLARE_RELEASE_FEED_DEPLOY_API_TOKEN");
    expect(deploy).not.toContain("CLOUDFLARE_RELEASE_FEED_API_TOKEN");
    expect(deploy).not.toContain("wrangler kv key put");
    expect(deploy).not.toContain("push:");
  });

  test("OpenTofu configuration is gone", () => {
    expect(fs.existsSync(path.join(ROOT, "infra/tofu/providers.tf"))).toBe(false);
    expect(read("package.json")).not.toContain("lint:tofu");
    expect(read(".github/workflows/actionlint.yml")).not.toMatch(/opentofu|\btofu\b/i);
  });
});
