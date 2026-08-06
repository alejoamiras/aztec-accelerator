# Scope (Phase 0, confirmed by owner 2026-07-31)

**In scope**
- `packages/accelerator/src-tauri/src` (~9.9k LOC Rust) — desktop app: IPC/commands, updater,
  autostart + crash recovery, update marker, trust/CA, tray/windows, main wiring
- `packages/accelerator/core/src` (~8.0k LOC Rust) — the in-app localhost HTTP/HTTPS listener,
  origin authorization, config, update manifest, prove pipeline
- `packages/accelerator/frontend-src` — the desktop app's own frontend (IPC surface, popup,
  settings, onboarding)
- `packages/sdk/src` (~2.7k LOC TS) — the published `@alejoamiras/aztec-accelerator` package

**Explicitly OUT of scope** (owner's call)
- `packages/accelerator/server` — the separate HEADLESS server crate
- `packages/playground`, `packages/landing`
- `.github/workflows`, `infra/tofu` (CI/CD + IaC left for a separate pass)

**Excluded as non-eligible** (generated / vendored / build output)
- `node_modules/`, `target/`, `dist/`, `packages/accelerator/src-tauri/frontend/assets/**`
  (build output of frontend-src), lockfiles, `audit/` itself

**Prior audits** — treat as CONTEXT, not gospel: `audit/security/2026-07-09-5c788c0`,
`audit/security/2026-06-09-accel-closeout`, `audit/security/2026-06-09-accel-reaudit-7f2a`.
Fresh independent scan; the coordinator cross-references them to flag regressions/reintroductions
and to avoid re-reporting consciously accepted trade-offs.

**Owner-flagged concerns (extra attention)**
1. Local server trust boundary — origin approval / deny-by-default, loopback `Host` allowlist
   (anti-DNS-rebinding), Origin-tiered `/health`.
2. CA trust & HTTPS — keyless local CA name-constrained to loopback, install into macOS Keychain /
   Windows CurrentUser Root / Linux NSS DBs, renewal + rotation, the NSIS uninstall hook.
3. The recent arc — autostart/crash-recovery persistence, update-window marker, NSIS installer
   hooks, the binary rename: a lot of new privileged-path code just landed.

**Effort**: `medium` tier shape (no Phase 2.5 cross-rebuttal; verifier top-5 by severity bucket),
with owner-requested MODEL upgrades: Claude legs on Opus (not Sonnet) and codex at xhigh (not
medium). Deviation documented per the skill's Methodology requirement.
