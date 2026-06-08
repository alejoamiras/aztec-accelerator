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
import { execFileSync, execSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
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
  // @aztec/bb.js 4.2.0 — verified on windows-latest (Windows Prebuild Smoke CI gate).
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
        `Download ${WINDOWS_BB_ASSET} from the v${version} aztec-packages release, ` +
        `sha256sum it, and add the hash to WINDOWS_BB_CHECKSUMS in copy-bb.ts.`,
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

// bb's tarball is ~5 MiB; cap well above that to bound build-time memory if a
// compromised CDN serves a giant body (the SHA-256 still rejects it — this only
// prevents an OOM before the mismatch is detected). Build-time only, never on users.
const MAX_BB_TARBALL_BYTES = 64 * 1024 * 1024;

async function fetchWindowsBb(version: string, destExe: string): Promise<void> {
  const tag = windowsBbReleaseTag(version);
  const expected = resolveWindowsBbChecksum(version);
  const url = `https://github.com/AztecProtocol/aztec-packages/releases/download/${tag}/${WINDOWS_BB_ASSET}`;

  console.log(`Fetching Windows bb.exe: ${url}`);
  const res = await fetch(url);
  if (!res.ok) {
    throw new Error(`Failed to download ${url}: HTTP ${res.status} ${res.statusText}`);
  }
  const declared = Number(res.headers.get("content-length") ?? "0");
  if (declared > MAX_BB_TARBALL_BYTES) {
    throw new Error(
      `${WINDOWS_BB_ASSET} too large: Content-Length ${declared} > ${MAX_BB_TARBALL_BYTES}`,
    );
  }
  const data = new Uint8Array(await res.arrayBuffer());
  if (data.length > MAX_BB_TARBALL_BYTES) {
    throw new Error(
      `${WINDOWS_BB_ASSET} too large: ${data.length} bytes > ${MAX_BB_TARBALL_BYTES}`,
    );
  }
  assertSha256(data, expected, WINDOWS_BB_ASSET);
  console.log(`SHA-256 verified: ${expected}`);

  const work = mkdtempSync(join(tmpdir(), "bb-win-"));
  try {
    const tarPath = join(work, WINDOWS_BB_ASSET);
    writeFileSync(tarPath, data);
    const extractDir = join(work, "extract");
    mkdirSync(extractDir);
    // Invoke System32's bsdtar by ABSOLUTE path. A bare "tar.exe" resolves via PATH,
    // and under Git Bash (e.g. a `shell: bash` CI step) that is Git's GNU tar, which
    // mishandles C:\ paths and dies with "gzip: stdin: unexpected end of file".
    // bsdtar is native (Win10 1803+/Server 2019+) and handles Windows paths; the
    // absolute path makes extraction shell-independent. execFileSync = no shell.
    const systemRoot = process.env.SystemRoot ?? process.env.windir ?? "C:\\Windows";
    const tarExe = join(systemRoot, "System32", "tar.exe");
    execFileSync(tarExe, ["-xzf", tarPath, "-C", extractDir], { stdio: "inherit" });
    // Canary: the tarball must hold ONLY bb.exe. If a future bb release bundles DLLs,
    // throw loudly rather than silently shipping a broken (missing-dependency) sidecar.
    const entries = readdirSync(extractDir);
    if (entries.length !== 1 || entries[0] !== "bb.exe") {
      throw new Error(
        `Unexpected Windows bb archive layout: expected only bb.exe, got [${entries.join(", ")}]. ` +
          `bb may have gained runtime dependencies — revisit the self-contained sidecar assumption.`,
      );
    }
    copyFileSync(join(extractDir, "bb.exe"), destExe);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

/**
 * Resolve the LIVE `@aztec/bb.js` version + package root from the installed dependency tree (bb-prover
 * is a direct SDK dep; bb.js is its dep). Single source of truth for the bb version — the committed
 * AZTEC_VERSION file can drift. Extracted so the lean headless CI legs can read the version without the
 * full bb-copy prebuild. (core-extraction Phase 3b)
 */
export function resolveAztecBb(): { version: string; bbJsRoot: string } {
  const sdkDir = join(import.meta.dirname!, "..", "..", "sdk");
  const bbProverEntry = Bun.resolveSync("@aztec/bb-prover", sdkDir);
  const bbJsPkgJson = Bun.resolveSync("@aztec/bb.js/package.json", dirname(bbProverEntry));
  const bbJsRoot = dirname(bbJsPkgJson);
  const version: string = JSON.parse(readFileSync(bbJsPkgJson, "utf8")).version;
  return { version, bbJsRoot };
}

async function main(): Promise<void> {
  // Single source of truth for the bb version + package root (the prebuild + the version-only CI step
  // both call resolveAztecBb — keeps them from drifting). The LIVE version drives the npm build dir,
  // the Windows release tag, and the AZTEC_VERSION file written below.
  const { version: aztecVersion, bbJsRoot } = resolveAztecBb();

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
