# Mega-readiness gap analysis — accelerator GUI app

**Date:** 2026-08-21 · Post-2.0.0-GA · Inputs: phases 1–3 of this plan + release assets of
`accelerator-v2.0.0` + the deferred-item ledger.

## User-journey walkthrough (per OS)

| Stage | macOS | Windows | Linux |
|---|---|---|---|
| Download | Landing derives version from signed feed ✔ | same ✔ | same ✔ |
| Install | Signed + notarized DMG (arm64 launch-validated, Intel verify-only in CI) ✔ | NSIS currentUser installer; silent-install + launch proven in CI ✔ | AppImage **and** .deb (FUSE-less distros covered) ✔ |
| First run | Onboarding wizard, HTTPS pre-checked, real Keychain prompt (owner-verified) ✔ | same via certutil CurrentUser Root ✔ | NSS DBs incl. snap/flatpak Firefox; silent rotation ✔ |
| First prove | Origin prompt-once → permanent Allow (disclosed); verified-sites ✓ ✔ | same ✔ | same ✔ |
| Auto-update | Ed25519 feed + version floor; positive smoke arm64 (+Intel positive) ✔ | full marker transaction; positive+negative smokes ✔ | positive smoke; updater applies AppImage in place ✔ |
| Uninstall | Manual script/runbook | Real uninstaller proof in CI (running-app scenario) ✔ | manual script |

Journey verdict: **no broken stage on any OS.** Remaining journey risks are trust-friction items,
not functional ones.

## Ship-blockers for mainstream usage

**None functional. One trust-blocker stands out:**

1. **B1 — Windows Authenticode signing (deferred from v2 train).** The #1 mainstream-adoption
   blocker on Windows: SmartScreen "Unknown publisher" on first install and on every update
   handshake until reputation accrues. Everything else on Windows is proven. Cost: Azure Trusted
   Signing (~$10/mo, identity vetting delay) or OV cert (~$300/yr) + CI secret handling +
   `sign-update-feed`/NSIS wiring. **Recommend: next engagement's P0.**

## High-value follow-ups (prioritized)

| # | Item | Source | Effort |
|---|---|---|---|
| 2 | Updater tamper-negative legs linux + Intel | test-plan P1 | small CI arcs |
| 3 | Playwright desktop-ui on windows-latest | test-plan P1 | small CI arc |
| 4 | closeout-followups blueprint (Windows ACL tail, popup arbiter refinements, autostart rollback) | existing approved-shape plan | medium; already dual-audited design |
| 5 | SDK↔server identity contract (old F-001/F-002/C11) | security campaign | large; needs owner design decision (per-install secret vs TLS-only) |
| 6 | Playground prod-deploy + source-version bump | v2 train owner-gated leftovers | trivial; owner action |
| 7 | B-1 tray graceful degradation | this audit, bugs report | small refactor arc |
| 8 | B-2 Firefox enterprise-roots hint in Settings | this audit, bugs report | small UX arc |
| 9 | #344 macOS Keychain negative-binding manual smoke | open issue | manual, 30 min |

## Accepted residuals (correctly tracked, no action)

#343 (needs upstream bb signing) · #345 (plugin streaming cap; key-compromise-gated) · F-09
(needs upstream revocation story) · never-armed-copy uninstall residual · until-next-login
launchd gap · headless localhost auto-approve (documented model).

## Bottom line

The app is functionally mega-ready today for technical users on all three OSes. The gap between
"mega-ready" and "mainstream-ready" is almost entirely **install-time trust** (Windows
Authenticode, plus reputation time after it lands) and the two small polish arcs above. Security
posture held up under independent re-verification; the deferred list contains no hidden landmines.
