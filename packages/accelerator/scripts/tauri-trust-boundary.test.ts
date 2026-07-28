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

const PAGES = [
  "authorize.html",
  "settings.html",
  "update-prompt.html",
  "onboarding.html",
  "renewal.html",
] as const;

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
    "repair_autostart",
    "set_auto_update",
    "set_speed",
    "enable_https",
    "disable_https",
    "remove_https_trust",
    "remove_approved_origin",
  ],
  authorize: ["get_verified_info", "get_pending_auth", "respond_auth"],
  "update-prompt": ["respond_update_prompt"],
  onboarding: ["get_onboarding_state", "complete_onboarding"],
  renewal: ["renew_cert", "record_renewal_prompt"],
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

  test("exactly the 5 scoped capabilities exist — no default.json, no extras", async () => {
    const files = await capFiles();
    expect(Object.keys(files).sort()).toEqual([
      "authorize.json",
      "onboarding.json",
      "renewal.json",
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

    // main.rs: the generate_handler! command list. Every entry is `commands::<name>,` today, but a
    // command defined in main.rs itself would appear bare — so match an optional `commands::` prefix
    // and require the trailing comma every entry has.
    const handlerBlock = mainRs.match(/generate_handler!\[([\s\S]*?)\]/);
    expect(handlerBlock, "main.rs generate_handler!").toBeTruthy();
    const handlers = [...handlerBlock![1].matchAll(/(?:commands::)?([a-z_]+)\s*,/g)]
      .map((m) => m[1])
      .sort();

    // union of every capability's granted commands (perm → snake).
    const files = await capFiles();
    const granted = new Set<string>();
    for (const c of Object.values(files)) {
      for (const p of c.permissions) granted.add(p.replace(/^allow-/, "").replace(/-/g, "_"));
    }
    const grantedSorted = [...granted].sort();

    expect(buildCommands).toEqual(handlers); // declared surface == registered surface
    expect(grantedSorted).toEqual(handlers); // every registered command is granted to exactly some window
    expect(handlers.length).toBe(19); // +repair_autostart (autostart self-heal Fix button, plan D16)
  });

  test("tauri.conf.json pins the capability allowlist to exactly the 5", async () => {
    const c = JSON.parse(await read(TAURI_CONF));
    expect([...(c.app?.security?.capabilities ?? [])].sort()).toEqual([
      "authorize",
      "onboarding",
      "renewal",
      "settings",
      "update-prompt",
    ]);
  });
});

// Tier-4 (audit R1 / C-1): the Windows NSIS uninstall hook must NEVER wipe the CA trust + certs during
// an UPGRADE — Tauri runs the previous version's uninstaller when installing over an existing install.
// The Windows leg of the `cert-trust` CI job now runs the real hook under all three command lines
// (nsis/harness.test.nsi); this static test is the cheap companion that pins the SHAPE, so a refactor
// that drops a guard fails on every platform in milliseconds rather than only in the Windows leg.
//
// TWO guards are load-bearing, not one. `$UpdateMode` is set from the installer's own `/UPDATE` flag
// and forwarded to the old uninstaller only when the installer received it — so a manually downloaded
// installer (the only upgrade route once auto-update is off) leaves it 0. The second guard is
// `$EXEDIR` vs `$INSTDIR`: `_?=` means "do not copy yourself to temp", so an install-over runs the
// uninstaller IN PLACE while a real uninstall runs a `~nsu*.tmp` copy. `_?=` itself is NOT detectable
// — the NSIS stub strips it from `$CMDLINE` (measured; a guard that searched $CMDLINE for it shipped
// and failed). Both directories are canonicalized before comparison because a textual mismatch would
// mean "delete", the unsafe direction.
describe("NSIS uninstall hook — an upgrade must not wipe trust", () => {
  const HOOKS = path.join(SRC_TAURI, "nsis", "hooks.nsi");

  test("the destructive ops require BOTH $UpdateMode <> 1 and $EXEDIR != $INSTDIR", async () => {
    const nsi = await read(HOOKS);
    const guardOpen = nsi.search(/\$\{If\}\s*\$UpdateMode\s*<>\s*1/);
    const andGuard = nsi.search(/\$\{AndIf\}\s*\$\d\s*!=\s*\$\d/);
    const delstore = nsi.search(/-delstore\s+Root/i);
    const rmdir = nsi.search(/RMDir\s+\/r/i);
    // The LAST ${EndIf} closes the destructive block; earlier ones close the path canonicalization.
    const guardClose = nsi.lastIndexOf("${EndIf}");

    expect(guardOpen, "an ${If} $UpdateMode <> 1 guard must exist").toBeGreaterThanOrEqual(0);
    expect(
      andGuard,
      "the in-place-vs-temp-copy result must be ANDed into the SAME condition",
    ).toBeGreaterThan(guardOpen);
    expect(delstore, "-delstore Root must sit AFTER both guards").toBeGreaterThan(andGuard);
    expect(rmdir, "the cert RMDir must sit AFTER both guards").toBeGreaterThan(andGuard);
    expect(guardClose, "the guard must close AFTER the destructive ops").toBeGreaterThan(delstore);
    expect(guardClose).toBeGreaterThan(rmdir);
  });

  test("both directories are canonicalized before they are compared", async () => {
    // Comparing $EXEDIR to $INSTDIR raw would let one directory spelled two ways (casing, trailing
    // slash, 8.3 short name) read as "different" — i.e. as a real uninstall — and wipe the anchor.
    const nsi = await read(HOOKS);
    const normalized = [
      ...nsi.matchAll(/GetFullPathName\s+\/SHORT\s+\$\d\s+"\$(EXEDIR|INSTDIR)"/g),
    ];
    expect(
      normalized.map((m) => m[1]).sort(),
      "both $EXEDIR and $INSTDIR must be canonicalized",
    ).toEqual(["EXEDIR", "INSTDIR"]);
    // Raw $EXEDIR/$INSTDIR must never be the operands of the guard comparison itself.
    expect(nsi).not.toMatch(/\$\{AndIf\}\s*"?\$EXEDIR/);
  });
});

// The desktop-ui layout specs size the page to the REAL Tauri window, because Playwright's default
// 1280x720 viewport made every clipping bug invisible — the speed slider cut off at the window's
// bottom edge and the onboarding height both had to be caught by hand. Those specs are only as
// truthful as their constants, so pin them to windows.rs.
describe("desktop-ui layout specs use the real window sizes", () => {
  test("e2e/window-sizes.ts matches src-tauri/src/windows.rs", async () => {
    const rs = await read(path.join(SRC_TAURI, "src", "windows.rs"));
    const ts = await read(path.join(SRC_TAURI, "..", "e2e", "window-sizes.ts"));

    // Each WindowConfig literal: label (or the auth popup's `&label`), then width/height. Comments sit
    // between the fields, so match non-greedily across them.
    const configs = [
      ...rs.matchAll(
        /label:\s*(?:"(?<label>[a-z-]+)"|&label)[\s\S]*?width:\s*(?<w>[\d.]+)[\s\S]*?height:\s*(?<h>[\d.]+)/g,
      ),
    ].map((m) => ({
      // The auth popup's label is built per request (`auth-<id>`); the specs key it as "authorize".
      label: m.groups?.label ?? "authorize",
      width: Number(m.groups?.w),
      height: Number(m.groups?.h),
    }));
    expect(configs.length, "should find every WindowConfig in windows.rs").toBeGreaterThanOrEqual(
      5,
    );

    for (const { label, width, height } of configs) {
      const entry = ts.match(
        new RegExp(`"?${label}"?:\\s*\\{\\s*width:\\s*(\\d+),\\s*height:\\s*(\\d+)`),
      );
      expect(entry, `window-sizes.ts is missing "${label}"`).not.toBeNull();
      expect(Number(entry?.[1]), `${label} width`).toBe(width);
      expect(Number(entry?.[2]), `${label} height`).toBe(height);
    }
  });
});
