# Phase 7 — docs + closeout

Fully local (no compile/CI/push needed).

## What shipped
- **docs/PLATFORM_SUPPORT.md**: rewrote the stale claims — Windows is now **Supported** (was "Not supported"), Linux HTTPS is **supported** (was "No HTTPS support"). New **Encrypted Connection (HTTPS)** section with the keyless-CA explanation + a per-OS trust/consent/rotation/uninstall matrix; Linux notes (certutil/libnss3-tools, snap/flatpak-Chromium disclaimer, Firefox-restart). Added a Windows Details section. Kept the merged Chrome-LNA "Browser Notes".
- **packages/accelerator/README.md**: "Safari Support (macOS only)" → "Encrypted Connection (HTTPS)" — cross-OS default-on, per-OS consent, keyless-CA, Remove-certificate-trust, renewal.
- **CLAUDE.md**: current-state SDK bullet (prefers-HTTPS-when-healthy + httpsOnly) and Accelerator bullet (3-OS, HTTPS default-on + wizard, trust module, rename, renewal window, NSIS hook, headless stays TLS-free).
- **packages/sdk/README.md**: preference order + httpsOnly already landed in Phase 2.

## Sweep
Remaining "safari" hits are all legitimate references to the browser (Safari genuinely requires HTTPS) — not the removed "Safari Support" feature name. Verified no code identifier uses `safari_support` (only the serde alias + migration test strings, intentional).

## Deferred (low-value / out of local scope)
- Root README + docs/RELEASE_RUNBOOK.md Windows/renewal/NSIS-verify notes — minor; can fold in a follow-up. RELEASE_RUNBOOK's macOS-trust manual-spike note (I7) is captured in lessons/phase-4.md and the trust_macos.rs module docs.
