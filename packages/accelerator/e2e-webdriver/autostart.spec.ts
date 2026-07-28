/**
 * L7 — autostart self-heal through REAL IPC (plan §6).
 *
 * The one layer that exercises `repair_autostart` through the real capability ACL in the real
 * binary: L5 mocks both sides of the IPC seam, so a forgotten `allow-repair-autostart` grant, a
 * missing build.rs app-manifest entry, or serde camelCase drift ships green through L1–L6 — this
 * spec is what catches those.
 *
 * Self-contained and ordered LAST: it seeds a broken artifact from Node, REFRESHES the settings
 * window so the page re-reads status, drives Fix, asserts the on-disk artifact healed, and cleans
 * up — earlier specs (which assert the switch unchecked) never see the seeded state.
 *
 * Windows is SKIPPED by choice, not impossibility (plan §6 r4): HKCU is global — a throwaway HOME
 * does not isolate the registry, and seeding the real Run value here would also arm the intent-keyed
 * crash-recovery rearm on the runner. Windows end-to-end lives in L6 (Build Smoke) + L3.
 */
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { ensureSettingsWindow } from "./helpers.js";

const PLATFORM = os.platform();

function artifactPath(): string {
  if (PLATFORM === "darwin") {
    return path.join(os.homedir(), "Library/LaunchAgents", "Aztec Accelerator.plist");
  }
  // D9: production honours XDG_CONFIG_HOME; the harness leaves it unset, so ~/.config.
  const configDir = process.env.XDG_CONFIG_HOME ?? path.join(os.homedir(), ".config");
  return path.join(configDir, "autostart", "Aztec Accelerator.desktop");
}

/** Seed EXACTLY what the removed plugin used to write: an UNQUOTED spaced path (dead) — legacy
 *  format in, owned quoted format out. */
function seedBrokenArtifact(): void {
  const dead = path.join(os.homedir(), "definitely gone", "Aztec Accelerator");
  const file = artifactPath();
  fs.mkdirSync(path.dirname(file), { recursive: true });
  if (PLATFORM === "darwin") {
    fs.writeFileSync(
      file,
      `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
  <key>Label</key>
  <string>Aztec Accelerator</string>
  <key>ProgramArguments</key>
  <array><string>${dead}</string></array>
  <key>RunAtLoad</key>
  <true/>
  </dict>
</plist>`,
    );
  } else {
    fs.writeFileSync(
      file,
      `[Desktop Entry]\nType=Application\nVersion=1.0\nName=Aztec Accelerator\nComment=Aztec Accelerator startup script\nExec=${dead} \nStartupNotify=false\nTerminal=false`,
    );
  }
}

function readStoredProgram(): string {
  const raw = fs.readFileSync(artifactPath(), "utf-8");
  if (PLATFORM === "darwin") {
    const m = raw.match(/<key>ProgramArguments<\/key>\s*<array>\s*<string>([^<]*)<\/string>/);
    if (!m) throw new Error(`no ProgramArguments in healed plist:\n${raw}`);
    return m[1];
  }
  const m = raw.match(/^Exec=(.*)$/m);
  if (!m) throw new Error(`no Exec in healed .desktop:\n${raw}`);
  const value = m[1].trim();
  if (!value.startsWith('"')) throw new Error(`healed Exec must be QUOTED, got: ${value}`);
  // Our owned quoting: unwrap + unescape \" \` \$ \\ and undo %%.
  const inner = value.slice(1, value.lastIndexOf('"'));
  return inner.replace(/\\(["`$\\])/g, "$1").replace(/%%/g, "%");
}

describe("Autostart self-heal (real IPC)", () => {
  if (PLATFORM === "win32") {
    it.skip("skipped on Windows — HKCU is global (see header); covered by L3 + L6", () => {});
    return;
  }

  before(async () => {
    seedBrokenArtifact();
    await ensureSettingsWindow();
    // The page read status at load, BEFORE we seeded — refresh so loadSettings() re-runs and the
    // real get_autostart_enabled sees the broken artifact. (The STARTUP heal is webdriver-gated
    // off — S1 — so nothing repaired it behind our back; that gating is itself what makes this
    // spec deterministic.)
    await browser.refresh();
    await browser.$("#speed-label").waitForExist({ timeout: 5000 });
  });

  after(() => {
    // Leave no trace for reruns (the artifact is healed, i.e. present, after the happy path).
    fs.rmSync(artifactPath(), { force: true });
  });

  it("shows intent ON + the health row + Fix for a seeded broken entry", async () => {
    // D17: the switch reflects INTENT — a broken entry is still "the user wants autostart".
    const toggle = await browser.$("#autostart");
    await toggle.waitForExist({ timeout: 5000 });
    expect(await toggle.isSelected()).toBe(true);

    const row = await browser.$("#autostart-health");
    await row.waitForDisplayed({ timeout: 5000 });
    expect(await browser.$("#autostart-health-text").getText()).toContain("no longer exists");
    // canRepairNow: the app is running from a live path, so Fix must be offered (D14).
    await expect(browser.$("#autostart-fix")).toBeDisplayed();
  });

  it("Fix heals the artifact ON DISK through the real repair_autostart command", async () => {
    await browser.$("#autostart-fix").click();

    // The row hides once the returned status is healthy — the IPC round-trip completing is the
    // ACL proof (a missing allow-repair-autostart grant rejects before the handler runs).
    await browser.$("#autostart-health").waitForDisplayed({ reverse: true, timeout: 10_000 });

    const program = readStoredProgram();
    expect(fs.existsSync(program)).toBe(true);
    expect(path.basename(program).startsWith("aztec-accelerator")).toBe(true);

    if (PLATFORM === "darwin") {
      // The intent-keyed startup rearm patched KeepAlive into the seeded plist at launch; the heal
      // must have preserved it (C1) — the full crash-recovery composition, end to end.
      const raw = fs.readFileSync(artifactPath(), "utf-8");
      expect(raw.includes("<key>KeepAlive</key>")).toBe(true);
    }

    // Switch stays ON, still intent (D13).
    expect(await browser.$("#autostart").isSelected()).toBe(true);
  });

  it("a second Fix is a no-op (convergent), and the artifact is untouched", async () => {
    const before = fs.readFileSync(artifactPath(), "utf-8");
    // The button is hidden now (healthy) — invoke the command through the page's own injected
    // primitive instead, proving NotNeeded → status round-trips cleanly too. WebKitGTK's WebDriver
    // REJECTS execute/async ("Origin header is not a valid URL" — see trust-boundary.spec.ts), so
    // this uses the same sync-execute-stash-then-poll pattern.
    await browser
      .execute(() => {
        const w = window as unknown as {
          __REPAIR_RESULT__?: unknown;
          __TAURI_INTERNALS__?: { invoke?: (cmd: string) => Promise<unknown> };
        };
        w.__REPAIR_RESULT__ = undefined;
        const inv = w.__TAURI_INTERNALS__?.invoke;
        if (typeof inv !== "function") {
          w.__REPAIR_RESULT__ = { error: "no injected primitive" };
          return;
        }
        inv("repair_autostart")
          .then((s: unknown) => {
            w.__REPAIR_RESULT__ = s;
          })
          .catch((e: unknown) => {
            w.__REPAIR_RESULT__ = { error: String(e) };
          });
      })
      .catch(() => {});
    let status: Record<string, unknown> | null = null;
    await browser.waitUntil(
      async () => {
        status = await browser.execute(
          () =>
            ((window as unknown as { __REPAIR_RESULT__?: unknown }).__REPAIR_RESULT__ ??
              null) as Record<string, unknown> | null,
        );
        return status !== null;
      },
      { timeout: 8000, interval: 100, timeoutMsg: "repair_autostart did not settle" },
    );
    const st = status as unknown as Record<string, unknown>;
    expect(st.error).toBeUndefined();
    expect(st.healthy).toBe(true);
    expect(fs.readFileSync(artifactPath(), "utf-8")).toBe(before);
  });
});
