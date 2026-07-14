/**
 * F-012 trust-boundary static guards (`bun test scripts/`). These are the drift-detectors: they fail CI
 * if a future change reopens the frontend trust boundary — an inline script/style, the `withGlobalTauri`
 * global, a CSP relaxation, or a capability/command-matrix mismatch. They read files only (no app runtime),
 * so they run fast on the GUI-less CI matrix. Grows per phase: P1 externalization (here); P2 CSP; P3 caps.
 */
import { describe, expect, test } from "bun:test";
import path from "node:path";

const SRC_TAURI = path.resolve(import.meta.dir, "..", "src-tauri");
const FRONTEND = path.join(SRC_TAURI, "frontend");
const FRONTEND_SRC = path.join(SRC_TAURI, "frontend-src");

const PAGES = ["authorize.html", "settings.html", "update-prompt.html"] as const;

async function read(p: string): Promise<string> {
  return await Bun.file(p).text();
}

/** Strip `/* *​/` block and `//` line comments so guards match CODE, not prose in doc comments. */
function stripComments(js: string): string {
  return js.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
}

describe("F-012 P1 — frontend externalization", () => {
  test("each popup page loads exactly one ES-module bundle and no other script", async () => {
    for (const page of PAGES) {
      const html = await read(path.join(FRONTEND, page));
      const scripts = [...html.matchAll(/<script\b[^>]*>/gi)].map((m) => m[0]);
      expect(scripts.length, `${page}: exactly one <script>`).toBe(1);
      const tag = scripts[0];
      expect(tag, `${page}: must be a module`).toContain('type="module"');
      // Points at a bundled asset built from frontend-src/ — never an inline block.
      expect(tag, `${page}: loads assets/*.js`).toMatch(/src="assets\/[a-z-]+\.js"/);
    }
  });

  test("no inline scripts, inline styles, or markup event handlers in any page", async () => {
    for (const page of PAGES) {
      const html = await read(path.join(FRONTEND, page));
      // An inline <script> has no `src=` before its closing `>`.
      for (const tag of html.matchAll(/<script\b([^>]*)>/gi)) {
        expect(tag[1], `${page}: <script> must have src (no inline JS)`).toContain("src=");
      }
      expect(html, `${page}: no <style> block`).not.toMatch(/<style\b/i);
      expect(html, `${page}: no inline style= attribute`).not.toMatch(/\sstyle\s*=/i);
      expect(html, `${page}: no on*= handler attribute`).not.toMatch(/\son[a-z]+\s*=\s*["']/i);
    }
  });

  test("frontend-src references the official API, never the window.__TAURI__ global", async () => {
    const glob = new Bun.Glob("*.js");
    const names: string[] = [];
    for await (const name of glob.scan({ cwd: FRONTEND_SRC })) names.push(name);
    expect(names.length).toBeGreaterThanOrEqual(4); // bridge + 3 pages

    let importsCore = false;
    for (const name of names) {
      const js = await read(path.join(FRONTEND_SRC, name));
      const code = stripComments(js);
      expect(code, `${name}: no window.__TAURI__ global back-door`).not.toMatch(
        /window\.__TAURI__\b/,
      );
      if (/@tauri-apps\/api\/core/.test(code)) importsCore = true;
    }
    expect(importsCore, "the shared bridge imports invoke from @tauri-apps/api/core").toBe(true);
  });

  test("the old global tauri-bridge.js is gone", async () => {
    expect(await Bun.file(path.join(FRONTEND, "tauri-bridge.js")).exists()).toBe(false);
  });
});
