// B6: the signed auto-update feed is the single source of truth for the live STABLE version — an explicit
// promote step (release-accelerator.yml) flips `latest.json` only after the release gates pass, so the feed
// never carries an unpromoted RC. The landing download button derives its version from the feed instead of
// scanning the GitHub releases API (which had no prerelease filter and could surface an RC as the download).
//
// SECURITY: the browser does NOT verify the feed's Ed25519 signature, so the payload shape is UNTRUSTED.
// Accept ONLY a strict stable-SemVer `version`, and splice it solely into the canonical GitHub release TAG
// path segment — never interpolate a feed field into a URL host. Asset download URLs still come from the
// GitHub API's own `browser_download_url` (GitHub-origin), never from the feed.

// Same-origin on the deployed site (aztec-accelerator.dev/releases/latest.json); 404s harmlessly in local
// dev → the caller falls back to the GitHub releases page.
export const FEED_URL = "/releases/latest.json";

// The promoted feed only ever carries a stable X.Y.Z — reject prereleases and anything non-SemVer. This is
// both the untrusted-input guard and the fix for "an RC shows up as the download".
const STABLE_SEMVER = /^\d+\.\d+\.\d+$/;

/**
 * Map an untrusted parsed feed body to the canonical release tag `accelerator-v<version>`, or `null` if the
 * body has no strict stable-SemVer `version`. Pure + side-effect-free so it can be unit-tested without a DOM.
 */
export function feedVersionToTag(data: unknown): string | null {
  if (!data || typeof data !== "object") return null;
  const version = (data as { version?: unknown }).version;
  if (typeof version !== "string" || !STABLE_SEMVER.test(version)) return null;
  return `accelerator-v${version}`;
}
