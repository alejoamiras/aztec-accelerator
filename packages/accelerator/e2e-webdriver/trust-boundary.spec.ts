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

/** Invoke a command through the REAL injected primitive in the active window and capture resolve/reject. */
function invokeHere(cmd: string, args: Record<string, unknown>): Promise<InvokeResult> {
  return browser.executeAsync(
    (c: string, a: Record<string, unknown>, done: (r: InvokeResult) => void) => {
      const inv = (window as unknown as { __TAURI_INTERNALS__?: { invoke?: unknown } })
        .__TAURI_INTERNALS__?.invoke as
        | ((cmd: string, args: unknown) => Promise<unknown>)
        | undefined;
      if (typeof inv !== "function") {
        done({ resolved: false, hasPrimitive: false });
        return;
      }
      inv(c, a)
        .then((v: unknown) => done({ resolved: true, value: v, hasPrimitive: true }))
        .catch((e: unknown) => done({ resolved: false, error: String(e), hasPrimitive: true }));
    },
    cmd,
    args,
  );
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
    // Inline script must NOT execute (script-src 'self', no unsafe-inline / hash for runtime-injected code).
    const inline = await browser.executeAsync(
      (done: (r: { ran: boolean; violation: boolean }) => void) => {
        let violation = false;
        document.addEventListener("securitypolicyviolation", () => {
          violation = true;
        });
        const s = document.createElement("script");
        s.textContent = "window.__CSP_INLINE_RAN__ = true;";
        document.head.appendChild(s);
        setTimeout(
          () =>
            done({
              ran:
                (window as unknown as { __CSP_INLINE_RAN__?: boolean }).__CSP_INLINE_RAN__ === true,
              violation,
            }),
          300,
        );
      },
    );
    expect(inline.ran).toBe(false);
    expect(inline.violation).toBe(true);

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

    // Off-origin fetch is blocked (connect-src is ipc: only, no 'self', no remote).
    const fetchOutcome = await browser.executeAsync((done: (r: string) => void) => {
      fetch("https://trust-boundary-exfil.example.com/")
        .then(() => done("reached"))
        .catch(() => done("blocked"));
    });
    expect(fetchOutcome).toBe("blocked");
  });

  it("remove_approved_origin is a REAL, mutating command from its authorized Settings window", async () => {
    // Prove the negative-test target exists and does something — from the window that IS allowed it.
    const res = await invokeHere("remove_approved_origin", {
      origin: "https://not-present-benign.example.com",
    });
    expect(res.hasPrimitive).toBe(true);
    expect(res.resolved).toBe(true); // real command, granted to Settings → resolves (no-op on absent origin)
  });

  it("cross-window: the auth popup is DENIED a Settings command by the ACL (isolated capability proof)", async () => {
    const before = await browser.getWindowHandles();
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

      // Canary: capture Settings-owned state before the forbidden attempt.
      const originsBefore = JSON.stringify(readConfig().approved_origins ?? []);

      // The attack: a Settings-only command from the auth popup. The per-window capability ACL rejects it
      // BEFORE dispatch — so the error is the ACL's "not allowed on window …" (debug-build form), NOT the
      // Rust caller-label "not available" message. Matching the ACL wording proves the capability layer
      // enforces (a broken ACL that fell through to the Rust label would surface a different message).
      const denied = await invokeHere("remove_approved_origin", { origin: TEST_ORIGIN });
      expect(denied.hasPrimitive).toBe(true); // not a spurious pass from a missing primitive
      expect(denied.resolved).toBe(false);
      expect(denied.error ?? "").toContain("not allowed"); // ACL reason, not the Rust label reason
      expect(denied.error ?? "").not.toContain("not available from this window"); // would mean ACL fell through

      // State canary: the denied call executed nothing.
      const originsAfter = JSON.stringify(readConfig().approved_origins ?? []);
      expect(originsAfter).toBe(originsBefore);

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
