#!/usr/bin/env bun
/**
 * Download bb binaries for specific Aztec versions into the local version cache.
 *
 * Usage:
 *   bun scripts/download-bb.ts <version>[,<version>,...]
 *   bun scripts/download-bb.ts --list
 *
 * Examples:
 *   bun scripts/download-bb.ts 5.0.0-nightly.20260305
 *   bun scripts/download-bb.ts 5.0.0-nightly.20260305,5.0.0-rc.1
 *   bun scripts/download-bb.ts --list
 *
 * Downloads from Aztec GitHub releases into ~/.aztec-accelerator/versions/{version}/bb
 * (or BB_VERSIONS_DIR if set). Runs retention cleanup after download.
 */
import { chmodSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

// ---------------------------------------------------------------------------
// Platform detection — matches accelerator's current_platform() (versions.rs)
// Aztec release naming: arm64-darwin, amd64-darwin, amd64-linux, arm64-linux
// ---------------------------------------------------------------------------

function currentPlatform(): string {
  const arch = process.arch === "arm64" ? "arm64" : "amd64";
  const os = process.platform === "darwin" ? "darwin" : "linux";
  return `${arch}-${os}`;
}

function downloadUrl(version: string): string {
  return `https://github.com/AztecProtocol/aztec-packages/releases/download/v${version}/barretenberg-${currentPlatform()}.tar.gz`;
}

// ---------------------------------------------------------------------------
// Paths — same as packages/accelerator/src-tauri/src/versions.rs
// ---------------------------------------------------------------------------

function versionsBaseDir(): string {
  return process.env.BB_VERSIONS_DIR || join(homedir(), ".aztec-accelerator", "versions");
}

function versionBbPath(version: string): string {
  return join(versionsBaseDir(), version, "bb");
}

function listCachedVersions(): string[] {
  const base = versionsBaseDir();
  if (!existsSync(base)) return [];
  const { readdirSync } = require("node:fs");
  const versions: string[] = [];
  for (const entry of readdirSync(base, { withFileTypes: true })) {
    if (entry.isDirectory() && existsSync(join(base, entry.name, "bb"))) {
      versions.push(entry.name);
    }
  }
  return versions.sort();
}

// ---------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------

async function downloadBb(version: string): Promise<void> {
  const bbPath = versionBbPath(version);

  if (existsSync(bbPath)) {
    console.log(`  ✓ ${version} (already cached)`);
    return;
  }

  const url = downloadUrl(version);
  console.log(`  ↓ ${version} — downloading from GitHub releases...`);

  const response = await fetch(url);
  if (!response.ok) {
    if (response.status === 404) {
      throw new Error(
        `Version ${version} not found (404). Check available releases at:\n` +
          `  https://github.com/AztecProtocol/aztec-packages/releases`,
      );
    }
    throw new Error(`Download failed: ${response.status} ${response.statusText}`);
  }

  const tarball = await response.arrayBuffer();
  const versionDir = join(versionsBaseDir(), version);
  mkdirSync(versionDir, { recursive: true });

  // Extract tarball — bb binary is at the root of the archive
  const proc = Bun.spawn(["tar", "-xzf", "-", "-C", versionDir], {
    stdin: new Uint8Array(tarball),
  });
  const exitCode = await proc.exited;
  if (exitCode !== 0) {
    rmSync(versionDir, { recursive: true, force: true });
    throw new Error(`tar extraction failed (exit code ${exitCode})`);
  }

  if (!existsSync(bbPath)) {
    rmSync(versionDir, { recursive: true, force: true });
    throw new Error(`bb binary not found after extraction at ${bbPath}`);
  }

  chmodSync(bbPath, 0o755);

  // macOS: clear quarantine attribute and ad-hoc re-sign.
  // GitHub release binaries have ad-hoc signatures that get invalidated during
  // download/extraction, causing Gatekeeper to silently block execution (hang).
  if (process.platform === "darwin") {
    Bun.spawnSync(["xattr", "-d", "com.apple.quarantine", bbPath]);
    Bun.spawnSync(["codesign", "--force", "--sign", "-", bbPath]);
  }

  const stat = Bun.file(bbPath);
  const sizeMb = ((await stat.size) / 1024 / 1024).toFixed(1);
  console.log(`  ✓ ${version} (${sizeMb} MB)`);
}

// ---------------------------------------------------------------------------
// Retention cleanup — same logic as bb-versions.ts
// ---------------------------------------------------------------------------

type NetworkTier = "nightly" | "devnet" | "testnet" | "mainnet";

const RETENTION_LIMITS: Record<NetworkTier, number | null> = {
  nightly: 2,
  devnet: 3,
  testnet: 5,
  mainnet: null,
};

function classifyVersion(version: string): NetworkTier {
  const prerelease = version.split("-").slice(1).join("-");
  if (prerelease.startsWith("nightly")) return "nightly";
  if (prerelease.startsWith("devnet")) return "devnet";
  if (prerelease.startsWith("rc")) return "testnet";
  return "mainnet";
}

function cleanupOldVersions(): void {
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
    const sorted = [...versions].sort();
    while (sorted.length > limit) {
      const evict = sorted.shift()!;
      const dir = join(versionsBaseDir(), evict);
      rmSync(dir, { recursive: true, force: true });
      console.log(`  🗑 Evicted ${evict} (${tier} retention: keep ${limit})`);
    }
  }
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

const args = process.argv.slice(2);

if (args.length === 0 || args.includes("--help") || args.includes("-h")) {
  console.log(`Usage: bun scripts/download-bb.ts <version>[,<version>,...] [--list]

Downloads bb binaries for specific Aztec versions.

Options:
  --list    List cached versions and exit

Examples:
  bun scripts/download-bb.ts 5.0.0-nightly.20260305
  bun scripts/download-bb.ts 5.0.0-nightly.20260305,5.0.0-devnet.1
  bun scripts/download-bb.ts --list

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
    for (const v of cached) {
      const tier = classifyVersion(v);
      console.log(`  ${v} (${tier})`);
    }
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
for (const version of versions) {
  try {
    await downloadBb(version);
  } catch (err) {
    console.error(`  ✗ ${version}: ${err instanceof Error ? err.message : String(err)}`);
    failed = true;
  }
}

console.log("");
cleanupOldVersions();

const cached = listCachedVersions();
console.log(`\nCached versions: ${cached.join(", ") || "(none)"}`);

if (failed) process.exit(1);
