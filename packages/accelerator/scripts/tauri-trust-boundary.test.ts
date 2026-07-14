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

// The authoritative per-window (window → snake_case command) matrix. Every command a window's frontend
// invokes MUST be here, and NOTHING more (least privilege). has_app_acl is all-or-nothing: a command absent
// from a window's capability is default-DENIED for that window.
const WINDOW_MATRIX: Record<string, string[]> = {
  settings: [
    "get_config",
    "get_autostart_enabled",
    "get_system_info",
    "set_autostart",
    "set_auto_update",
    "set_speed",
    "enable_safari_support",
    "disable_safari_support",
    "remove_approved_origin",
  ],
  authorize: ["get_verified_info", "respond_auth"],
  "update-prompt": ["respond_update_prompt"],
};
const snakeToPerm = (cmd: string) => `allow-${cmd.replace(/_/g, "-")}`;

describe("F-012 P3 — per-window capability ACL", () => {
  const CAPS = path.join(SRC_TAURI, "capabilities");

  async function capFiles(): Promise<Record<string, any>> {
    const glob = new Bun.Glob("*.json");
    const out: Record<string, any> = {};
    for await (const name of glob.scan({ cwd: CAPS })) {
      out[name] = JSON.parse(await read(path.join(CAPS, name)));
    }
    return out;
  }

  test("exactly the 3 scoped capabilities exist — no default.json, no extras", async () => {
    const files = await capFiles();
    expect(Object.keys(files).sort()).toEqual([
      "authorize.json",
      "settings.json",
      "update-prompt.json",
    ]);
    // The old broad default.json (core:default + plugin grants to every window) must be gone (D7 DROP).
    expect(files["default.json"]).toBeUndefined();
  });

  test("each capability grants EXACTLY its window's commands and no others", async () => {
    const files = await capFiles();
    const byId: Record<string, any> = {};
    for (const c of Object.values(files)) byId[c.identifier] = c;

    for (const [id, cmds] of Object.entries(WINDOW_MATRIX)) {
      const cap = byId[id];
      expect(cap, `capability ${id}`).toBeTruthy();
      // The window glob matches the runtime label (settings / auth-* / update-prompt).
      const expectedWindow = id === "authorize" ? "auth-*" : id;
      expect(cap.windows).toEqual([expectedWindow]);
      // Exact permission set — kebab `allow-<cmd>`, no plugin/core grants (least privilege).
      expect([...cap.permissions].sort()).toEqual(cmds.map(snakeToPerm).sort());
      for (const p of cap.permissions) {
        expect(p, `${id} grants only app allow-* perms`).toMatch(/^allow-[a-z-]+$/);
      }
    }
  });

  test("the authorization popup canNOT reach any settings mutator (least-privilege drift guard)", async () => {
    const files = await capFiles();
    const authorize = Object.values(files).find((c) => c.identifier === "authorize");
    for (const settingsCmd of WINDOW_MATRIX.settings) {
      expect(authorize.permissions, `auth must not grant ${settingsCmd}`).not.toContain(
        snakeToPerm(settingsCmd),
      );
    }
  });

  test("build.rs COMMANDS == main.rs generate_handler! == union of capability grants (set-equality)", async () => {
    const buildRs = await read(path.join(SRC_TAURI, "build.rs"));
    const mainRs = await read(path.join(SRC_TAURI, "src", "main.rs"));

    // build.rs: the string list passed to AppManifest.commands().
    const commandsBlock = buildRs.match(/let commands: &\[&str\] = &\[([\s\S]*?)\];/);
    expect(commandsBlock, "build.rs COMMANDS block").toBeTruthy();
    const buildCommands = [...commandsBlock![1].matchAll(/"([a-z_]+)"/g)].map((m) => m[1]).sort();

    // main.rs: the generate_handler! command list.
    const handlerBlock = mainRs.match(/generate_handler!\[([\s\S]*?)\]/);
    expect(handlerBlock, "main.rs generate_handler!").toBeTruthy();
    const handlers = [...handlerBlock![1].matchAll(/commands::([a-z_]+)/g)].map((m) => m[1]).sort();

    // union of every capability's granted commands (perm → snake).
    const files = await capFiles();
    const granted = new Set<string>();
    for (const c of Object.values(files)) {
      for (const p of c.permissions) granted.add(p.replace(/^allow-/, "").replace(/-/g, "_"));
    }
    const grantedSorted = [...granted].sort();

    expect(buildCommands).toEqual(handlers); // declared surface == registered surface
    expect(grantedSorted).toEqual(handlers); // every registered command is granted to exactly some window
    expect(handlers.length).toBe(12);
  });

  test("tauri.conf.json pins the capability allowlist to exactly the 3", async () => {
    const c = JSON.parse(await read(TAURI_CONF));
    expect([...(c.app?.security?.capabilities ?? [])].sort()).toEqual([
      "authorize",
      "settings",
      "update-prompt",
    ]);
  });
});
