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

// Per-page entrypoints (bridge.js is shared and pulled into each bundle, not an entry of its own).
const ENTRIES = ["authorize.js", "settings.js", "update-prompt.js"] as const;

/**
 * FNV-1a 64-bit over raw bytes → lowercase hex. Deliberately simple and dependency-free so `build.rs`
 * can recompute it identically in Rust (a content fingerprint to detect "forgot to rebuild", not a
 * security primitive — the trust comes from CI building the bundles from reviewed source).
 */
function fnv1a64Hex(bytes: Uint8Array): string {
  const OFFSET = 0xcbf29ce484222325n;
  const PRIME = 0x100000001b3n;
  const MASK = 0xffffffffffffffffn;
  let hash = OFFSET;
  for (const b of bytes) {
    hash = ((hash ^ BigInt(b)) * PRIME) & MASK;
  }
  return hash.toString(16).padStart(16, "0");
}

async function hashInputs(): Promise<Record<string, string>> {
  const inputs: Record<string, string> = {};
  const glob = new Glob("*.js");
  const names: string[] = [];
  for await (const name of glob.scan({ cwd: SRC_DIR })) names.push(name);
  names.sort();
  for (const name of names) {
    const bytes = new Uint8Array(await Bun.file(path.join(SRC_DIR, name)).arrayBuffer());
    inputs[name] = fnv1a64Hex(bytes);
  }
  return inputs;
}

async function main() {
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

  const inputs = await hashInputs();
  await Bun.write(MANIFEST, `${JSON.stringify({ schema: 1, algo: "fnv1a64", inputs }, null, 2)}\n`);

  console.log(
    `frontend:build → ${ENTRIES.length} bundles + manifest in ${path.relative(ROOT, OUT_DIR)}`,
  );
}

await main();
