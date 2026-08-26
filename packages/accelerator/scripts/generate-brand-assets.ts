/**
 * Regenerates every committed brand binary from vector/HTML sources.
 *
 *   bun scripts/generate-brand-assets.ts --target app-icons   # icns/ico/png ladder via `tauri icon`
 *   bun scripts/generate-brand-assets.ts --target tray        # tray-idle + 24 orbit frames (SVG+PNG)
 *   bun scripts/generate-brand-assets.ts --target og-landing | og-playground   # 1200×630 via chromium
 *   bun scripts/generate-brand-assets.ts --check              # byte-reproduce the TRAY assets
 *                                                             # (app icons: sha256 manifest test)
 *   bun scripts/generate-brand-assets.ts --write-manifest     # refresh brand-assets.sha256.json
 *
 * Determinism: icon/tray rendering is resvg-wasm with system fonts disabled and no text, so bytes
 * are stable per resvg version. og cards render real text in chromium and are NOT
 * cross-machine-stable; the sha256 manifest pins the committed bytes, not the renderer.
 */
import { cpSync, existsSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { icnsChunks, icoEntries, isTemplatePure, pngInfo } from "./png-inspect";

const PKG_DIR = resolve(import.meta.dir, "..");
const ICONS_DIR = join(PKG_DIR, "src-tauri", "icons");
const FONTS_DIR = join(PKG_DIR, "src-tauri", "frontend", "fonts");
const MANIFEST = join(PKG_DIR, "scripts", "brand-assets.sha256.json");

// One geometry source for every tray asset: the Brand Book bolt in a 48-unit box,
// spark orbiting at r=20 from center, frame N at -90° + (N-1)·15° (top start, clockwise).
const TRAY_BOLT = "M26 5 L12 27 H21 L19 43 L36 19 H25 Z";
const TRAY_FRAMES = 24;
const TRAY_PX = 44;
const SPARK_R = 3.1;
// Clear space held around the spark wherever it crosses the bolt. 1.8 units survives the menu bar's
// downscale of the 44px asset; much more starts eating visible chunks out of the bolt.
const SPARK_GAP = 1.8;

const BUNDLE_FILES = ["32x32.png", "128x128.png", "icon.png", "icon.icns", "icon.ico"];

// App-shipped weights only; web surfaces load Google Fonts. latin + latin-ext per family.
const VENDORED_FONTS: Array<[pkg: string, family: string, weights: number[]]> = [
  ["@fontsource/bricolage-grotesque", "bricolage-grotesque", [600, 700, 800]],
  ["@fontsource/figtree", "figtree", [400, 500, 600, 700]],
  ["@fontsource/fragment-mono", "fragment-mono", [400]],
];

async function genFonts(outDir: string): Promise<void> {
  for (const [pkg, family, weights] of VENDORED_FONTS) {
    for (const weight of weights) {
      for (const subset of ["latin", "latin-ext"]) {
        const name = `${family}-${subset}-${weight}-normal.woff2`;
        const src = join(PKG_DIR, "node_modules", pkg, "files", name);
        if (!existsSync(src)) throw new Error(`missing ${src}`);
        cpSync(src, join(outDir, name));
      }
    }
  }
  console.log(`vendored ${VENDORED_FONTS.reduce((n, [, , w]) => n + w.length * 2, 0)} woff2 files`);
}

let wasmReady = false;
async function resvg(svg: string, widthPx: number): Promise<Uint8Array> {
  const mod = await import("@resvg/resvg-wasm");
  if (!wasmReady) {
    const wasmPath = join(PKG_DIR, "node_modules", "@resvg", "resvg-wasm", "index_bg.wasm");
    await mod.initWasm(await Bun.file(wasmPath).arrayBuffer());
    wasmReady = true;
  }
  const r = new mod.Resvg(svg, {
    fitTo: { mode: "width", value: widthPx },
    font: { loadSystemFonts: false },
  });
  return r.render().asPng();
}

function traySvg(frame: number | null): string {
  let spark = "";
  let knockout = "";
  let masked = "";
  if (frame !== null) {
    const theta = (-90 + (frame - 1) * 15) * (Math.PI / 180);
    const cx = (24 + 20 * Math.cos(theta)).toFixed(3);
    const cy = (24 + 20 * Math.sin(theta)).toFixed(3);
    spark = `\n  <circle cx="${cx}" cy="${cy}" r="${SPARK_R}" fill="black"/>`;
    // Both shapes are solid black, so where the orbit crosses the bolt they fuse and the spark
    // disappears — worst at the bolt's lower tail, which sits 19.6 from center against an orbit of
    // 20 (frame 14 lands on it almost exactly). Punching the gap out of the BOLT keeps the spark a
    // full disc and the orbit geometry untouched.
    knockout =
      `\n  <mask id="gap" maskUnits="userSpaceOnUse" x="0" y="0" width="48" height="48">` +
      `\n    <rect width="48" height="48" fill="white"/>` +
      `\n    <circle cx="${cx}" cy="${cy}" r="${SPARK_R + SPARK_GAP}" fill="black"/>` +
      `\n  </mask>`;
    masked = ` mask="url(#gap)"`;
  }
  return `<svg viewBox="0 0 48 48" xmlns="http://www.w3.org/2000/svg" fill="none">${knockout}
  <path d="${TRAY_BOLT}" fill="black" stroke="black" stroke-width="4" stroke-linejoin="round"${masked}/>${spark}
</svg>
`;
}

async function genTray(outDir: string): Promise<void> {
  const jobs: Array<[string, number | null]> = [["tray-idle", null]];
  for (let n = 1; n <= TRAY_FRAMES; n++) jobs.push([`tray-proving-${n}`, n]);
  for (const [name, frame] of jobs) {
    const svg = traySvg(frame);
    const png = await resvg(svg, TRAY_PX);
    const info = pngInfo(png);
    if (info.width !== TRAY_PX || info.height !== TRAY_PX || info.colorType !== 6) {
      throw new Error(
        `${name}: expected ${TRAY_PX}x${TRAY_PX} RGBA, got ${info.width}x${info.height} ct=${info.colorType}`,
      );
    }
    if (!isTemplatePure(png)) throw new Error(`${name}: non-black RGB breaks macOS template mode`);
    await Bun.write(join(outDir, `${name}.svg`), svg);
    await Bun.write(join(outDir, `${name}.png`), png);
  }
}

async function genAppIcons(outDir: string): Promise<void> {
  const masterSvg = await Bun.file(join(ICONS_DIR, "icon.svg")).text();
  const staging = mkdtempSync(join(tmpdir(), "presto-icons-"));
  try {
    const master = join(staging, "icon-1024.png");
    await Bun.write(master, await resvg(masterSvg, 1024));
    const cli = Bun.spawnSync(["bunx", "tauri", "icon", master, "-o", join(staging, "out")], {
      cwd: PKG_DIR,
      stdout: "pipe",
      stderr: "pipe",
    });
    if (cli.exitCode !== 0) {
      throw new Error(`tauri icon failed:\n${cli.stderr.toString()}\n${cli.stdout.toString()}`);
    }
    for (const f of BUNDLE_FILES) {
      const src = join(staging, "out", f);
      if (!existsSync(src)) throw new Error(`tauri icon did not emit ${f}`);
      cpSync(src, join(outDir, f));
    }
    // The CLI emits 128x128@2x but not 256/512, and its icon.png is 512px; the repo tracks
    // 256/512 rungs and a 1024px icon.png (the Linux {{icon}} source), so render those directly.
    await Bun.write(join(outDir, "256x256.png"), await resvg(masterSvg, 256));
    await Bun.write(join(outDir, "512x512.png"), await resvg(masterSvg, 512));
    await Bun.write(join(outDir, "icon.png"), await resvg(masterSvg, 1024));
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
  const icns = icnsChunks(new Uint8Array(await Bun.file(join(outDir, "icon.icns")).arrayBuffer()));
  const ico = icoEntries(new Uint8Array(await Bun.file(join(outDir, "icon.ico")).arrayBuffer()));
  console.log(`icon.icns chunks: ${icns.map((c) => c.type).join(",")}`);
  console.log(`icon.ico entries: ${ico.map((e) => `${e.width}x${e.height}@${e.bpp}`).join(",")}`);
}

function ogHtml(variant: "landing" | "playground"): string {
  const font = (file: string) => `url("file://${join(FONTS_DIR, file)}") format("woff2")`;
  const domain =
    variant === "landing" ? "aztec-accelerator.dev" : "playground.aztec-accelerator.dev";
  const line1 = variant === "landing" ? "Fast proofs." : "Try it live.";
  const line2 = variant === "landing" ? "Like magic" : "The playground";
  return `<!doctype html><html><head><meta charset="utf-8"><style>
  @font-face{font-family:Bricolage;font-weight:800;src:${font("bricolage-grotesque-latin-800-normal.woff2")}}
  @font-face{font-family:Fragment;font-weight:400;src:${font("fragment-mono-latin-400-normal.woff2")}}
  *{margin:0;box-sizing:border-box}
  body{width:1200px;height:630px;background:#FFFAF1;display:flex;align-items:center;gap:64px;
    padding:0 96px;font-family:Bricolage,sans-serif;color:#241B33}
  .mark{flex:none}
  h1{font-size:96px;font-weight:800;line-height:1.04;letter-spacing:-.02em}
  h1 .c{color:#3B4FE0}.h1 .g{color:#FFC53D}
  .meta{font-family:Fragment,monospace;font-size:30px;color:#6E6580;margin-top:36px}
  </style></head><body>
  <svg class="mark" width="300" height="300" viewBox="0 0 120 120" fill="none">
    <path d="M64 24 L38 66 H56 L52 94 L86 50 H64 Z" fill="#3B4FE0" stroke="#3B4FE0" stroke-width="8" stroke-linejoin="round"/>
    <path d="M88 22 l4 8.4 8.4 4 -8.4 4 -4 8.4 -4-8.4 -8.4-4 8.4-4 Z" fill="#FFC53D"/>
    <circle cx="30" cy="34" r="4" fill="#F04E42"/>
  </svg>
  <div><h1>${line1}<br>${line2}<span class="c"> </span><span style="color:#FFC53D">✦</span></h1>
  <div class="meta">${domain} · free · open source</div></div>
  </body></html>`;
}

async function genOg(variant: "landing" | "playground"): Promise<void> {
  const outPath = resolve(PKG_DIR, "..", variant, "public", "og-image.png");
  const { chromium } = await import("playwright");
  const browser = await chromium.launch({ headless: true }).catch((err) => {
    console.error(`chromium launch failed (run \`bunx playwright install chromium\` once): ${err}`);
    return process.exit(1);
  });
  try {
    const page = await browser.newPage({ viewport: { width: 1200, height: 630 } });
    await page.setContent(ogHtml(variant), { waitUntil: "networkidle" });
    await page.evaluate(
      () => (document as unknown as { fonts: { ready: Promise<unknown> } }).fonts.ready,
    );
    await Bun.write(outPath, await page.screenshot({ type: "png" }));
  } finally {
    await browser.close();
  }
  console.log(`wrote ${outPath}`);
}

async function sha256(path: string): Promise<string> {
  const bytes = await Bun.file(path).arrayBuffer();
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Buffer.from(digest).toString("hex");
}

/** Every committed brand binary, keyed by repo-relative path. */
async function manifestEntries(): Promise<Record<string, string>> {
  const entries: Record<string, string> = {};
  const add = async (abs: string, rel: string) => {
    entries[rel] = await sha256(abs);
  };
  for (const f of readdirSync(ICONS_DIR)
    .filter((f) => f.endsWith(".png") || f.endsWith(".icns") || f.endsWith(".ico"))
    .sort()) {
    await add(join(ICONS_DIR, f), `packages/accelerator/src-tauri/icons/${f}`);
  }
  if (existsSync(FONTS_DIR)) {
    for (const f of readdirSync(FONTS_DIR)
      .filter((f) => f.endsWith(".woff2"))
      .sort()) {
      await add(join(FONTS_DIR, f), `packages/accelerator/src-tauri/frontend/fonts/${f}`);
    }
  }
  for (const v of ["landing", "playground"] as const) {
    const p = resolve(PKG_DIR, "..", v, "public", "og-image.png");
    if (existsSync(p)) await add(p, `packages/${v}/public/og-image.png`);
  }
  return entries;
}

async function check(): Promise<void> {
  const temp = mkdtempSync(join(tmpdir(), "presto-check-"));
  try {
    await genTray(temp);
    const names = [
      "tray-idle",
      ...Array.from({ length: TRAY_FRAMES }, (_, i) => `tray-proving-${i + 1}`),
    ];
    for (const name of names) {
      for (const ext of ["png", "svg"]) {
        const fresh = new Uint8Array(await Bun.file(join(temp, `${name}.${ext}`)).arrayBuffer());
        const committed = new Uint8Array(
          await Bun.file(join(ICONS_DIR, `${name}.${ext}`)).arrayBuffer(),
        );
        if (Buffer.compare(fresh, committed) !== 0)
          throw new Error(`${name}.${ext} differs from committed`);
      }
    }
    console.log("check ok: tray assets reproduce byte-identically");
  } finally {
    rmSync(temp, { recursive: true, force: true });
  }
}

const args = new Set(Bun.argv.slice(2));
const target = Bun.argv[Bun.argv.indexOf("--target") + 1];
if (args.has("--check")) {
  await check();
} else if (args.has("--write-manifest")) {
  await Bun.write(MANIFEST, `${JSON.stringify(await manifestEntries(), null, 2)}\n`);
  console.log(`wrote ${MANIFEST}`);
} else if (args.has("--target")) {
  if (target === "tray") await genTray(ICONS_DIR);
  else if (target === "app-icons") await genAppIcons(ICONS_DIR);
  else if (target === "fonts") await genFonts(FONTS_DIR);
  else if (target === "og-landing") await genOg("landing");
  else if (target === "og-playground") await genOg("playground");
  else {
    console.error(`unknown target ${target}`);
    process.exit(1);
  }
} else {
  console.error(
    "usage: --target app-icons|tray|og-landing|og-playground | --check | --write-manifest",
  );
  process.exit(1);
}
