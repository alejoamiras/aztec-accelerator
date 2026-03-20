/**
 * Extract the `bb` binary from `@aztec/bb.js` and copy it to `src-tauri/binaries/`
 * as a Tauri sidecar with the correct target-triple suffix.
 *
 * Tauri expects sidecars at `binaries/<name>-<target-triple>`.
 */
import { execSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";

// --- Resolve bb binary from @aztec/bb.js ---
// Two-step resolution: first find @aztec/bb-prover (direct dep of sdk), then resolve
// @aztec/bb.js from bb-prover's directory (it's bb-prover's own dependency).
// This uses bun's module resolution instead of scanning node_modules/.bun/ internals.

const sdkDir = join(import.meta.dirname!, "..", "..", "sdk");
const bbProverEntry = Bun.resolveSync("@aztec/bb-prover", sdkDir);
const bbJsPkgJson = Bun.resolveSync("@aztec/bb.js/package.json", dirname(bbProverEntry));
const bbJsRoot = dirname(bbJsPkgJson);

const arch = process.arch === "arm64" ? "arm64" : "amd64";
const os = process.platform === "darwin" ? "macos" : "linux";
const bbSource = join(bbJsRoot, "build", `${arch}-${os}`, "bb");

if (!existsSync(bbSource)) {
  console.error(`bb binary not found at ${bbSource}`);
  process.exit(1);
}

// --- Map to Tauri target triple ---

function getTargetTriple(): string {
  const platform = process.platform;
  const nodeArch = process.arch;

  if (platform === "darwin") {
    return nodeArch === "arm64" ? "aarch64-apple-darwin" : "x86_64-apple-darwin";
  }
  if (platform === "linux") {
    return nodeArch === "arm64" ? "aarch64-unknown-linux-gnu" : "x86_64-unknown-linux-gnu";
  }
  throw new Error(`Unsupported platform: ${platform}`);
}

const targetTriple = getTargetTriple();
const binariesDir = join(import.meta.dirname!, "..", "src-tauri", "binaries");
const dest = join(binariesDir, `bb-${targetTriple}`);

mkdirSync(binariesDir, { recursive: true });
copyFileSync(bbSource, dest);
chmodSync(dest, 0o755);

// Remove macOS quarantine attribute (prevents Gatekeeper from killing the binary)
if (process.platform === "darwin") {
  try {
    execSync(`xattr -d com.apple.quarantine "${dest}"`, { stdio: "ignore" });
  } catch {
    // Attribute may not exist, that's fine
  }
}

// --- Write Aztec version for build.rs ---
const bbJsPkg = JSON.parse(readFileSync(join(bbJsRoot, "package.json"), "utf8"));
const aztecVersion: string = bbJsPkg.version;
writeFileSync(join(import.meta.dirname!, "..", "src-tauri", "AZTEC_VERSION"), aztecVersion);

console.log(`Copied bb -> ${dest} (from ${bbJsRoot})`);
console.log(`Aztec bb version: ${aztecVersion}`);
