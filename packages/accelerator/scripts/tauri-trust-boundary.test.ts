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
const TAURI_CONF = path.join(SRC_TAURI, "tauri.conf.json");

// The exact CSP directives F-012 ships. Kept as an ordered list so the drift test pins each one — a future
// relaxation (adding `unsafe-inline`, dropping `form-action`, widening `connect-src`) fails CI loudly.
const REQUIRED_CSP: Record<string, string> = {
  "default-src": "'self'",
  "script-src": "'self'",
  "style-src": "'self'",
  "img-src": "'self'",
  "connect-src": "ipc: http://ipc.localhost",
  "object-src": "'none'",
  "base-uri": "'none'",
  "frame-ancestors": "'none'",
  "form-action": "'none'",
  "frame-src": "'none'",
  "child-src": "'none'",
  "worker-src": "'none'",
};

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

describe("F-012 P2 — CSP + global flag drift guards", () => {
  async function conf(): Promise<any> {
    return JSON.parse(await read(TAURI_CONF));
  }

  test("withGlobalTauri is false (no window.__TAURI__ back-door)", async () => {
    expect((await conf()).app?.withGlobalTauri).toBe(false);
  });

  test("CSP pins every required directive with no unsafe-* relaxation", async () => {
    const csp: string = (await conf()).app?.security?.csp ?? "";
    expect(csp, "csp must be set").toBeTruthy();

    // Parse "name a b c; name2 …" into a directive → sources map.
    const directives = new Map<string, string>();
    for (const chunk of csp.split(";")) {
      const parts = chunk.trim().split(/\s+/);
      if (parts.length) directives.set(parts[0], parts.slice(1).join(" "));
    }
    for (const [name, sources] of Object.entries(REQUIRED_CSP)) {
      expect(directives.get(name), `csp ${name}`).toBe(sources);
    }
    // connect-src deliberately EXCLUDES 'self' (popups never fetch) — only the IPC origins.
    expect(directives.get("connect-src")).not.toContain("'self'");
    // No inline/eval escape hatches, ever.
    expect(csp).not.toContain("unsafe-inline");
    expect(csp).not.toContain("unsafe-eval");
  });

  test("dev never silently weakens the shipped CSP (no devUrl, no weaker devCsp)", async () => {
    const c = await conf();
    // is_dev() serves the same csp only when devCsp is unset and there is no devUrl — pin both so a future
    // edit can't turn the dev-mode PR-gate WebDriver run into a no-op against the real policy.
    expect(c.build?.devUrl, "no devUrl").toBeUndefined();
    const devCsp = c.app?.security?.devCsp;
    if (devCsp !== undefined) expect(devCsp).toBe(c.app.security.csp);
    // The asset-CSP nonce augmentation must stay on (never disable it).
    expect(c.app?.security?.dangerousDisableAssetCspModification ?? false).toBe(false);
  });
});
