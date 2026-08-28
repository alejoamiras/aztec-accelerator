import {
  detectAccelerator,
  type LandingAcceleratorStatus,
  LandingDetectionController,
  watchLoopbackPermissionChanges,
} from "./accelerator-detection";
import { FEED_URL, feedVersionToTag } from "./feed";

// ── Scroll reveals ──
const observer = new IntersectionObserver(
  (entries) => {
    for (const entry of entries) {
      if (entry.isIntersecting) {
        entry.target.classList.add("revealed");
        observer.unobserve(entry.target);
      }
    }
  },
  { threshold: 0.15 },
);

for (const el of document.querySelectorAll(".reveal")) {
  observer.observe(el);
}

// ── Mouse-reactive ambient glow ──
const glow = document.querySelector(".hero-ambient") as HTMLElement | null;
if (glow) {
  document.addEventListener("mousemove", (e) => {
    glow.style.left = `${e.clientX}px`;
    glow.style.top = `${e.clientY}px`;
  });
}

// ── OS-aware download button ──
const REPO = "alejoamiras/aztec-accelerator";
const RELEASES_URL = `https://github.com/${REPO}/releases`;

interface OsInfo {
  label: string;
  pattern: RegExp;
}

function detectOs(): OsInfo {
  const ua = navigator.userAgent;
  if (/Mac/.test(ua)) {
    // navigator.platform is deprecated but still the most reliable
    // way to distinguish Apple Silicon from Intel in-browser
    const isArm =
      /arm64|aarch64/i.test(navigator.userAgent) ||
      (navigator as any).userAgentData?.architecture === "arm" ||
      // Safari + Chrome on Apple Silicon report this
      (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
    return isArm
      ? { label: "Download for macOS (Apple Silicon)", pattern: /Apple-Silicon\.dmg$/ }
      : { label: "Download for macOS", pattern: /macOS.*\.dmg$/ };
  }
  if (/Linux/.test(ua)) {
    return { label: "Download for Linux", pattern: /\.AppImage$/ };
  }
  // Windows or unknown — point to releases page
  return { label: "Download", pattern: /^$/ };
}

// Resolve the live stable tag from the SIGNED S3 feed (single source of truth — B6), NOT a GitHub releases
// list-scan (which had no prerelease filter). Best effort, non-blocking; the feed body is untrusted and
// validated in `feedVersionToTag`.
async function fetchLatestAcceleratorTag(): Promise<string | null> {
  try {
    const res = await fetch(FEED_URL, { signal: AbortSignal.timeout(3000) });
    if (!res.ok) return null;
    return feedVersionToTag(await res.json());
  } catch {
    return null;
  }
}

async function initDownload(): Promise<void> {
  const btn = document.getElementById("download-btn") as HTMLAnchorElement | null;
  if (!btn) return;

  const os = detectOs();
  btn.textContent = os.label;

  const tag = await fetchLatestAcceleratorTag();
  if (!tag) return;

  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/tags/${tag}`, {
      signal: AbortSignal.timeout(3000),
    });
    if (!res.ok) return;
    const data = await res.json();
    const asset = (data.assets as { name: string; browser_download_url: string }[])?.find((a) =>
      os.pattern.test(a.name),
    );
    if (asset) {
      btn.href = asset.browser_download_url;
    } else {
      btn.href = `${RELEASES_URL}/tag/${tag}`;
    }
  } catch {
    // Fall back to releases page
  }
}

initDownload();

// ── Accelerator detection ──
const heroSub = document.querySelector(".hero-sub") as HTMLElement | null;
const heroLink = heroSub?.querySelector("a") as HTMLAnchorElement | null;
const originalHeroLink = heroLink?.innerHTML ?? "";
const httpsOnly = new URLSearchParams(window.location.search).get("httpsOnly") === "true";

function renderAcceleratorStatus(status: LandingAcceleratorStatus): void {
  const blocked = status === "permission-blocked";
  document.getElementById("landing-permission-help")?.classList.toggle("hidden", !blocked);
  document.getElementById("download-actions")?.classList.toggle("hidden", blocked);

  if (!heroSub || !heroLink) return;
  if (status === "available") {
    heroSub.classList.add("detected");
    heroLink.innerHTML =
      '<span class="accel-dot" aria-hidden="true"></span> Accelerator detected — Open the Playground <span>&rarr;</span>';
  } else {
    // Offline and generic error remain deliberately quiet: restore the unchanged landing CTA.
    heroSub.classList.remove("detected");
    heroLink.innerHTML = originalHeroLink;
  }
}

const detection = new LandingDetectionController(
  () => detectAccelerator({ httpsOnly }),
  renderAcceleratorStatus,
  (pending) => {
    const button = document.getElementById("landing-accelerator-retry") as HTMLButtonElement | null;
    if (!button) return;
    button.disabled = pending;
    button.textContent = pending ? "Checking…" : "Retry";
  },
);

document.getElementById("landing-accelerator-retry")?.addEventListener("click", () => {
  // The detector has no settled cache. The token guard ensures a late startup result cannot overwrite
  // this same-context recovery attempt.
  void detection.refresh().catch(() => {});
});

void (async () => {
  // Subscribe before the first health request can open the browser prompt. A decision made after the
  // bounded probe expires must still replace the quiet offline/download state without a reload.
  await watchLoopbackPermissionChanges(() => {
    void detection.refreshAfterPermissionChange().catch(() => {});
  });
  await detection.refresh();
})().catch(() => {});
