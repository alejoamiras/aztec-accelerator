/**
 * Brand-asset integrity: geometry, macOS template purity, container ladders, and the sha256
 * manifest. The icns/ico expectations mirror what `tauri icon` (CLI 2.x) actually emits — if a
 * CLI bump changes the ladder, regenerate and update these sets deliberately.
 */
import { describe, expect, test } from "bun:test";
import { existsSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { icnsChunks, icoEntries, isTemplatePure, pngInfo } from "./png-inspect";

const PKG_DIR = resolve(import.meta.dir, "..");
const REPO_DIR = resolve(PKG_DIR, "..", "..");
const ICONS_DIR = join(PKG_DIR, "src-tauri", "icons");
const FONTS_DIR = join(PKG_DIR, "src-tauri", "frontend", "fonts");

const bytes = async (path: string) => new Uint8Array(await Bun.file(path).arrayBuffer());

describe("tray icons", () => {
  const names = ["tray-idle", ...Array.from({ length: 24 }, (_, i) => `tray-proving-${i + 1}`)];

  test("25 frames, 44x44 RGBA, template-pure, with SVG sources", async () => {
    for (const name of names) {
      expect(existsSync(join(ICONS_DIR, `${name}.svg`))).toBe(true);
      const png = await bytes(join(ICONS_DIR, `${name}.png`));
      const info = pngInfo(png);
      expect(`${name}:${info.width}x${info.height}:ct${info.colorType}`).toBe(`${name}:44x44:ct6`);
      expect(isTemplatePure(png)).toBe(true);
    }
  });
});

describe("app icon ladder", () => {
  test("sized PNG rungs match their filenames", async () => {
    for (const [file, size] of [
      ["32x32.png", 32],
      ["128x128.png", 128],
      ["256x256.png", 256],
      ["512x512.png", 512],
      ["icon.png", 1024],
    ] as const) {
      const info = pngInfo(await bytes(join(ICONS_DIR, file)));
      expect(`${file}:${info.width}x${info.height}`).toBe(`${file}:${size}x${size}`);
    }
  });

  test("icns carries the full ladder tauri icon emits", async () => {
    const types = icnsChunks(await bytes(join(ICONS_DIR, "icon.icns")))
      .map((c) => c.type)
      .sort();
    expect(types).toEqual(
      [
        "ic07",
        "ic08",
        "ic09",
        "ic10",
        "ic11",
        "ic12",
        "ic13",
        "ic14",
        "il32",
        "is32",
        "l8mk",
        "s8mk",
      ].sort(),
    );
  });

  test("ico carries the full ladder tauri icon emits", async () => {
    const entries = icoEntries(await bytes(join(ICONS_DIR, "icon.ico")));
    const sizes = entries.map((e) => e.width).sort((a, b) => a - b);
    expect(sizes).toEqual([16, 24, 32, 48, 64, 256]);
    for (const e of entries) expect(e.bpp).toBe(32);
  });
});

describe("sha256 manifest", () => {
  test("every brand binary is pinned and matches", async () => {
    const manifest: Record<string, string> = await Bun.file(
      join(PKG_DIR, "scripts", "brand-assets.sha256.json"),
    ).json();
    // Completeness: every binary in the governed dirs has a manifest row.
    const governed = [
      ...readdirSync(ICONS_DIR)
        .filter((f) => /\.(png|icns|ico)$/.test(f))
        .map((f) => `packages/accelerator/src-tauri/icons/${f}`),
      ...readdirSync(FONTS_DIR)
        .filter((f) => f.endsWith(".woff2"))
        .map((f) => `packages/accelerator/src-tauri/frontend/fonts/${f}`),
    ];
    for (const rel of governed) expect(manifest[rel]).toBeDefined();
    // Integrity: every row's file exists and hashes to the pinned digest.
    for (const [rel, expected] of Object.entries(manifest)) {
      const abs = join(REPO_DIR, rel);
      expect(existsSync(abs)).toBe(true);
      const digest = Buffer.from(
        await crypto.subtle.digest("SHA-256", await Bun.file(abs).arrayBuffer()),
      ).toString("hex");
      expect(`${rel}:${digest}`).toBe(`${rel}:${expected}`);
    }
  });

  test("16 vendored woff2 files are present", () => {
    expect(readdirSync(FONTS_DIR).filter((f) => f.endsWith(".woff2")).length).toBe(16);
  });
});
