/**
 * Aztec protocol compatibility is MAJOR-scoped: any 5.x.x aztec.js talks to any
 * 5.x.x node; only a major difference (5.x.x vs 6.x.x) breaks the wire contract.
 * (Distinct from the accelerator's bb pairing, which is exact-version by design.)
 */

/** Major component of a semver-ish string (`5.2.0`, `v5.2.0`, `^5.2.0`, `5.2.0-rc.1` → `"5"`). */
export function majorOf(version: string): string | undefined {
  return /^[v^~]*(\d+)\./.exec(version.trim())?.[1];
}

/**
 * True when both versions parse and share a major. `undefined` when either is
 * unparseable (e.g. the `"unknown"` sentinel) — compatibility can't be judged,
 * so callers should stay quiet rather than warn on garbage.
 */
export function sameMajor(a: string, b: string): boolean | undefined {
  const ma = majorOf(a);
  const mb = majorOf(b);
  if (!ma || !mb) return undefined;
  return ma === mb;
}
