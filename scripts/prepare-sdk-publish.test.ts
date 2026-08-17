import { describe, expect, test } from "bun:test";
import { PUBLISHED_EXPORTS, preparePublishManifest } from "./prepare-sdk-publish.ts";

describe("preparePublishManifest (B7 SDK publish rewrite)", () => {
  const src = {
    name: "@alejoamiras/aztec-accelerator",
    version: "0.0.0",
    exports: "./src/index.ts",
    files: ["src", "dist", ".claude", "MIGRATION.md"],
    dependencies: { "@aztec/stdlib": "5.0.1" },
    publishConfig: { access: "public", exports: "./src/index.ts" },
  };

  test("repoints main/types/exports at dist and sets the resolved version", () => {
    const out = preparePublishManifest(src, "5.0.1-revision.2");
    expect(out.version).toBe("5.0.1-revision.2");
    expect(out.main).toBe("./dist/index.js");
    expect(out.types).toBe("./dist/index.d.ts");
    expect(out.exports).toEqual(PUBLISHED_EXPORTS);
  });

  test("drops publishConfig.exports, keeps access, preserves files + deps", () => {
    const out = preparePublishManifest(src, "1.0.0");
    expect((out.publishConfig as Record<string, unknown>).exports).toBeUndefined();
    expect((out.publishConfig as Record<string, unknown>).access).toBe("public");
    // files (incl. MIGRATION.md) and deps must survive verbatim — the tarball's contents depend on it.
    expect(out.files).toEqual(src.files);
    expect(out.dependencies).toEqual(src.dependencies);
  });

  test("does not mutate the input manifest", () => {
    const copy = structuredClone(src);
    preparePublishManifest(src, "9.9.9");
    expect(src).toEqual(copy);
  });
});
