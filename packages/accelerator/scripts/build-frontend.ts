#!/usr/bin/env bun
import { mkdir, rm } from "node:fs/promises";
import path from "node:path";
/**
 * F-012: bundle the popup frontend ESM modules (`src-tauri/frontend-src/*.js`) into the
 * gitignored `src-tauri/frontend/assets/*.js` that ship inside the Tauri app. Each page gets one
 * self-contained bundle (the shared `bridge.js` + `@tauri-apps/api/core` are inlined), loaded via a
 * single `<script type="module">`. This is what lets `withGlobalTauri: false` + `script-src 'self'`
 * work: no inline scripts, no global back-door.
 *
 * Runs as tauri's beforeDev/beforeBuildCommand AND must run before any raw `cargo build` (the
 * bundles are gitignored, so a fresh checkout has none). `build.rs` fails the Rust build if a bundle
 * is missing or STALE vs the sources — the `.build-manifest.json` written here is that staleness key.
 *
 * Wired via `bun run --cwd packages/accelerator frontend:build`.
 */
import { Glob } from "bun";

const ROOT = path.resolve(import.meta.dir, "..");
const SRC_DIR = path.join(ROOT, "src-tauri", "frontend-src");
const OUT_DIR = path.join(ROOT, "src-tauri", "frontend", "assets");
const MANIFEST = path.join(OUT_DIR, ".build-manifest.json");
// Also fingerprint the dependency surface: a @tauri-apps/api bump (package.json) or a resolution change
// (root bun.lock) must invalidate the shipped bundles even if frontend-src/ is untouched (GATE-3 codex).
const PKG_JSON = path.join(ROOT, "package.json");
const LOCKFILE = path.join(ROOT, "..", "..", "bun.lock");

// Per-page entrypoints (bridge.js is shared and pulled into each bundle, not an entry of its own).
const ENTRIES = ["authorize.js", "settings.js", "update-prompt.js"] as const;

/**
 * SHA-256 over raw bytes → lowercase hex. `build.rs` recomputes it identically (sha2 crate) so the guard
 * detects not just "forgot to rebuild" but a swapped/injected OUTPUT bundle — a fnv-style fingerprint an
 * attacker could trivially forge would not. (An attacker who can rewrite BOTH a bundle and this manifest
 * still wins; the guard's job is to catch accidental staleness and a bundle-only substitution.)
 */
function sha256Hex(bytes: ArrayBuffer | Uint8Array): string {
  return new Bun.CryptoHasher("sha256").update(bytes).digest("hex");
}

/** Fingerprint every build INPUT: each frontend-src/*.js plus the dependency-surface files. */
async function hashInputs(): Promise<Record<string, string>> {
  const inputs: Record<string, string> = {};
  const glob = new Glob("*.js");
  const names: string[] = [];
  for await (const name of glob.scan({ cwd: SRC_DIR })) names.push(name);
  names.sort();
  for (const name of names) {
    inputs[`frontend-src/${name}`] = sha256Hex(
      await Bun.file(path.join(SRC_DIR, name)).arrayBuffer(),
    );
  }
  inputs["package.json"] = sha256Hex(await Bun.file(PKG_JSON).arrayBuffer());
  inputs["bun.lock"] = sha256Hex(await Bun.file(LOCKFILE).arrayBuffer());
  return inputs;
}

/** Fingerprint every emitted OUTPUT bundle (so a post-build swap is caught). */
async function hashOutputs(): Promise<Record<string, string>> {
  const outputs: Record<string, string> = {};
  for (const e of ENTRIES) {
    outputs[e] = sha256Hex(await Bun.file(path.join(OUT_DIR, e)).arrayBuffer());
  }
  return outputs;
}

async function main() {
  // Snapshot inputs BEFORE bundling (to catch a source edit racing the build).
  const inputsBefore = await hashInputs();

  // Clean-before-build: never ship an orphaned stale bundle from a since-deleted source.
  await rm(OUT_DIR, { recursive: true, force: true });
  await mkdir(OUT_DIR, { recursive: true });

  const result = await Bun.build({
    entrypoints: ENTRIES.map((e) => path.join(SRC_DIR, e)),
    outdir: OUT_DIR,
    target: "browser",
    format: "esm",
    minify: true,
    sourcemap: "none",
    splitting: false,
    naming: "[name].js",
  });

  if (!result.success) {
    for (const log of result.logs) console.error(log);
    throw new Error("frontend bundle build failed");
  }

  const emitted = new Set(result.outputs.map((o) => path.basename(o.path)));
  for (const entry of ENTRIES) {
    if (!emitted.has(entry)) throw new Error(`expected bundle ${entry} was not emitted`);
  }

  // Re-snapshot inputs AFTER bundling; a mismatch means a source changed mid-build → the manifest would
  // record hashes for content the bundles don't reflect. Fail rather than record a lie.
  const inputs = await hashInputs();
  if (JSON.stringify(inputs) !== JSON.stringify(inputsBefore)) {
    throw new Error("a tracked input changed during the bundle build; re-run frontend:build");
  }

  const outputs = await hashOutputs();
  await Bun.write(
    MANIFEST,
    `${JSON.stringify({ schema: 2, algo: "sha256", inputs, outputs }, null, 2)}\n`,
  );

  console.log(
    `frontend:build → ${ENTRIES.length} bundles + manifest in ${path.relative(ROOT, OUT_DIR)}`,
  );
}

await main();
