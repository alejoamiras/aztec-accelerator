import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  assertValidVersion,
  type BbMarker,
  cleanupOldVersions,
  currentPlatform,
  downloadBb,
  downloadTarball,
  fetchAssetDigest,
  findSingleBb,
  isValidVersionName,
  listCachedVersions,
  MARKER_NAME,
  MARKER_SCHEMA,
  readMarker,
  sha256Hex,
  verifyCachedBb,
  versionBbPath,
  versionMarkerPath,
} from "./download-bb";

// These tests exercise the exported functions on the host platform (Linux CI = amd64-linux, matching
// the committed fixtures). The CLI itself refuses to run on Windows; the functions are platform-generic.

const sha = (s: string): string => createHash("sha256").update(s).digest("hex");

let base: string;
const origFetch = globalThis.fetch;

beforeEach(() => {
  base = mkdtempSync(join(tmpdir(), "bbcache-"));
  process.env.BB_VERSIONS_DIR = base;
});
afterEach(() => {
  globalThis.fetch = origFetch;
  delete process.env.BB_VERSIONS_DIR;
  rmSync(base, { recursive: true, force: true });
});

/** Build a gzipped tar containing `barretenberg/bb` (+ optional unsafe members). */
function buildTarball(
  bbContent: string,
  opts: { symlink?: boolean; extraBb?: boolean } = {},
): Uint8Array {
  const work = mkdtempSync(join(tmpdir(), "bbtar-"));
  try {
    const inner = join(work, "barretenberg");
    mkdirSync(inner, { recursive: true });
    if (opts.symlink) {
      symlinkSync("/etc/passwd", join(inner, "bb"));
    } else {
      writeFileSync(join(inner, "bb"), bbContent);
    }
    if (opts.extraBb) {
      const other = join(work, "other");
      mkdirSync(other, { recursive: true });
      writeFileSync(join(other, "bb"), "second-bb");
    }
    const tgz = join(work, "a.tgz");
    const dirs = opts.extraBb ? ["barretenberg", "other"] : ["barretenberg"];
    const p = Bun.spawnSync(["tar", "-czf", tgz, "-C", work, ...dirs]);
    if (!p.success) throw new Error("test tar build failed");
    return new Uint8Array(readFileSync(tgz));
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

// Minimal fetch fakes — download-bb only touches the fields below.
function jsonResp(status: number, body: unknown) {
  return { ok: status >= 200 && status < 300, status, statusText: "s", json: async () => body };
}
function streamResp(bytes: Uint8Array, o: { status?: number; contentLength?: string | null } = {}) {
  const status = o.status ?? 200;
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText: "s",
    headers: {
      get: (k: string) =>
        k.toLowerCase() === "content-length"
          ? o.contentLength === undefined
            ? String(bytes.length)
            : o.contentLength
          : null,
    },
    body: new Response(bytes).body,
  };
}
function routeFetch(fn: (url: string) => unknown): void {
  globalThis.fetch = ((input: unknown) => Promise.resolve(fn(String(input)))) as typeof fetch;
}

/** Wire fetch so the api.github.com digest lookup + the release download both resolve for `version`. */
function wireHappyFetch(version: string, tarball: Uint8Array): void {
  const asset = `barretenberg-${currentPlatform()}.tar.gz`;
  const digest = sha256Hex(tarball);
  routeFetch((url) => {
    if (url.includes("api.github.com")) {
      return jsonResp(200, { assets: [{ name: asset, digest: `sha256:${digest}` }] });
    }
    return streamResp(tarball);
  });
}

/** Write a valid { bb, marker } cache entry by hand (bypassing the download path). */
function writeCacheEntry(version: string, bbContent: string, markerOverride: Partial<BbMarker> = {}): void {
  const dir = join(base, version);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "bb"), bbContent);
  const marker: BbMarker = {
    schema: MARKER_SCHEMA,
    version,
    platform: currentPlatform(),
    archive_sha256: sha("archive"),
    binary_sha256: sha(bbContent),
    ...markerOverride,
  };
  writeFileSync(versionMarkerPath(version), `${JSON.stringify(marker, null, 2)}\n`);
}

// ---------------------------------------------------------------------------

describe("assertValidVersion / isValidVersionName (mirrors Rust is_valid_version)", () => {
  test("accepts real versions", () => {
    for (const v of ["5.0.0", "5.0.0-rc.2", "5.0.0-nightly.20260307", "4.2.0-aztecnr-rc.2"]) {
      expect(isValidVersionName(v)).toBe(true);
      expect(() => assertValidVersion(v)).not.toThrow();
    }
  });
  test("rejects traversal + injection", () => {
    for (const v of ["", ".hidden", "5.0.0.", "a..b", "../etc", "5.0.0/bb", "a b", "a;b", "x".repeat(129)]) {
      expect(isValidVersionName(v)).toBe(false);
      expect(() => assertValidVersion(v)).toThrow();
    }
  });
});

describe("readMarker / verifyCachedBb", () => {
  test("valid entry verifies", () => {
    writeCacheEntry("5.0.0-rc.2", "the-bb-bytes");
    expect(verifyCachedBb("5.0.0-rc.2")).toBe(true);
    expect(readMarker("5.0.0-rc.2")?.schema).toBe(MARKER_SCHEMA);
  });
  test("missing marker ⇒ false", () => {
    const dir = join(base, "5.0.0-rc.2");
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "bb"), "x");
    expect(verifyCachedBb("5.0.0-rc.2")).toBe(false);
  });
  test("unknown schema ⇒ reject", () => {
    writeCacheEntry("5.0.0-rc.2", "x", { schema: "other@9" });
    expect(readMarker("5.0.0-rc.2")).toBeNull();
    expect(verifyCachedBb("5.0.0-rc.2")).toBe(false);
  });
  test("version / platform mismatch ⇒ reject", () => {
    writeCacheEntry("5.0.0-rc.2", "x", { version: "9.9.9" });
    expect(verifyCachedBb("5.0.0-rc.2")).toBe(false);
    writeCacheEntry("5.0.0-rc.3", "x", { platform: "arm64-solaris" });
    expect(verifyCachedBb("5.0.0-rc.3")).toBe(false);
  });
  test("noncanonical hex ⇒ reject", () => {
    writeCacheEntry("5.0.0-rc.2", "x", { binary_sha256: "NOTHEX" });
    expect(verifyCachedBb("5.0.0-rc.2")).toBe(false);
  });
  test("tampered binary (hash mismatch) ⇒ reject", () => {
    writeCacheEntry("5.0.0-rc.2", "original");
    writeFileSync(versionBbPath("5.0.0-rc.2"), "tampered");
    expect(verifyCachedBb("5.0.0-rc.2")).toBe(false);
  });
  test("oversized marker ⇒ reject", () => {
    writeCacheEntry("5.0.0-rc.2", "x");
    writeFileSync(versionMarkerPath("5.0.0-rc.2"), " ".repeat(5000));
    expect(readMarker("5.0.0-rc.2")).toBeNull();
  });
});

describe("fetchAssetDigest (fail-closed, mirrors release_metadata.rs)", () => {
  const asset = `barretenberg-${currentPlatform()}.tar.gz`;
  test("returns the hex on a valid release", async () => {
    routeFetch(() => jsonResp(200, { assets: [{ name: asset, digest: `sha256:${sha("t")}` }] }));
    expect(await fetchAssetDigest("5.0.0-rc.2", asset)).toBe(sha("t"));
  });
  test("non-2xx ⇒ throws", async () => {
    routeFetch(() => jsonResp(404, {}));
    await expect(fetchAssetDigest("5.0.0-rc.2", asset)).rejects.toThrow();
  });
  test("missing asset ⇒ throws", async () => {
    routeFetch(() => jsonResp(200, { assets: [{ name: "other", digest: `sha256:${sha("t")}` }] }));
    await expect(fetchAssetDigest("5.0.0-rc.2", asset)).rejects.toThrow();
  });
  test("malformed digest ⇒ throws", async () => {
    routeFetch(() => jsonResp(200, { assets: [{ name: asset, digest: "sha256:xyz" }] }));
    await expect(fetchAssetDigest("5.0.0-rc.2", asset)).rejects.toThrow();
  });
});

describe("downloadTarball (bounded streaming)", () => {
  test("404 ⇒ throws", async () => {
    routeFetch(() => streamResp(new Uint8Array(0), { status: 404 }));
    await expect(downloadTarball("5.0.0-rc.2")).rejects.toThrow();
  });
  test("declared oversize Content-Length ⇒ throws before streaming", async () => {
    routeFetch(() => streamResp(new Uint8Array([1, 2, 3]), { contentLength: "70000000" }));
    await expect(downloadTarball("5.0.0-rc.2")).rejects.toThrow();
  });
  test("normal body streams through", async () => {
    const bytes = new Uint8Array([1, 2, 3, 4]);
    routeFetch(() => streamResp(bytes));
    expect(await downloadTarball("5.0.0-rc.2")).toEqual(bytes);
  });
});

describe("findSingleBb (archive-member safety)", () => {
  test("finds a single regular bb", () => {
    const root = mkdtempSync(join(tmpdir(), "ex-"));
    mkdirSync(join(root, "barretenberg"));
    writeFileSync(join(root, "barretenberg", "bb"), "x");
    expect(findSingleBb(root)).toBe(join(root, "barretenberg", "bb"));
    rmSync(root, { recursive: true, force: true });
  });
  test("rejects a symlink member", () => {
    const root = mkdtempSync(join(tmpdir(), "ex-"));
    symlinkSync("/etc/passwd", join(root, "bb"));
    expect(() => findSingleBb(root)).toThrow(/symlink/);
    rmSync(root, { recursive: true, force: true });
  });
  test("rejects multiple bb entries", () => {
    const root = mkdtempSync(join(tmpdir(), "ex-"));
    mkdirSync(join(root, "a"));
    mkdirSync(join(root, "b"));
    writeFileSync(join(root, "a", "bb"), "1");
    writeFileSync(join(root, "b", "bb"), "2");
    expect(() => findSingleBb(root)).toThrow(/bb entries/);
    rmSync(root, { recursive: true, force: true });
  });
  test("rejects when no bb present", () => {
    const root = mkdtempSync(join(tmpdir(), "ex-"));
    writeFileSync(join(root, "notbb"), "x");
    expect(() => findSingleBb(root)).toThrow(/not found/);
    rmSync(root, { recursive: true, force: true });
  });
});

describe("downloadBb end-to-end", () => {
  test("verifies, publishes bb + marker, and is a verified cache entry", async () => {
    const v = "5.0.0-rc.2";
    const tarball = buildTarball("real-bb-binary");
    wireHappyFetch(v, tarball);
    await downloadBb(v);

    expect(readFileSync(versionBbPath(v), "utf8")).toBe("real-bb-binary");
    expect(existsSync(versionMarkerPath(v))).toBe(true);
    expect(verifyCachedBb(v)).toBe(true);
    expect(readMarker(v)?.binary_sha256).toBe(sha("real-bb-binary"));
  });

  test("skips (no fetch) when a valid marker already exists", async () => {
    const v = "5.0.0-rc.2";
    writeCacheEntry(v, "already-here");
    routeFetch(() => {
      throw new Error("fetch must not be called on a verified skip");
    });
    await expect(downloadBb(v)).resolves.toBeUndefined();
    expect(readFileSync(versionBbPath(v), "utf8")).toBe("already-here");
  });

  test("digest mismatch ⇒ throws and publishes nothing", async () => {
    const v = "5.0.0-rc.2";
    const tarball = buildTarball("real-bb-binary");
    const asset = `barretenberg-${currentPlatform()}.tar.gz`;
    routeFetch((url) => {
      if (url.includes("api.github.com")) {
        return jsonResp(200, { assets: [{ name: asset, digest: `sha256:${sha("WRONG")}` }] });
      }
      return streamResp(tarball);
    });
    await expect(downloadBb(v)).rejects.toThrow(/Integrity check failed/);
    expect(existsSync(join(base, v))).toBe(false); // no live entry, no leftover
  });

  test("legacy/tampered entry ⇒ re-downloads to a verified state", async () => {
    const v = "5.0.0-rc.2";
    writeCacheEntry(v, "original");
    writeFileSync(versionBbPath(v), "tampered"); // now marker-invalid
    expect(verifyCachedBb(v)).toBe(false);

    const tarball = buildTarball("fresh-bb");
    wireHappyFetch(v, tarball);
    await downloadBb(v);
    expect(verifyCachedBb(v)).toBe(true);
    expect(readFileSync(versionBbPath(v), "utf8")).toBe("fresh-bb");
  });

  test("rejects an unsafe (symlink) archive without publishing", async () => {
    const v = "5.0.0-rc.2";
    const tarball = buildTarball("", { symlink: true });
    wireHappyFetch(v, tarball);
    await expect(downloadBb(v)).rejects.toThrow(/symlink/);
    expect(existsSync(join(base, v))).toBe(false);
  });
});

describe("listCachedVersions (inventory excludes stages + unmarked)", () => {
  test("lists only valid-named, marker-bearing dirs", () => {
    writeCacheEntry("5.0.0-rc.2", "x"); // valid + marked
    mkdirSync(join(base, "5.0.0-rc.3"), { recursive: true }); // unmarked
    writeFileSync(join(base, "5.0.0-rc.3", "bb"), "y");
    // a crash-stale staging dir — dot-prefixed, WITH a marker inside — must NOT be listed
    const stage = join(base, ".5.0.0-rc.9.tmp.deadbeef");
    mkdirSync(stage, { recursive: true });
    writeFileSync(join(stage, MARKER_NAME), "{}");
    expect(listCachedVersions()).toEqual(["5.0.0-rc.2"]);
  });
});

describe("cleanupOldVersions", () => {
  test("never evicts a protected (this-invocation) version", () => {
    for (const v of ["5.0.0-nightly.20260301", "5.0.0-nightly.20260302", "5.0.0-nightly.20260303"]) {
      writeCacheEntry(v, "x");
    }
    // nightly limit is 2; protect the OLDEST so a non-protected one is evicted instead.
    cleanupOldVersions(new Set(["5.0.0-nightly.20260301"]));
    const left = listCachedVersions();
    expect(left).toContain("5.0.0-nightly.20260301"); // protected survived
    expect(left.length).toBe(2);
  });
});

describe("cross-language contract fixtures", () => {
  test("bb-cache-marker.json matches the marker schema", () => {
    const m = JSON.parse(readFileSync(join(import.meta.dir, "__fixtures__", "bb-cache-marker.json"), "utf8"));
    expect(m.schema).toBe(MARKER_SCHEMA);
    for (const f of [m.archive_sha256, m.binary_sha256]) expect(f).toMatch(/^[0-9a-f]{64}$/);
    expect(typeof m.version).toBe("string");
    expect(typeof m.platform).toBe("string");
  });
  test("github-release-metadata.json has sha256-prefixed asset digests", () => {
    const r = JSON.parse(
      readFileSync(join(import.meta.dir, "__fixtures__", "github-release-metadata.json"), "utf8"),
    );
    expect(Array.isArray(r.assets)).toBe(true);
    for (const a of r.assets) expect(a.digest).toMatch(/^sha256:[0-9a-f]{64}$/);
  });
});
