# Implementations Plans Index

- [maintenance-2026-05-27](maintenance-2026-05-27/plan.md) — completed — non-aztec dep bumps + dependabot removal + headless server release artifacts (PRs #223 #224 #225 merged)
- [release-2026-05-27](release-2026-05-27/plan.md) — completed — automation deprecation + branch cleanup + @aztec 4.2.0 forward-roll + SDK 4.2.0 release + accelerator 1.0.1 release (first headless)
- [ci-dedup-2026-05-28](ci-dedup-2026-05-28/plan.md) — completed — extended setup-accelerator composite to cover release `build` + `build-headless` + `_e2e-webdriver` (PR #230; validated by 1.0.2-rc.2 dry-run)
- [verified-sites-2026-05-28](verified-sites-2026-05-28/plan.md) — in review — friendly name + green ✓ in authorization popup for curated origins (PR #231 open; Nulo extension entry parked pending real Chrome Web Store ID)
- [ci-reliability-2026-05-29](ci-reliability-2026-05-29/plan.md) — completed — WebDriver flake (ungated update check pops prompt mid-E2E after 1.0.2 shipped) + codex post-1.0.2 hardening (PR #236 + #237 merged); see [diagnosis.md](ci-reliability-2026-05-29/diagnosis.md) + [eli5.html](ci-reliability-2026-05-29/eli5.html)
- [ci-speed-2026-05-29](ci-speed-2026-05-29/plan.md) — approved-pending — fix the uncached `_e2e.yml` server build (server→target) + Playwright install reliability (version-keyed cache + retry + timeout)
- [updater-validation-2026-05-29](updater-validation-2026-05-29/plan.md) — approved-pending — release-time gate: install N-1, auto-update to the just-signed N via a local CA+443 feed (no signing key), assert relaunch + /health==N on macOS(arm+intel)+Linux; reproduces the 1.0.1 amfid failure class
