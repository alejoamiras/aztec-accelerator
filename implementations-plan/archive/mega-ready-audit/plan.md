# mega-ready-audit

## Outcome

**Closed 2026-09-18 — verified against source, not status text.** deep, **audits COMPLETE 2026-08-21 — stacked draft PRs pending** — solo ox-alpha full-app audit post-2.0.0: ALL prior findings re-verified as claims (every fix held; F-07/F-10 found CLOSED by #446), fresh 9-cluster adversarial pass = no new findings above Low (3 Lows recorded, none fixed by design), cross-OS bug hunt clean (B-1 tray degradation + B-2 Firefox-roots hint recorded), win_acl inline tests shipped (first direct tests for the 402-LOC module), CLAUDE.md counts corrected, readiness roadmap: B1 Authenticode = the one mainstream blocker. Reports: audit/{security,bugs}/2026-08-21-mega-ready-bf234e6/

Confirmed complete by a code-level re-check during the 2026-09 archive pass. The product has since been retired (final native 3.1.0; release workflows disabled; see `docs/PRESTO_RETIREMENT.md`), so nothing here is actionable.

Archived record of what was decided and why. The `/goal` and `/loop` seeds below are retired: do not execute them.


**Date:** 2026-08-21 · **Model:** ox-alpha solo (+ own subagents; no codex/fable) · **Status:** implementing

Full-app audit of `packages/accelerator` (GUI app: `src-tauri` + `core` + `frontend-src`) after the
2.0.0 GA. **Governing principle: no prior audit is treated as truth.** Every prior finding is a
claim to re-verify at source level; claimed fixes get independently re-checked (the fixes were
written by other models and never adversarially re-reviewed); open/deferred findings get
re-assessed on merit — some may be refuted outright.

## Scope

- IN: `packages/accelerator/src-tauri`, `packages/accelerator/core`,
  `packages/accelerator/src-tauri/frontend-src` (+ its tests).
- OUT (owner decision): headless `server` crate, SDK, playground, landing, CI workflows, infra.
- Delta emphasis: everything after audit snapshot `9c4cb0c` (~33 commits / ~8.4k insertions:
  v2 train cohorts B2–B7, autostart arc, binary rename, publisher flip, consent-UX reversal).

## Phases

0. **Homing + recon** — worktree, this plan, architecture map, v2-delta inventory,
   CLAIM ledger of all prior findings.
1. **Security audit** — (a) source-level re-verification of the 10 claimed-fixed findings from
   run `2026-07-31-9c4cb0c`; (b) merit re-assessment of open/deferred (F-07/F-09/F-10,
   issues #343/#344/#345); (c) fresh adversarial pass over 9 clusters:
   C1 server surface · C2 origin-auth + consent · C3 certs/trust · C4 updater chain ·
   C5 bb download/cache/exec · C6 config/migration/lock/win_acl · C7 autostart/crash/uninstall ·
   C8 frontend windows/IPC/CSP · C9 startup sequencer.
   Report → `audit/security/<run-id>/`. Fix confirmed findings.
2. **Cross-OS bug hunt** — clusters by OS surface (macOS / Windows / Linux) + cross-OS lifecycle
   matrix (upgrade 1.0.x→2.x, reinstall-over-running, relocation, port conflicts, cert expiry/
   renewal offline, multi-instance, odd paths/usernames). Report → `audit/bugs/<run-id>/`. Fix.
3. **Test-suite improvement** — consolidate known holes (win_acl zero tests, router/auth wiring,
   ubuntu-only Playwright, missing updater-negative legs, …) + audit discoveries into a
   prioritized plan; implement top tier.
4. **Mega-readiness gap analysis** — per-OS user-journey walkthrough
   (download→install→onboard→first prove→update→uninstall), deferred-item inventory
   (B1 Authenticode, closeout-followups, F-002/C11, #343/#344/#345, playground deploy),
   ship-blocker vs nice-to-have roadmap.
5. **Closeout** — full validation gates, lessons, index update, stack summary.

## Delivery

Single worktree `mega-ready-audit` → stacked draft PRs via `gh stack submit --draft --auto`
after each fix arc. Validation gate before every push: `bun run test` + `bun run lint`;
plus `cargo check --target x86_64-pc-windows-gnu --lib` for any `cfg(windows)` change
(needs gitignored placeholder `binaries/bb-x86_64-pc-windows-gnu.exe`). Commits unsigned on
this machine (interactive 1Password signing skipped during autonomous stretches).

## Ledger

See [ledger.md](ledger.md) for the claim ledger (updated as phases complete).
