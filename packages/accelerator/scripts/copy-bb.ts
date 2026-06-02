/**
 * Extract the `bb` binary from `@aztec/bb.js` and copy it to `src-tauri/binaries/`
 * as a Tauri sidecar with the correct target-triple suffix.
 *
 * Tauri expects sidecars at `binaries/<name>-<target-triple>` (plus `.exe` on Windows).
 *
 * - macOS/Linux: bb ships inside the `@aztec/bb.js` npm package (`build/<arch>-<os>/bb`).
 * - Windows: bb.js ships NO Windows build, so we fetch the self-contained `bb.exe`
 *   from the matching aztec-packages GitHub release tarball and verify it against a
 *   pinned SHA-256. Upstream publishes no checksum file, so this in-repo, review-gated
 *   pin is the supply-chain integrity anchor.
 */
import { execSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

// --- Map to Tauri target triple ---

export function getTargetTriple(): string {
  const platform = process.platform;
  const nodeArch = process.arch;

  if (platform === "darwin") {
    return nodeArch === "arm64" ? "aarch64-apple-darwin" : "x86_64-apple-darwin";
  }
  if (platform === "linux") {
    return nodeArch === "arm64" ? "aarch64-unknown-linux-gnu" : "x86_64-unknown-linux-gnu";
  }
  if (platform === "win32") {
    // x64 only (locked scope). bb.exe is x86_64; arm64-windows is not shipped.
    return "x86_64-pc-windows-msvc";
  }
  throw new Error(`Unsupported platform: ${platform}`);
}

// --- Windows bb.exe supply chain ---
// The Windows bb.exe is fetched from the aztec-packages release whose tag matches the
// LIVE @aztec/bb.js version (never the committed AZTEC_VERSION file, which can drift).
// Each version's tarball SHA-256 is pinned below; the prebuild fails closed on an
// unknown version or a hash mismatch — both force a deliberate review whenever bb bumps.

export const WINDOWS_BB_ASSET = "barretenberg-amd64-windows.tar.gz";

export const WINDOWS_BB_CHECKSUMS: Record<string, string> = {
  // @aztec/bb.js 4.2.0 — verified on windows-latest via the windows-bb-spike workflow.
  "4.2.0": "55043d74d20afd55cb3d3c5fd690b79f9d964ba52bfebd13bcba71b74a3d0c8f",
};

export function windowsBbReleaseTag(version: string): string {
  return `v${version}`;
}

export function resolveWindowsBbChecksum(version: string): string {
  const sha = WINDOWS_BB_CHECKSUMS[version];
  if (!sha) {
    throw new Error(
      `No pinned Windows bb.exe SHA-256 for @aztec/bb.js ${version}.\n` +
        `Download ${WINDOWS_BB_ASSET} from the v${version} aztec-packages release ` +
        `(or run the windows-bb-spike workflow), then add its sha256 to ` +
        `WINDOWS_BB_CHECKSUMS in copy-bb.ts.`,
    );
  }
  return sha;
}

export function assertSha256(data: Uint8Array, expected: string, label: string): void {
  const actual = createHash("sha256").update(data).digest("hex");
  if (actual !== expected) {
    throw new Error(`SHA-256 mismatch for ${label}: expected ${expected}, got ${actual}`);
  }
}

async function fetchWindowsBb(version: string, destExe: string): Promise<void> {
  const tag = windowsBbReleaseTag(version);
  const expected = resolveWindowsBbChecksum(version);
  const url = `https://github.com/AztecProtocol/aztec-packages/releases/download/${tag}/${WINDOWS_BB_ASSET}`;

  console.log(`Fetching Windows bb.exe: ${url}`);
  const res = await fetch(url);
  if (!res.ok) {
    throw new Error(`Failed to download ${url}: HTTP ${res.status} ${res.statusText}`);
  }
  const data = new Uint8Array(await res.arrayBuffer());
  assertSha256(data, expected, WINDOWS_BB_ASSET);
  console.log(`SHA-256 verified: ${expected}`);

  const work = mkdtempSync(join(tmpdir(), "bb-win-"));
  try {
    const tarPath = join(work, WINDOWS_BB_ASSET);
    writeFileSync(tarPath, data);
    // bsdtar ships in System32 on Windows 10+, the only platform this branch runs on.
    execSync(`tar -xzf "${tarPath}" -C "${work}"`, { stdio: "inherit" });
    const extracted = join(work, "bb.exe");
    if (!existsSync(extracted)) {
      throw new Error(`bb.exe not found after extracting ${WINDOWS_BB_ASSET}`);
    }
    copyFileSync(extracted, destExe);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

async function main(): Promise<void> {
  // Resolve @aztec/bb.js two ways: bb-prover is a direct sdk dep; bb.js is its dep.
  const sdkDir = join(import.meta.dirname!, "..", "..", "sdk");
  const bbProverEntry = Bun.resolveSync("@aztec/bb-prover", sdkDir);
  const bbJsPkgJson = Bun.resolveSync("@aztec/bb.js/package.json", dirname(bbProverEntry));
  const bbJsRoot = dirname(bbJsPkgJson);
  // The LIVE version drives both the npm build dir AND the Windows release tag.
  const aztecVersion: string = JSON.parse(readFileSync(bbJsPkgJson, "utf8")).version;

  const platform = process.platform;
  const triple = getTargetTriple();
  const ext = platform === "win32" ? ".exe" : "";
  const binariesDir = join(import.meta.dirname!, "..", "src-tauri", "binaries");
  const dest = join(binariesDir, `bb-${triple}${ext}`);
  mkdirSync(binariesDir, { recursive: true });

  if (platform === "win32") {
    await fetchWindowsBb(aztecVersion, dest);
  } else {
    const arch = process.arch === "arm64" ? "arm64" : "amd64";
    const os = platform === "darwin" ? "macos" : "linux";
    const bbSource = join(bbJsRoot, "build", `${arch}-${os}`, "bb");
    if (!existsSync(bbSource)) {
      console.error(`bb binary not found at ${bbSource}`);
      process.exit(1);
    }
    copyFileSync(bbSource, dest);
    chmodSync(dest, 0o755);
    // Remove the macOS quarantine attribute (prevents Gatekeeper from killing the binary).
    if (platform === "darwin") {
      try {
        execSync(`xattr -d com.apple.quarantine "${dest}"`, { stdio: "ignore" });
      } catch {
        // Attribute may not exist, that's fine.
      }
    }
  }

  // Write the Aztec version for build.rs (self-heals a stale committed AZTEC_VERSION).
  writeFileSync(join(import.meta.dirname!, "..", "src-tauri", "AZTEC_VERSION"), aztecVersion);

  console.log(`Copied bb -> ${dest} (from ${bbJsRoot})`);
  console.log(`Aztec bb version: ${aztecVersion}`);
}

if (import.meta.main) {
  await main();
}
