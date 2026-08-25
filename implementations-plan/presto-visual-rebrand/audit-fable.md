# Fable audit — round 1 (top-tier Claude architectural reviewer, verdict: conditional approve)

Verdict: **conditional approve** (with conditions: 1. correct the icns/ico container spec — the
committed artifacts use ARGB ic04/ic05 and BMP ico entries, not "PNG payloads", and counts are 25
include_bytes / 30 load-bearing files / 10 icns image chunks — either adopt the already-available
`bunx tauri icon` for the app-icon ladder or fix the hand-writer spec and its validation test, with
resvg determinism pinned via loadSystemFonts:false; 2. do not flatten the per-OS cert-consent copy
(onboarding.js:5-7) into the Book's single "your system will ask you to approve it once" line —
keep per-OS factual accuracy and surface the Book correction to the owner; 3. re-scope the speed
control to restyling the existing native input per recon's "adapt" verdict, or make the
custom-control replacement an explicit owner Ask acknowledging it breaks slider-drive mechanics in
e2e/settings.spec.ts and e2e-webdriver/settings.spec.ts beyond the plan's stated title-constant
lockstep; 4. plan the removal/replacement of the old-brand SVG masters committed in
src-tauri/icons/ so the Phase 5 zero-hit sweep is achievable without an undocumented allowlist)

All four conditions ADOPTED in plan.md v2 (see Decision ledger).

## Findings (condensed transcript)

Verification summary: frozen-identity guard real and untouched; CSP verbatim, no font-src;
frontend/assets gitignored+wiped; include_bytes block at tray.rs:12-37; window titles at
windows.rs:108/128/257; display_text pinned by core/src/server/tests.rs:597; WebDriver pins at
helpers.ts:19 / trust-boundary.spec.ts:22 / smoke.spec.ts:19; branch push fires zero CI (three
push workflows, main-only); AsciiController call sites verified; playground e2e asserts only
#progress; CloudFront extension passthrough; KEEP-list citations verified; verified-sites
displayName uncoupled; gates use real scripts. Architecture sound.

ADVERSARIAL/SECURITY: H-1 per-OS cert copy flattening is a consent-accuracy regression (Linux "no
separate prompt", Windows browser-install, macOS password). M-1 name-mismatch bridging at web→OS
trust handoffs (download/installer, keychain CA prompt, app list) — phishing-grooming pattern;
should be an Ask. M-2 resvg byte-reproducibility requires loadSystemFonts:false + explicit fonts;
version bumps legitimately regenerate bytes. M-3 vendored woff2 need integrity control parity
(sha256 / fontsource-built unmodified; OFL Reserved-Font-Name wrinkle). L-1 prefer
@resvg/resvg-wasm over napi binaries; sharp is a bigger dep, not smaller. L-2 web Google Fonts =
existing posture, note-only. L-3 legacy innerHTML precedent in landing main.ts:128-131 — don't
inherit for new code.

ASSUMPTION-ATTACK: Facts — F3 misstated (25 include_bytes, not 26; icns 10 image chunks + info,
ic04/ic05 ARGB not PNG; ico 7×32bpp BMP; 30 load-bearing files); F4 holds but breakage-set omits
interaction mechanics (slider driven as HTMLInputElement in BOTH e2e suites); F1/F2/F5/F6/F7/F8
verified correct. Inferences — I5 materially incomplete: display_text reword has four companions
(main.rs:728 initial "Status: Idle" literal; byte-identity doc comments at server.rs:66 and
tests.rs:528-529; tray-fed error strings at main.rs:366/820 need explicit KEEP classification).
Asks that should have existed — cert-copy Book correction; bridging copy; slider replace-vs-adapt;
fate of old-brand SVG masters (Phase 5 "allowlist: none" would trip on icon.svg's #d4ff28).

IMPLEMENTATION-CRITIQUE: C-1 (High) hand-written icns/ico spec contradicts committed byte formats;
@tauri-apps/cli ^2 already a devDep → `tauri icon` for the app ladder, resvg only for parametric
tray/og. C-2 (Med) slider: recon said "adapt"; plan silently upgraded to "replace" with real
a11y/test blast radius — restyle instead. C-3 (Med) duplicate master sets (new icon-sources/ vs
committed src-tauri/icons/*.svg) — replace in place. C-4 (Low) gates: root `test` already includes
lint (drop redundant); cargo gates need frontend:build first; landing has NO typecheck (add one or
stop claiming the layer); drop magic-number pass criteria. C-5 (Low) sweep pattern `Inter`
false-positives (IntersectionObserver, "Interactive") — word-boundary/quoted patterns. C-6 (Low)
add color-scheme: light dark + dark theme-color twin. C-7 (Low) tokens must stay defined in
style.css only (prior fix). Outline B beats A nowhere material; optional P3/P4 swap noted
(rejected in ledger: riskiest code earlier = more soak).

RECON CHECK: complied on assets dir, include_bytes, WebDriver lockstep, ~80-literal sweep
(49 rgba lines counted, supports ~80+ with hex/neutral/meta), OS-name rule. Contradictions found:
slider adapt→replace; stepToPhase idle-gap not addressed in dial spec (specify idle-hold);
token-placement ambiguity (C-7).
