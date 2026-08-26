/**
 * WCAG contrast guard over the SHIPPED token sheets — parses the real CSS custom-property
 * declarations (no duplicated constants) and fails any text-role pair under 4.5:1 in either
 * theme. `silk`/`go`/`gold` are non-text by design; their `-text` twins carry text.
 * Covers the converted surfaces: the Tauri app + the playground (landing joins in its phase).
 */
import { describe, expect, test } from "bun:test";
import { join, resolve } from "node:path";

const PKG_DIR = resolve(import.meta.dir, "..");

interface Themes {
  light: Record<string, string>;
  dark: Record<string, string>;
}

/** First block = light `:root`/@theme; the prefers-color-scheme dark block = dark. */
async function parseThemes(cssPath: string, prefix: string): Promise<Themes> {
  const css = await Bun.file(cssPath).text();
  const varRe = new RegExp(`(${prefix}[a-z-]+)\\s*:\\s*(#[0-9a-fA-F]{6})`, "g");
  const darkStart = css.indexOf("@media (prefers-color-scheme: dark)");
  if (darkStart < 0) throw new Error(`${cssPath}: no dark theme block`);
  const light: Record<string, string> = {};
  const dark: Record<string, string> = {};
  for (const m of css.slice(0, darkStart).matchAll(varRe)) light[m[1]] = m[2].toLowerCase();
  const darkEnd = css.indexOf("}", css.indexOf("}", darkStart) + 1) + 1;
  for (const m of css.slice(darkStart, darkEnd).matchAll(varRe)) dark[m[1]] = m[2].toLowerCase();
  return { light, dark };
}

function luminance(hex: string): number {
  const c = (i: number) => {
    const v = Number.parseInt(hex.slice(i, i + 2), 16) / 255;
    return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * c(1) + 0.7152 * c(3) + 0.0722 * c(5);
}

function contrast(a: string, b: string): number {
  const [l1, l2] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (l1 + 0.05) / (l2 + 0.05);
}

function assertPairs(themes: Themes, pairs: Array<[fg: string, bg: string]>, label: string): void {
  for (const themeName of ["light", "dark"] as const) {
    const t = themes[themeName];
    for (const [fg, bg] of pairs) {
      const fgHex = t[fg];
      const bgHex = t[bg];
      expect(fgHex, `${label}/${themeName}: missing ${fg}`).toBeDefined();
      expect(bgHex, `${label}/${themeName}: missing ${bg}`).toBeDefined();
      const ratio = contrast(fgHex as string, bgHex as string);
      expect(`${label}/${themeName} ${fg} on ${bg} = ${ratio.toFixed(2)}`).toBe(
        `${label}/${themeName} ${fg} on ${bg} = ${ratio.toFixed(2)}`,
      );
      if (ratio < 4.5) {
        throw new Error(`${label}/${themeName}: ${fg} on ${bg} is ${ratio.toFixed(2)}:1 (< 4.5)`);
      }
    }
  }
}

describe("contrast guard", () => {
  test("app token sheet: every text role clears 4.5:1 in both themes", async () => {
    const themes = await parseThemes(join(PKG_DIR, "src-tauri", "frontend", "style.css"), "--");
    assertPairs(
      themes,
      [
        ["--text", "--bg"],
        ["--text", "--surface"],
        ["--text-muted", "--bg"],
        ["--text-muted", "--surface"],
        ["--accent", "--bg"],
        ["--accent-on-dark", "--accent"],
        ["--danger", "--bg"],
        ["--danger", "--surface"],
        ["--danger-contrast", "--danger"],
        ["--warning", "--bg"],
        ["--go-text", "--bg"],
      ],
      "app",
    );
  });

  test("playground token sheet: every text role clears 4.5:1 in both themes", async () => {
    const themes = await parseThemes(
      resolve(PKG_DIR, "..", "playground", "src", "style.css"),
      "--color-brand-",
    );
    assertPairs(
      themes,
      [
        ["--color-brand-text", "--color-brand-bg"],
        ["--color-brand-text", "--color-brand-surface"],
        ["--color-brand-text-muted", "--color-brand-bg"],
        ["--color-brand-text-muted", "--color-brand-surface"],
        ["--color-brand-accent", "--color-brand-bg"],
        ["--color-brand-accent-on", "--color-brand-accent"],
        ["--color-brand-warning", "--color-brand-bg"],
        ["--color-brand-danger", "--color-brand-bg"],
        ["--color-brand-go-text", "--color-brand-bg"],
      ],
      "playground",
    );
  });
});
