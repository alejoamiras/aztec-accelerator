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

// Fetch latest release for the specific platform — best effort, non-blocking
async function fetchLatestAcceleratorTag(): Promise<string | null> {
  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases`, {
      signal: AbortSignal.timeout(3000),
    });
    if (!res.ok) return null;
    const releases = (await res.json()) as { tag_name: string; assets: unknown[] }[];
    const accel = releases.find(
      (r) => r.tag_name.startsWith("accelerator-") && r.assets.length > 0,
    );
    return accel?.tag_name ?? null;
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
