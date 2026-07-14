/**
 * F-012 trust-boundary proof (real webview + real IPC + real CSP/ACL — no mocks).
 *
 * This is THE test that fails loudly if the boundary is actually open. It proves, against a running app:
 *  1. withGlobalTauri:false — there is no `window.__TAURI__` global back-door.
 *  2. The injected IPC primitive still works for a GRANTED command (so later rejections aren't false).
 *  3. The strict CSP blocks inline scripts, eval, and off-origin fetch (but not normal app IPC).
 *  4. Cross-window ACL denial (the capability layer, isolated): the authorization popup is REJECTED when it
 *     invokes a Settings-only command — with the ACL's own "not allowed" reason (distinct from the Rust
 *     caller-label error), and with the target command proven REAL + mutating from its authorized window
 *     first (final-codex MED: an allowed call from the attacker window doesn't prove the forbidden command
 *     name is real). A state canary confirms the denied call had no effect.
 *
 * Runs in `mode: dev` (3-OS PR matrix) AND the built-debug custom-protocol lane. Everything goes through the
 * injected `window.__TAURI_INTERNALS__.invoke` — a hand-rolled postMessage is dropped before the ACL
 * (invoke-key pre-check) and would test the wrong boundary.
 */
import * as os from "node:os";
import { readConfig } from "./helpers.ts";

const IS_LINUX = os.platform() === "linux";
const SETTINGS_TITLE = "Aztec Accelerator Settings";
const TEST_ORIGIN = "https://trust-boundary-e2e.example.com";
const PROVE_URL = "http://127.0.0.1:59833/prove";

interface InvokeResult {
  resolved: boolean;
  value?: unknown;
  error?: string;
  hasPrimitive: boolean;
}

/**
 * Invoke a command through the REAL injected primitive and capture resolve/reject.
 *
 * WebKitGTK's WebDriver (via tauri-plugin-webdriver) rejects `execute/async` ("Origin header is not a valid
 * URL"), so we kick off the async invoke with a SYNC execute that stashes the outcome on a global, then poll
 * that global with another sync execute.
 */
async function invokeHere(cmd: string, args: Record<string, unknown>): Promise<InvokeResult> {
  await browser.execute(
    (c: string, a: Record<string, unknown>) => {
      const w = window as unknown as {
        __INVOKE_RESULT__?: InvokeResult;
        __TAURI_INTERNALS__?: { invoke?: (cmd: string, args: unknown) => Promise<unknown> };
      };
      w.__INVOKE_RESULT__ = undefined;
      const inv = w.__TAURI_INTERNALS__?.invoke;
      if (typeof inv !== "function") {
        w.__INVOKE_RESULT__ = { resolved: false, hasPrimitive: false };
        return;
      }
      inv(c, a)
        .then((v: unknown) => {
          w.__INVOKE_RESULT__ = { resolved: true, value: v, hasPrimitive: true };
        })
        .catch((e: unknown) => {
          w.__INVOKE_RESULT__ = { resolved: false, error: String(e), hasPrimitive: true };
        });
    },
    cmd,
    args,
  );
  let res: InvokeResult | null = null;
  await browser.waitUntil(
    async () => {
      res = await browser.execute(
        () => (window as unknown as { __INVOKE_RESULT__?: InvokeResult }).__INVOKE_RESULT__ ?? null,
      );
      return res !== null;
    },
    { timeout: 8000, interval: 100, timeoutMsg: `invoke(${cmd}) did not settle` },
  );
  return res as unknown as InvokeResult;
}

async function switchToSettings(): Promise<string> {
  for (const h of await browser.getWindowHandles()) {
    await browser.switchToWindow(h);
    if ((await browser.getTitle()) === SETTINGS_TITLE) {
      await browser.$("#speed-label").waitForExist({ timeout: 5000 });
      return h;
    }
  }
  throw new Error("Settings bootstrap window not found");
}

function fireProve(origin: string): Promise<Response> {
  return fetch(PROVE_URL, {
    method: "POST",
    headers: { "Content-Type": "application/octet-stream", Origin: origin },
    body: new Uint8Array([0]),
  });
}

async function waitForNewWindow(existing: string[]): Promise<string | null> {
  for (let i = 0; i < 30; i++) {
    await browser.pause(500);
    const now = await browser.getWindowHandles();
    const fresh = now.find((h) => !existing.includes(h));
    if (fresh) return fresh;
  }
  return null;
}

describe("Trust boundary (F-012)", () => {
  let settingsHandle: string;

  before(async () => {
    settingsHandle = await switchToSettings();
  });

  afterEach(async () => {
    // Return to Settings and close any stray popup so specs stay isolated.
    for (const h of await browser.getWindowHandles()) {
      if (h !== settingsHandle) {
        await browser.switchToWindow(h);
        await browser.closeWindow().catch(() => {});
      }
    }
    await browser.switchToWindow(settingsHandle).catch(() => {});
  });

  it("exposes no window.__TAURI__ global (withGlobalTauri:false)", async () => {
    const hasGlobal = await browser.execute(
      () => typeof (window as unknown as { __TAURI__?: unknown }).__TAURI__ !== "undefined",
    );
    expect(hasGlobal).toBe(false);
  });

  it("a GRANTED settings command resolves through the injected primitive", async () => {
    const res = await invokeHere("get_config", {});
    expect(res.hasPrimitive).toBe(true);
    expect(res.resolved).toBe(true); // proves the IPC path works from Settings — later denials are real
  });

  it("the strict CSP blocks inline script, eval, and off-origin fetch — but not app IPC", async () => {
    // Arm CSP-violation listeners + trigger inline-script injection and an off-origin fetch, stashing outcomes
    // on globals (sync execute only — WebKitGTK rejects execute/async), then read them after a pause.
    await browser.execute(() => {
      const w = window as unknown as {
        __CSP__?: { inlineViolation: boolean; connectViolation: boolean };
        __CSP_INLINE_RAN__?: boolean;
      };
      w.__CSP__ = { inlineViolation: false, connectViolation: false };
      document.addEventListener("securitypolicyviolation", (e) => {
        const ev = e as SecurityPolicyViolationEvent;
        const dir = ev.effectiveDirective || ev.violatedDirective || "";
        if (dir.includes("script-src")) w.__CSP__!.inlineViolation = true;
        if (dir.includes("connect-src")) w.__CSP__!.connectViolation = true;
      });
      // Inline script must NOT execute (script-src 'self', no unsafe-inline / hash for injected code).
      const s = document.createElement("script");
      s.textContent = "window.__CSP_INLINE_RAN__ = true;";
      document.head.appendChild(s);
      // Off-origin fetch is blocked BY CSP (connect-src is ipc: only).
      fetch("https://trust-boundary-exfil.example.com/").catch(() => {});
    });
    await browser.pause(500);
    const csp = await browser.execute(() => {
      const w = window as unknown as {
        __CSP__?: { inlineViolation: boolean; connectViolation: boolean };
        __CSP_INLINE_RAN__?: boolean;
      };
      return { ...w.__CSP__, inlineRan: w.__CSP_INLINE_RAN__ === true };
    });
    expect(csp.inlineRan).toBe(false); // injected inline script did not run
    expect(csp.inlineViolation).toBe(true);
    // Assert a connect-src violation specifically — a bare fetch rejection could be DNS/TLS and prove nothing.
    expect(csp.connectViolation).toBe(true);

    // eval is blocked (no unsafe-eval).
    const evalOutcome = await browser.execute(() => {
      try {
        // biome-ignore lint/security/noGlobalEval: deliberately testing that CSP blocks eval
        window.eval("1+1");
        return "ran";
      } catch {
        return "threw";
      }
    });
    expect(evalOutcome).toBe("threw");
  });

  it("blocks off-origin navigation and window.open (Rust on_navigation/on_new_window)", async () => {
    // The nav guards are what MEDIUM-1 sharpened — assert the webview stays put and opens no window.
    const urlBefore = await browser.getUrl();
    const handlesBefore = (await browser.getWindowHandles()).length;
    await browser.execute(() => {
      try {
        window.location.assign("https://trust-boundary-nav.example.com/?exfil=1");
      } catch {}
      try {
        window.open("https://trust-boundary-nav.example.com/", "_blank");
      } catch {}
    });
    await browser.pause(600);
    expect(await browser.getUrl()).toBe(urlBefore); // navigation was denied — still on the same page
    expect((await browser.getWindowHandles()).length).toBe(handlesBefore); // no new window opened
  });

  it("set_speed is a REAL, mutating command from its authorized Settings window", async () => {
    // Prove the cross-window target actually mutates (not a no-op) — from the window that IS allowed it.
    const before = (readConfig().speed as string) || "full";
    const target = before === "full" ? "balanced" : "full";
    const res = await invokeHere("set_speed", { speed: target });
    expect(res.hasPrimitive).toBe(true);
    expect(res.resolved).toBe(true);
    expect(readConfig().speed).toBe(target); // real mutation observed in the persisted config
    // restore
    await invokeHere("set_speed", { speed: before });
    expect(readConfig().speed).toBe(before);
  });

  it("cross-window: the auth popup is DENIED a Settings command by the ACL (isolated capability proof)", async () => {
    const before = await browser.getWindowHandles();
    const speedBefore = (readConfig().speed as string) || "full";
    const attackSpeed = speedBefore === "full" ? "balanced" : "full"; // a value that WOULD change state
    // Fire /prove to open a REAL auth popup (blocks until resolved / 60s timeout).
    const pending = fireProve(TEST_ORIGIN);
    try {
      const authHandle = await waitForNewWindow(before);
      expect(authHandle).not.toBeNull();
      await browser.switchToWindow(authHandle as string);
      await browser.$("#origin").waitForExist({ timeout: 5000 });

      // Sanity: the auth window CAN invoke its own granted command (primitive works here too).
      const allowed = await invokeHere("get_verified_info", { origin: TEST_ORIGIN });
      expect(allowed.hasPrimitive).toBe(true);
      expect(allowed.resolved).toBe(true);

      // The attack: set_speed (a Settings-only command that WOULD mutate) from the auth popup. The per-window
      // capability ACL rejects it BEFORE dispatch — so the error is the ACL's "not allowed on window …"
      // (debug-build form), NOT the Rust caller-label "not available" message. Matching the ACL wording
      // proves the capability layer enforces (a broken ACL that fell through to the Rust label would differ).
      const denied = await invokeHere("set_speed", { speed: attackSpeed });
      expect(denied.hasPrimitive).toBe(true); // not a spurious pass from a missing primitive
      expect(denied.resolved).toBe(false);
      expect(denied.error ?? "").toContain("not allowed"); // ACL reason, not the Rust label reason
      expect(denied.error ?? "").not.toContain("not available from this window"); // would mean ACL fell through

      // Strong canary: had the call executed, speed would now be attackSpeed. It must be unchanged.
      expect(readConfig().speed).toBe(speedBefore);

      // Resolve the popup (Deny) so the pending /prove returns and the window closes.
      if (IS_LINUX) {
        await browser
          .execute(() => (document.getElementById("deny") as HTMLElement | null)?.click())
          .catch(() => {});
      } else {
        await (await browser.$("#deny")).click().catch(() => {});
      }
    } finally {
      await pending.catch(() => {});
      await browser.switchToWindow(settingsHandle).catch(() => {});
    }
  });
});
