#!/usr/bin/env bun
/**
 * Download bb binaries for specific Aztec versions into the local version cache.
 *
 * Usage:
 *   bun scripts/download-bb.ts <version>[,<version>,...]
 *   bun scripts/download-bb.ts --list
 *
 * Downloads from Aztec GitHub releases into ~/.aztec-accelerator/versions/{version}/bb
 * (or BB_VERSIONS_DIR if set), then runs retention cleanup.
 *
 * SECURITY (F-007): this cache is runtime-trusted — the accelerator executes `bb` from it over the
 * private proving witness. So every download is verified against the GitHub release asset's published
 * SHA-256 digest, extracted into a PRIVATE per-run staging dir with archive-member safety checks,
 * ad-hoc re-signed (macOS), fingerprinted, and published atomically alongside a `bb.sha256.json` MARKER
 * (archive digest + final-binary digest). The Rust runtime rehashes the cached `bb` against that marker
 * on every use; a missing/tampered marker fails closed and re-downloads. Mirrors the fail-closed Rust
 * pipeline in packages/accelerator/core/src/versions/{downloader,release_metadata,cache_layout}.rs.
 *
 * Windows: bb.exe ships as a bundled Tauri sidecar via packages/accelerator/scripts/copy-bb.ts — NOT
 * this cache tool — so this script is Unix-only and refuses to run on win32.
 */
import { createHash, randomBytes } from "node:crypto";
import {
  chmodSync,
  closeSync,
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  openSync,
  readdirSync,
  readFileSync,
  readSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { gunzipSync } from "node:zlib";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

export const MARKER_SCHEMA = "aztec-accelerator/bb-cache-marker@1";
export const MARKER_NAME = "bb.sha256.json";
// bb's tarball is ~5 MiB; cap far above that so a compromised CDN can't OOM us before the digest
// mismatch is detected. Mirrors Rust MAX_DOWNLOAD_BYTES.
const MAX_BB_TARBALL_BYTES = 64 * 1024 * 1024;
// Decompressed ceiling — a ≤64 MB-compressed binary inflates to at most a few hundred MB. Applied as a
// CUMULATIVE cap on the whole decompressed tar (before extraction, defeating a gzip bomb) AND to the
// selected bb. Mirrors Rust MAX_DECOMPRESSED_BYTES.
const MAX_BB_BINARY_BYTES = 512 * 1024 * 1024;
const HEX64 = /^[0-9a-f]{64}$/;

export interface BbMarker {
  schema: string;
  version: string;
  platform: string;
  archive_sha256: string;
  binary_sha256: string;
}

// ---------------------------------------------------------------------------
// Platform detection — matches accelerator's current_platform() (release_metadata.rs)
// ---------------------------------------------------------------------------

export function currentPlatform(): string {
  const arch = process.arch === "arm64" ? "arm64" : "amd64";
  const os = process.platform === "darwin" ? "darwin" : "linux";
  return `${arch}-${os}`;
}

function assetName(): string {
  return `barretenberg-${currentPlatform()}.tar.gz`;
}

export function downloadUrl(version: string): string {
  return `https://github.com/AztecProtocol/aztec-packages/releases/download/v${version}/${assetName()}`;
}

// ---------------------------------------------------------------------------
// Version validation — mirror of Rust is_valid_version (version_policy.rs): the single
// path-traversal/injection guard before a version string is used to build a cache path.
// ---------------------------------------------------------------------------

export function isValidVersionName(version: string): boolean {
  return (
    version.length > 0 &&
    version.length <= 128 &&
    !version.startsWith(".") &&
    !version.endsWith(".") &&
    !version.includes("..") &&
    /^[A-Za-z0-9._-]+$/.test(version)
  );
}

export function assertValidVersion(version: string): void {
  if (!isValidVersionName(version)) {
    throw new Error(
      `Invalid version string ${JSON.stringify(version)} — rejected by the path-traversal/injection guard`,
    );
  }
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

export function versionsBaseDir(): string {
  return process.env.BB_VERSIONS_DIR || join(homedir(), ".aztec-accelerator", "versions");
}
function versionDir(version: string): string {
  return join(versionsBaseDir(), version);
}
export function versionBbPath(version: string): string {
  return join(versionDir(version), "bb");
}
export function versionMarkerPath(version: string): string {
  return join(versionDir(version), MARKER_NAME);
}

// ---------------------------------------------------------------------------
// Integrity: digest lookup, hashing, marker read + cached-bb verification
// ---------------------------------------------------------------------------

export function sha256Hex(data: Uint8Array): string {
  return createHash("sha256").update(data).digest("hex");
}

/** Streamed SHA-256 of a file (chunked reads — never buffers the whole binary). Mirrors Rust sha256_file. */
export function sha256File(path: string): string {
  const hash = createHash("sha256");
  const fd = openSync(path, "r");
  try {
    const buf = Buffer.allocUnsafe(64 * 1024);
    let n: number;
    while ((n = readSync(fd, buf, 0, buf.length, null)) > 0) {
      hash.update(buf.subarray(0, n));
    }
  } finally {
    closeSync(fd);
  }
  return hash.digest("hex");
}

/**
 * Fetch the expected SHA-256 for a release asset from the GitHub API. Mirrors
 * release_metadata.rs::fetch_github_asset_digest, but FAIL-CLOSED at the call site: a non-2xx, a
 * missing asset/digest, or a malformed digest THROWS (the Rust helper returns Ok(None) and its caller
 * throws — same net behavior). Honors GITHUB_TOKEN to dodge the 60/hr unauth rate limit.
 */
export async function fetchAssetDigest(version: string, asset: string): Promise<string> {
  const apiUrl = `https://api.github.com/repos/AztecProtocol/aztec-packages/releases/tags/v${version}`;
  const headers: Record<string, string> = {
    accept: "application/vnd.github+json",
    "user-agent": "aztec-accelerator",
  };
  if (process.env.GITHUB_TOKEN) headers.authorization = `Bearer ${process.env.GITHUB_TOKEN}`;

  const res = await fetch(apiUrl, { headers });
  if (!res.ok) {
    throw new Error(`Cannot verify bb v${version}: release metadata HTTP ${res.status} ${res.statusText}`);
  }
  const release = (await res.json()) as { assets?: Array<{ name?: string; digest?: string }> };
  const found = (release.assets ?? []).find((a) => a.name === asset);
  if (!found?.digest) {
    throw new Error(`Cannot verify bb v${version}: no digest for asset ${asset} in release metadata`);
  }
  const hex = found.digest.startsWith("sha256:") ? found.digest.slice("sha256:".length) : "";
  if (!HEX64.test(hex)) {
    throw new Error(`Cannot verify bb v${version}: malformed asset digest ${JSON.stringify(found.digest)}`);
  }
  return hex;
}

/** Read + structurally validate a cached version's marker. Returns null on any defect (fail-closed). */
export function readMarker(version: string): BbMarker | null {
  const p = versionMarkerPath(version);
  if (!existsSync(p)) return null;
  try {
    // Bounded read — a marker is a few hundred bytes; refuse an oversized blob.
    if (statSync(p).size > 4096) return null;
    const m = JSON.parse(readFileSync(p, "utf8")) as BbMarker;
    if (m.schema !== MARKER_SCHEMA) return null;
    if (m.version !== version) return null;
    if (m.platform !== currentPlatform()) return null;
    if (!HEX64.test(m.archive_sha256) || !HEX64.test(m.binary_sha256)) return null;
    return m;
  } catch {
    return null;
  }
}

/** True iff the cached bb for `version` is a present regular file whose bytes match its valid marker. */
export function verifyCachedBb(version: string): boolean {
  const bb = versionBbPath(version);
  if (!existsSync(bb)) return false;
  const st = lstatSync(bb);
  if (!st.isFile()) return false;
  const marker = readMarker(version);
  if (!marker) return false;
  return sha256File(bb) === marker.binary_sha256;
}

// ---------------------------------------------------------------------------
// Download (bounded streaming) + archive-member safety
// ---------------------------------------------------------------------------

/**
 * Bounded streaming download (mirror Rust download_tarball): read the body chunk-by-chunk with a
 * running byte cap. Never `arrayBuffer()` — that would buffer an unbounded body when Content-Length is
 * absent or lying, defeating the cap.
 */
export async function downloadTarball(version: string): Promise<Uint8Array> {
  const url = downloadUrl(version);
  const res = await fetch(url);
  if (!res.ok) {
    if (res.status === 404) {
      throw new Error(
        `Version ${version} not found (404). Check available releases at:\n` +
          `  https://github.com/AztecProtocol/aztec-packages/releases`,
      );
    }
    throw new Error(`Download failed: ${res.status} ${res.statusText}`);
  }
  const declared = Number(res.headers.get("content-length") ?? "0");
  if (declared > MAX_BB_TARBALL_BYTES) {
    throw new Error(`bb v${version} download too large (advertised ${declared} bytes, max ${MAX_BB_TARBALL_BYTES})`);
  }
  if (!res.body) throw new Error(`bb v${version}: empty response body`);

  const reader = res.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.length;
    if (total > MAX_BB_TARBALL_BYTES) {
      throw new Error(`bb v${version} download exceeded ${MAX_BB_TARBALL_BYTES} bytes — aborting`);
    }
    chunks.push(value);
  }
  const out = new Uint8Array(total);
  let off = 0;
  for (const c of chunks) {
    out.set(c, off);
    off += c.length;
  }
  return out;
}

/**
 * Locate exactly one regular file named `bb` in the extracted tree, rejecting unsafe members. Symlinks
 * anywhere in the tree are rejected (a symlink `bb` → /etc/passwd is the classic archive-escape). Zero
 * or multiple `bb` entries, a non-regular `bb`, or an oversized `bb` all fail closed. (Digest
 * verification already ran before extraction, so this is defense-in-depth against a compromised-upstream
 * archive whose digest was also forged.)
 */
export function findSingleBb(root: string): string {
  const found: string[] = [];
  const walk = (dir: string): void => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      const st = lstatSync(full);
      if (st.isSymbolicLink()) {
        throw new Error(`Unsafe archive member (symlink): ${entry.name}`);
      }
      if (st.isDirectory()) {
        walk(full);
        continue;
      }
      if (entry.name === "bb") {
        if (!st.isFile()) throw new Error(`Unsafe archive: bb entry is not a regular file`);
        // A tar hardlink extracts to an inode with nlink > 1 (linked to another extracted member); a
        // lone legitimate bb has nlink 1. Reject to match the Rust path's regular-file-only guarantee.
        if (st.nlink > 1) throw new Error(`Unsafe archive: bb is a hardlink (nlink=${st.nlink})`);
        if (st.size > MAX_BB_BINARY_BYTES) {
          throw new Error(`bb entry too large: ${st.size} bytes > ${MAX_BB_BINARY_BYTES}`);
        }
        found.push(full);
      }
    }
  };
  walk(root);
  const [bb, ...rest] = found;
  if (bb === undefined) throw new Error("bb binary not found in tarball");
  if (rest.length > 0) throw new Error(`Unsafe archive: ${found.length} bb entries (expected exactly 1)`);
  return bb;
}

// ---------------------------------------------------------------------------
// Staged, verified install
// ---------------------------------------------------------------------------

/** Remove crash-stale staging siblings for `version` (`.{version}.tmp.*`) before starting a fresh one. */
function reapStaleStages(version: string): void {
  const base = versionsBaseDir();
  if (!existsSync(base)) return;
  const prefix = `.${version}.tmp.`;
  for (const name of readdirSync(base)) {
    if (name.startsWith(prefix)) {
      rmSync(join(base, name), { recursive: true, force: true });
    }
  }
}

/**
 * Create a per-run-UNIQUE, dot-prefixed, owner-only staging sibling (`.{version}.tmp.<rand>`). Unique so
 * a concurrent Rust/TS publisher never shares (and stomps) one stage; dot-prefixed + non-version-named so
 * inventory never surfaces an in-flight or crash-stale stage. `mkdir` is strict (non-recursive) so the
 * astronomically-unlikely name collision fails closed rather than reusing a dir.
 */
function createStagingDir(version: string): string {
  mkdirSync(versionsBaseDir(), { recursive: true, mode: 0o700 });
  const stage = join(versionsBaseDir(), `.${version}.tmp.${randomBytes(6).toString("hex")}`);
  mkdirSync(stage, { mode: 0o700 });
  return stage;
}

/**
 * Download + verify + stage + finalize + marker + fail-closed publish for one version. Skips (verified)
 * if the cache already holds a marker-valid binary.
 */
export async function downloadBb(version: string): Promise<void> {
  assertValidVersion(version);

  if (verifyCachedBb(version)) {
    console.log(`  ✓ ${version} (already cached, verified)`);
    return;
  }

  console.log(`  ↓ ${version} — downloading + verifying...`);
  const archiveDigest = await fetchAssetDigest(version, assetName());
  const tarball = await downloadTarball(version);
  const actualArchive = sha256Hex(tarball);
  if (actualArchive !== archiveDigest) {
    throw new Error(
      `Integrity check failed for bb v${version}: expected sha256:${archiveDigest}, got sha256:${actualArchive}`,
    );
  }

  reapStaleStages(version);
  const stage = createStagingDir(version);
  try {
    // Extract into an isolated subdir so only the promoted `bb` + marker ever publish.
    const extract = join(stage, "extract");
    mkdirSync(extract, { mode: 0o700 });
    // Bound decompression BEFORE touching the fs: `gunzipSync` with a cumulative output cap aborts a
    // gzip bomb (system `tar -xz` would stream an unbounded bomb to disk). Then extract the bounded tar.
    let tarBytes: Buffer;
    try {
      tarBytes = gunzipSync(tarball, { maxOutputLength: MAX_BB_BINARY_BYTES });
    } catch (e) {
      throw new Error(
        `bb v${version}: decompression aborted (exceeds ${MAX_BB_BINARY_BYTES} bytes or is corrupt): ${e instanceof Error ? e.message : String(e)}`,
      );
    }
    const tarPath = join(stage, "archive.tar");
    writeFileSync(tarPath, tarBytes, { mode: 0o600 });
    const proc = Bun.spawnSync(["tar", "-xf", tarPath, "-C", extract]);
    if (!proc.success) throw new Error(`tar extraction failed (exit code ${proc.exitCode})`);

    const bbSrc = findSingleBb(extract);
    const stagedBb = join(stage, "bb");
    copyFileSync(bbSrc, stagedBb);
    chmodSync(stagedBb, 0o755);

    // macOS: clear quarantine + ad-hoc re-sign (chmod invalidates the original signature, and
    // Gatekeeper SIGKILLs an unsigned binary). Codesign failure ABORTS — we never publish an
    // unsignable bb. This mutates the bytes, so the marker fingerprint is taken AFTER it.
    if (process.platform === "darwin") {
      Bun.spawnSync(["xattr", "-cr", stagedBb]);
      const cs = Bun.spawnSync(["codesign", "--force", "--sign", "-", stagedBb]);
      if (!cs.success) {
        throw new Error(`codesign failed for bb v${version} (exit ${cs.exitCode})`);
      }
    }

    const binaryDigest = sha256File(stagedBb);
    const marker: BbMarker = {
      schema: MARKER_SCHEMA,
      version,
      platform: currentPlatform(),
      archive_sha256: archiveDigest,
      binary_sha256: binaryDigest,
    };
    writeFileSync(join(stage, MARKER_NAME), `${JSON.stringify(marker, null, 2)}\n`, { mode: 0o600 });

    // Drop the extract scratch + archive so the stage holds only { bb, bb.sha256.json }.
    rmSync(extract, { recursive: true, force: true });
    rmSync(tarPath, { force: true });

    // Fail-closed publish: remove any live entry, then rename the stage into place. A crash between
    // the two leaves NO live entry (⇒ verified re-download next use) plus a `.tmp.<rand>` stage
    // (reaped by the next install's stage-unique naming + cleanup). Deliberately NOT atomic replacement.
    const vdir = versionDir(version);
    if (existsSync(vdir)) rmSync(vdir, { recursive: true, force: true });
    renameSync(stage, vdir);
  } catch (err) {
    rmSync(stage, { recursive: true, force: true });
    throw err;
  }

  const sizeMb = (statSync(versionBbPath(version)).size / 1024 / 1024).toFixed(1);
  console.log(`  ✓ ${version} (${sizeMb} MB, verified)`);
}

// ---------------------------------------------------------------------------
// Inventory + retention
// ---------------------------------------------------------------------------

/**
 * List cached versions: a directory is "cached" only if its name is a valid version (skips
 * dot-prefixed `.tmp.<rand>` stages and junk) AND it holds a marker. Cheap stat only — never rehashes
 * (execution paths rehash; inventory does not).
 */
export function listCachedVersions(): string[] {
  const base = versionsBaseDir();
  if (!existsSync(base)) return [];
  const versions: string[] = [];
  for (const entry of readdirSync(base, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    if (!isValidVersionName(entry.name)) continue; // excludes `.{v}.tmp.<rand>` (leading dot)
    if (!existsSync(join(base, entry.name, MARKER_NAME))) continue;
    versions.push(entry.name);
  }
  return versions.sort();
}

export type NetworkTier = "nightly" | "devnet" | "testnet" | "mainnet";

const RETENTION_LIMITS: Record<NetworkTier, number | null> = {
  nightly: 2,
  devnet: 3,
  testnet: 5,
  mainnet: null,
};

export function classifyVersion(version: string): NetworkTier {
  const prerelease = version.split("-").slice(1).join("-");
  if (prerelease.startsWith("nightly")) return "nightly";
  if (prerelease.startsWith("devnet")) return "devnet";
  if (prerelease.startsWith("rc")) return "testnet";
  return "mainnet";
}

/**
 * Evict old cached versions per tier retention. `protectedVersions` (the versions downloaded in THIS
 * invocation) are never evicted — otherwise `download-bb.ts <old-nightly>` could delete the very
 * version it just fetched.
 */
export function cleanupOldVersions(protectedVersions: Set<string> = new Set()): void {
  const cached = listCachedVersions();
  const byTier = new Map<NetworkTier, string[]>();
  for (const v of cached) {
    const tier = classifyVersion(v);
    if (!byTier.has(tier)) byTier.set(tier, []);
    byTier.get(tier)!.push(v);
  }

  for (const [tier, versions] of byTier) {
    const limit = RETENTION_LIMITS[tier];
    if (limit === null) continue;
    const sorted = [...versions].sort(); // oldest first
    while (sorted.length > limit) {
      // Evict the oldest NON-protected version; stop if only protected ones remain.
      const idx = sorted.findIndex((v) => !protectedVersions.has(v));
      if (idx === -1) break;
      const [evict] = sorted.splice(idx, 1);
      rmSync(join(versionsBaseDir(), evict), { recursive: true, force: true });
      console.log(`  🗑 Evicted ${evict} (${tier} retention: keep ${limit})`);
    }
  }
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

async function main(): Promise<void> {
  if (process.platform === "win32") {
    console.error(
      "download-bb.ts is not supported on Windows.\n" +
        "The Windows bb.exe ships as a bundled Tauri sidecar via packages/accelerator/scripts/copy-bb.ts.",
    );
    process.exit(1);
  }

  const args = process.argv.slice(2);

  if (args.length === 0 || args.includes("--help") || args.includes("-h")) {
    console.log(`Usage: bun scripts/download-bb.ts <version>[,<version>,...] [--list]

Downloads + verifies bb binaries for specific Aztec versions.

Options:
  --list    List cached versions and exit

Cache: ${versionsBaseDir()}
Platform: ${currentPlatform()}`);
    process.exit(0);
  }

  if (args.includes("--list")) {
    const cached = listCachedVersions();
    console.log(`Cache: ${versionsBaseDir()}`);
    if (cached.length === 0) {
      console.log("No cached versions.");
    } else {
      console.log(`\n${cached.length} cached version(s):`);
      for (const v of cached) console.log(`  ${v} (${classifyVersion(v)})`);
    }
    process.exit(0);
  }

  const versions = args
    .flatMap((a) => a.split(","))
    .map((v) => v.trim())
    .filter(Boolean);

  if (versions.length === 0) {
    console.error("Error: no versions specified");
    process.exit(1);
  }

  console.log(`Downloading bb for ${versions.length} version(s) [${currentPlatform()}]`);
  console.log(`Cache: ${versionsBaseDir()}\n`);

  let failed = false;
  const downloaded = new Set<string>();
  for (const version of versions) {
    try {
      await downloadBb(version);
      downloaded.add(version);
    } catch (err) {
      console.error(`  ✗ ${version}: ${err instanceof Error ? err.message : String(err)}`);
      failed = true;
    }
  }

  console.log("");
  cleanupOldVersions(downloaded);

  const cached = listCachedVersions();
  console.log(`\nCached versions: ${cached.join(", ") || "(none)"}`);

  if (failed) process.exit(1);
}

if (import.meta.main) {
  await main();
}
