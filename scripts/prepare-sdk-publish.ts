// B7 (F14/F15): rewrite the SDK package.json for publishing — set the resolved version and repoint
// `main`/`types`/`exports` at the built `dist/`, replacing the source `exports: "./src/index.ts"` the repo
// uses for workspace consumption. Extracted from an inline `node -e` in `_publish-sdk.yml` so the rewrite
// is diff-reviewable, unit-tested (the mutation is pure), and reusable by the tarball-consumer CI job.

/** The published `exports` map — dist-based, dual types/default condition. */
export const PUBLISHED_EXPORTS = {
  ".": { types: "./dist/index.d.ts", default: "./dist/index.js" },
} as const;

/**
 * Pure rewrite: given the source manifest and the resolved version, return the manifest to publish. Sets
 * `version`, `main`, `types`, `exports` (dist-based), and drops any `publishConfig.exports` override so the
 * top-level `exports` wins. Everything else — `files`, `dependencies`, `publishConfig.access` — is
 * preserved verbatim.
 */
export function preparePublishManifest(
  pkg: Record<string, unknown>,
  version: string,
): Record<string, unknown> {
  const next: Record<string, unknown> = {
    ...pkg,
    version,
    main: "./dist/index.js",
    types: "./dist/index.d.ts",
    exports: PUBLISHED_EXPORTS,
  };
  const publishConfig = pkg.publishConfig;
  if (publishConfig && typeof publishConfig === "object") {
    const { exports: _dropped, ...rest } = publishConfig as Record<string, unknown>;
    next.publishConfig = rest;
  }
  return next;
}

// CLI: `bun scripts/prepare-sdk-publish.ts <version> [package.json path]`. Version also accepted via
// $VERSION. Reads, rewrites in place, writes back with a trailing newline (matching the prior inline form).
if (import.meta.main) {
  const version = process.env.VERSION ?? process.argv[2];
  if (!version) {
    console.error("usage: prepare-sdk-publish.ts <version> [package.json path]  (or $VERSION)");
    process.exit(1);
  }
  const manifestPath = process.argv[3] ?? "package.json";
  const fs = await import("node:fs");
  const pkg = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  const next = preparePublishManifest(pkg, version);
  fs.writeFileSync(manifestPath, `${JSON.stringify(next, null, 2)}\n`);
  console.log(`prepared ${manifestPath} for publish as ${version}`);
}
